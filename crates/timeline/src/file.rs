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
use serde::{Deserialize, Serialize};

use crate::{Edit, Rational, Source, Timeline, Track};

/// Bumped when a change here stops older builds from reading newer files.
const VERSION: u32 = 1;

/// Serialise `timeline` to `path`, replacing whatever was there.
pub fn save(timeline: &Timeline, path: &Path) -> anyhow::Result<()> {
    let document = Document::from_timeline(timeline, &base_dir(path));
    let json = serde_json::to_string_pretty(&document).context("could not encode the project")?;

    write_atomically(path, json.as_bytes())
}

/// How a path becomes a [`Source`]. Passed in because opening media is
/// `cut-media`'s job, and this crate does not depend on it.
pub type Probe<'a> = &'a dyn Fn(&Path) -> anyhow::Result<Source>;

/// Read `path` back into a timeline, probing every source it names.
pub fn load(path: &Path, probe: Probe) -> anyhow::Result<Timeline> {
    let json =
        fs::read_to_string(path).with_context(|| format!("could not read {}", path.display()))?;
    let document: Document = serde_json::from_str(&json)
        .with_context(|| format!("{} is not a valid project file", path.display()))?;

    document.into_timeline(&base_dir(path), probe)
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
                        fps: clip.source.frame_rate,
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
            timebase: timeline.timebase,
            sources,
            tracks,
        }
    }

    fn into_timeline(self, base: &Path, probe: Probe) -> anyhow::Result<Timeline> {
        if self.version > VERSION {
            bail!(
                "this project was written by a newer build (format {}, this build reads {VERSION})",
                self.version
            );
        }

        let sources = self.resolve_sources(base, probe)?;

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
        timeline.timebase = self.timebase.checked().context("bad timebase")?;

        Ok(timeline)
    }

    /// Probe every source up front, so a project missing its media says so
    /// once and lists all of it.
    fn resolve_sources(&self, base: &Path, probe: Probe) -> anyhow::Result<Vec<Arc<Source>>> {
        let mut sources = Vec::with_capacity(self.sources.len());
        let mut unresolved = Vec::new();

        for entry in &self.sources {
            let path = resolve(&entry.path, base);

            match probe(&path) {
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
            let saved = entry.fps.checked()?;

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
    use super::*;
    use crate::DEFAULT_TIMEBASE;

    /// Nothing here names a source, so nothing is ever opened.
    fn no_media(path: &Path) -> anyhow::Result<Source> {
        bail!("{} should not have been probed", path.display())
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
            timebase: DEFAULT_TIMEBASE,
            sources: Vec::new(),
            tracks: Vec::new(),
        };

        assert!(document.into_timeline(Path::new("."), &no_media).is_err());
    }

    #[test]
    fn refuses_a_nonsense_frame_rate() {
        assert!(Rational::new_raw(30, 0).checked().is_err());
        assert!(Rational::new_raw(-30, 1).checked().is_err());
        assert_eq!(
            Rational::new_raw(30000, 1001).checked().unwrap().numer(),
            30000
        );
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
