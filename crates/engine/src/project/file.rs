//! The on-disk project format: its own types, so clips can share a source
//! table — playback keys its decoders by path and needs those `Arc`s shared
//! again on load — and so the schema stays stable while the runtime types
//! move.

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
    /// Relative when the media sits under the project, so the two travel
    /// together.
    path: PathBuf,
    /// What the file measured as when this was saved: every offset below is a
    /// frame index, and these are what those indices meant.
    fps: Rational,
    duration_ns: u64,
}

#[derive(Serialize, Deserialize)]
struct TrackEntry {
    /// Absent before tracks could be switched off, and those all played.
    #[serde(default = "playing")]
    enabled: bool,
    clips: Vec<ClipEntry>,
}

fn playing() -> bool {
    true
}

#[derive(Serialize, Deserialize)]
struct ClipEntry {
    /// Index into `Document::sources`.
    source: usize,
    position: usize,
    source_start: usize,
    length: usize,
}

/// A frame rate, written as `[30000, 1001]`: exact, and readable.
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
        // Path is the identity the rest of the program uses, whether or not
        // two clips of one file happen to hold the same `Arc`.
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

            tracks.push(TrackEntry {
                enabled: track.enabled,
                clips,
            });
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

        // Up front: an empty track keeps its place, or every index below it
        // means something else.
        timeline.tracks = vec![Track::default(); self.tracks.len()];

        for (t, track) in self.tracks.iter().enumerate() {
            timeline.tracks[t].enabled = track.enabled;

            for (c, entry) in track.clips.iter().enumerate() {
                let source: Arc<Source> =
                    sources.get(entry.source).cloned().with_context(|| {
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

                // A re-probe can round the frame count differently. Trim to
                // what is there rather than refuse over one frame.
                let length = entry.length.min(available - entry.source_start);
                if length != entry.length {
                    eprintln!(
                        "warning: track {t} clip {c} trimmed from {} to {length} frames to fit {}",
                        entry.length,
                        source.path.display()
                    );
                }

                // In through the same door as an edit, so a file cannot
                // describe a document no gesture could have made.
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

    /// Probe every source up front, so a project missing its media says so
    /// once and lists all of it.
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
            bail!(
                "could not open {} source(s):\n{}",
                unresolved.len(),
                unresolved.join("\n")
            );
        }

        for (entry, source) in self.sources.iter().zip(&sources) {
            let saved = entry.fps.fraction()?;

            // Playback has to use what the file measures now, so the stored
            // rate is only a check — a disagreement means the offsets no
            // longer land where they were authored to.
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
    // Both sides have to be real to compare: `base` may be `.`, and the media
    // may have come through a relative path or a symlink.
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

/// Write beside the target and rename over it, so a crash mid-save leaves the
/// previous project intact.
fn write_atomically(path: &Path, bytes: &[u8]) -> anyhow::Result<()> {
    let name = path
        .file_name()
        .ok_or_else(|| anyhow!("{} is not a file", path.display()))?;
    let temp = base_dir(path).join(format!(".{}.saving", name.to_string_lossy()));

    let mut file =
        File::create(&temp).with_context(|| format!("could not create {}", temp.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("could not write {}", temp.display()))?;
    // The rename is only worth anything once the bytes are on the device.
    file.sync_all()
        .with_context(|| format!("could not flush {}", temp.display()))?;
    drop(file);

    fs::rename(&temp, path).with_context(|| format!("could not move {} into place", temp.display()))
}

#[cfg(test)]
mod tests {
    use std::process::Command;

    use super::*;
    use crate::project::timeline::DEFAULT_TIMEBASE;

    const NTSC: (&str, &str, usize) = ("ntsc.mp4", "30000/1001", 60);
    const PAL: (&str, &str, usize) = ("pal.mp4", "25/1", 50);

    /// Two tiny clips beside a project file: the rates have to come back off
    /// a real probe for a round trip to mean anything.
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

        // The same source twice: the round trip has to collapse it to one
        // entry and share the `Arc` again.
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

        // Two sources for three clips, addressed relative to the project, at
        // an exact rate rather than 29.97.
        let json = fs::read_to_string(&path).unwrap();
        assert_eq!(json.matches(".mp4").count(), 2);
        assert!(json.contains("\"ntsc.mp4\""));
        assert!(json.contains("30000"), "{json}");
    }

    #[test]
    fn keeps_a_track_switched_off() {
        gstreamer::init().unwrap();

        let Some(dir) = fixture("switched-off") else {
            return;
        };
        let ntsc = Arc::new(Source::new(dir.join(NTSC.0)).unwrap());

        let mut before = Timeline::default();
        before
            .apply(Edit::Place {
                track: 0,
                source: ntsc,
                position: 0,
                source_start: 0,
                length: 10,
            })
            .unwrap();
        before
            .apply(Edit::Enable {
                track: 0,
                enabled: false,
            })
            .unwrap();

        let path = dir.join("off.cut");
        save(&before, &path).unwrap();

        assert!(!load(&path).unwrap().tracks[0].enabled);
    }

    #[test]
    fn a_track_written_before_the_flag_plays() {
        let entry: TrackEntry = serde_json::from_str(r#"{"clips":[]}"#).unwrap();

        assert!(entry.enabled);
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
