//! The on-disk project format.
//!
//! Its own set of types rather than `#[derive(Serialize)]` on `Timeline` and
//! friends, for three reasons. A clip refers to its source by index into a
//! table, so four hundred clips over three files write three source entries
//! and share three `Arc<Source>` again on load — which the playback layer
//! relies on, since it keys its decoders by path. `Source` is *probe output*
//! rather than something the user authored, and does not belong in a document
//! as though it were. And the schema below can stay a stable contract while
//! the runtime types are still moving.

use std::collections::HashMap;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{Context, anyhow, bail};
use gstreamer::Fraction;
use serde::{Deserialize, Serialize};

use crate::media::Source;
use crate::project::{Edit, Timeline, Track};

/// Bumped when a change here stops older builds from reading newer files.
const VERSION: u32 = 1;

/// Serialise `timeline` to `path`, replacing whatever was there.
pub fn save(timeline: &Timeline, path: &Path) -> anyhow::Result<()> {
    let document = Document::from_timeline(timeline, &base_dir(path));
    let json = serde_json::to_string_pretty(&document).context("could not encode the project")?;

    write_atomically(path, json.as_bytes())
}

/// Read `path` back into a timeline, probing every source it names.
pub fn load(path: &Path) -> anyhow::Result<Timeline> {
    let json =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    let document: Document = serde_json::from_str(&json)
        .with_context(|| format!("{} is not a valid project file", path.display()))?;

    document.into_timeline(&base_dir(path))
}

#[derive(Serialize, Deserialize)]
struct Document {
    version: u32,
    timebase: Rational,
    sources: Vec<SourceEntry>,
    tracks: Vec<TrackEntry>,
}

#[derive(Serialize, Deserialize)]
struct SourceEntry {
    /// Relative to the project file when the media sits under it, so a project
    /// that travels with its media opens on another machine.
    path: PathBuf,
    /// What the file measured as when this project was saved. Every offset
    /// below is a frame index, so these two are what those indices meant.
    fps: Rational,
    duration_ns: u64,
}

#[derive(Serialize, Deserialize)]
struct TrackEntry {
    clips: Vec<ClipEntry>,
}

#[derive(Serialize, Deserialize)]
struct ClipEntry {
    /// Index into `Document::sources`.
    source: usize,
    position: usize,
    source_start: usize,
    length: usize,
}

/// A frame rate, written as `[30000, 1001]` — exact, and readable when someone
/// opens the file to see what a clip is doing.
#[derive(Serialize, Deserialize, Clone, Copy)]
struct Rational(i32, i32);

impl From<Fraction> for Rational {
    fn from(fraction: Fraction) -> Self {
        Rational(fraction.numer(), fraction.denom())
    }
}

impl Rational {
    fn fraction(self) -> anyhow::Result<Fraction> {
        let Rational(numer, denom) = self;

        if numer <= 0 || denom <= 0 {
            bail!("{numer}/{denom} is not a usable frame rate");
        }

        Ok(Fraction::new(numer, denom))
    }
}

impl Document {
    fn from_timeline(timeline: &Timeline, base: &Path) -> Self {
        let mut sources: Vec<SourceEntry> = Vec::new();
        // Two clips of the same file are two clips of one source, whether or
        // not they happen to hold the same `Arc` — path is the identity the
        // rest of the program uses, so it is the identity here too.
        let mut ids: HashMap<&Path, usize> = HashMap::new();
        let mut tracks = Vec::with_capacity(timeline.tracks.len());

        for track in &timeline.tracks {
            let mut clips = Vec::with_capacity(track.clips.len());

            for clip in &track.clips {
                let next = sources.len();
                let source = *ids.entry(&clip.source.path).or_insert(next);

                if source == next {
                    sources.push(SourceEntry {
                        path: relative_to(&clip.source.path, base),
                        fps: clip.source.frame_rate.into(),
                        duration_ns: clip.source.duration.as_nanos() as u64,
                    });
                }

                clips.push(ClipEntry {
                    source,
                    position: clip.position,
                    source_start: clip.source_start,
                    length: clip.length,
                });
            }

            tracks.push(TrackEntry { clips });
        }

        Document {
            version: VERSION,
            timebase: timeline.timebase.into(),
            sources,
            tracks,
        }
    }

    fn into_timeline(self, base: &Path) -> anyhow::Result<Timeline> {
        if self.version > VERSION {
            bail!(
                "this project was written by a newer build (format {}, this build reads {VERSION})",
                self.version
            );
        }

        let sources = self.resolve_sources(base)?;

        let mut timeline = Timeline::default();

        // The track count the file declares, up front: an empty track has to
        // keep its place or every index below it means something else.
        timeline.tracks = vec![Track { clips: Vec::new() }; self.tracks.len()];

        for (t, track) in self.tracks.iter().enumerate() {
            for (c, entry) in track.clips.iter().enumerate() {
                let source: Arc<Source> = sources.get(entry.source).cloned().with_context(|| {
                    format!(
                        "track {t} clip {c} names source {} of {}",
                        entry.source,
                        sources.len()
                    )
                })?;

                let available = source.frame_count();
                if entry.source_start >= available {
                    bail!(
                        "track {t} clip {c} starts at frame {} but {} is {available} frames long",
                        entry.source_start,
                        source.path.display()
                    );
                }

                // A re-probe can round the frame count a frame differently than
                // it did when this was saved. Trim to what is there rather than
                // refuse to open the project over one frame.
                let length = entry.length.min(available - entry.source_start);
                if length != entry.length {
                    eprintln!(
                        "warning: track {t} clip {c} trimmed from {} to {length} frames to fit {}",
                        entry.length,
                        source.path.display()
                    );
                }

                // In through the same door as an edit, so a file cannot
                // describe a document no gesture could have made: an empty
                // clip, an in-point past the end, two clips over each other.
                timeline
                    .apply(Edit::Place {
                        track: t,
                        source,
                        position: entry.position,
                        source_start: entry.source_start,
                        length,
                    })
                    .with_context(|| format!("track {t} clip {c}"))?;
            }
        }

        // What the file says its frames count in, rather than what the first
        // clip placed happened to imply.
        timeline.timebase = self.timebase.fraction().context("bad timebase")?;

        Ok(timeline)
    }

    /// Probe every source up front. A project missing its media should say so
    /// once, listing everything that is gone, rather than failing on whichever
    /// file happens to come first.
    fn resolve_sources(&self, base: &Path) -> anyhow::Result<Vec<Arc<Source>>> {
        let mut sources = Vec::with_capacity(self.sources.len());
        let mut unresolved = Vec::new();

        for entry in &self.sources {
            let path = resolve(&entry.path, base);

            match Source::new(&path) {
                Ok(source) => sources.push(Arc::new(source)),
                Err(e) => unresolved.push(format!("  {}: {e}", path.display())),
            }
        }

        if !unresolved.is_empty() {
            bail!("could not open {} source(s):\n{}", unresolved.len(), unresolved.join("\n"));
        }

        for (entry, source) in self.sources.iter().zip(&sources) {
            let saved = entry.fps.fraction()?;

            // Playback has to use what the file measures now, or the decoder's
            // frame arithmetic would disagree with the pictures coming out of
            // it. So the stored rate is a check, and a disagreement means the
            // offsets below no longer point where they were authored to point.
            if saved != source.frame_rate {
                eprintln!(
                    "warning: {} was {}/{} fps when this project was saved and is {}/{} now — \
                     clip offsets no longer land on the same moments",
                    source.path.display(),
                    saved.numer(),
                    saved.denom(),
                    source.frame_rate.numer(),
                    source.frame_rate.denom(),
                );
            }
        }

        Ok(sources)
    }
}

/// The directory paths in the file are relative to.
fn base_dir(path: &Path) -> PathBuf {
    match path.parent() {
        Some(dir) if !dir.as_os_str().is_empty() => dir.to_path_buf(),
        _ => PathBuf::from("."),
    }
}

fn relative_to(path: &Path, base: &Path) -> PathBuf {
    // Both sides have to be real paths to compare: `base` may be `.`, and the
    // media may have been opened through a relative path or a symlink.
    let (Ok(absolute), Ok(root)) = (path.canonicalize(), base.canonicalize()) else {
        return path.to_path_buf();
    };

    absolute
        .strip_prefix(&root)
        .map(Path::to_path_buf)
        .unwrap_or(absolute)
}

fn resolve(stored: &Path, base: &Path) -> PathBuf {
    if stored.is_absolute() {
        stored.to_path_buf()
    } else {
        base.join(stored)
    }
}

/// Write beside the target and rename over it. A crash halfway through a save
/// then leaves the previous project intact instead of a half-written file.
fn write_atomically(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("{} is not a file", path.display()))?;
    let temp = base_dir(path).join(format!(".{}.saving", name.to_string_lossy()));

    let mut file = File::create(&temp)
        .with_context(|| format!("could not create {}", temp.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("could not write {}", temp.display()))?;
    // The rename is only worth anything once the bytes are on the device.
    file.sync_all()
        .with_context(|| format!("could not flush {}", temp.display()))?;
    drop(file);

    fs::rename(&temp, path)
        .with_context(|| format!("could not move {} into place", temp.display()))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::project::timeline::DEFAULT_TIMEBASE;

    const NTSC: (&str, &str, usize) = ("ntsc.mp4", "30000/1001", 60);
    const PAL: (&str, &str, usize) = ("pal.mp4", "25/1", 50);

    /// Encode a couple of tiny clips beside a project file. The whole job of
    /// this module is a round trip through real media: the frame rates have to
    /// come back off an actual probe for a test of it to mean anything.
    fn fixture(name: &str) -> Option<PathBuf> {
        let dir = std::env::temp_dir().join(format!("cut-{name}"));
        fs::create_dir_all(&dir).ok()?;

        for (file, frame_rate, buffers) in [NTSC, PAL] {
            let target = dir.join(file);

            let status = Command::new("gst-launch-1.0")
                .args([
                    "-q",
                    "videotestsrc",
                    &format!("num-buffers={buffers}"),
                    "!",
                    &format!("video/x-raw,width=320,height=240,framerate={frame_rate}"),
                    "!",
                    "x264enc",
                    "!",
                    "mp4mux",
                    "!",
                    "filesink",
                    &format!("location={}", target.display()),
                ])
                .status();

            match status {
                Ok(status) if status.success() && target.exists() => {}
                _ => {
                    eprintln!("skipping: could not encode {}", target.display());
                    return None;
                }
            }
        }

        Some(dir)
    }

    #[test]
    fn round_trips_a_timeline() {
        gstreamer::init().unwrap();

        let Some(dir) = fixture("round-trip") else {
            return;
        };
        let ntsc = Arc::new(Source::new(dir.join(NTSC.0)).unwrap());
        let pal = Arc::new(Source::new(dir.join(PAL.0)).unwrap());

        // The same source twice, so the round trip has to collapse it back to
        // one entry and hand both clips the same `Arc` again.
        let mut before = Timeline::default();
        for (source, position, source_start, length) in [
            (ntsc.clone(), 0, 4, 10),
            (pal.clone(), 10, 0, 20),
            (ntsc.clone(), 30, 20, 15),
        ] {
            before
                .apply(Edit::Place {
                    track: 0,
                    source,
                    position,
                    source_start,
                    length,
                })
                .unwrap();
        }

        let path = dir.join("project.cut");
        save(&before, &path).unwrap();
        let after = load(&path).unwrap();

        assert_eq!(after.timebase, ntsc.frame_rate);
        assert_eq!(after.tracks.len(), 1);

        let restored = &after.tracks[0].clips;
        let original = &before.tracks[0].clips;
        assert_eq!(restored.len(), original.len());

        for (restored, original) in restored.iter().zip(original) {
            assert_eq!(restored.position, original.position);
            assert_eq!(restored.source_start, original.source_start);
            assert_eq!(restored.length, original.length);
            assert_eq!(restored.source.path, original.source.path);
            assert_eq!(restored.source.frame_rate, original.source.frame_rate);
        }

        assert!(Arc::ptr_eq(&restored[0].source, &restored[2].source));

        let json = fs::read_to_string(&path).unwrap();
        // Two sources for three clips, media addressed relative to the project
        // file, and the rate exact rather than divided out into 29.97.
        assert_eq!(json.matches(".mp4").count(), 2);
        assert!(json.contains("\"ntsc.mp4\""));
        assert!(json.contains("30000"), "{json}");
    }

    #[test]
    fn refuses_a_newer_format() {
        let document = Document {
            version: VERSION + 1,
            timebase: DEFAULT_TIMEBASE.into(),
            sources: Vec::new(),
            tracks: Vec::new(),
        };

        assert!(document.into_timeline(Path::new(".")).is_err());
    }

    #[test]
    fn refuses_a_nonsense_frame_rate() {
        assert!(Rational(30, 0).fraction().is_err());
        assert!(Rational(-30, 1).fraction().is_err());
        assert_eq!(Rational(30000, 1001).fraction().unwrap().numer(), 30000);
    }

    #[test]
    fn resolves_paths_against_the_project() {
        assert_eq!(
            resolve(Path::new("media/a.mp4"), Path::new("/projects/one")),
            PathBuf::from("/projects/one/media/a.mp4")
        );
        // Media outside the project's directory stays where it is.
        assert_eq!(
            resolve(Path::new("/tank/a.mp4"), Path::new("/projects/one")),
            PathBuf::from("/tank/a.mp4")
        );
    }
}
