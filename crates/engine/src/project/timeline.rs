use std::sync::Arc;

use anyhow::{Context, bail};
use gstreamer::Fraction;

use crate::media::Source;
use crate::project::{Clip, ClipId, Edge, Edit, Track};

/// The rate an empty timeline counts in, and the fallback everywhere else a
/// frame rate is unknown.
pub const DEFAULT_TIMEBASE: Fraction = Fraction::new_raw(30, 1);

#[derive(Clone)]
pub struct Timeline {
    /// The rate `Clip::position` and `Clip::length` are counted in. Playback
    /// still maps frames through each clip's own source rate, so today this is
    /// only declared and carried — it is what mixed-rate sources will be
    /// conformed *to*, and it has to survive a save to be worth anything.
    pub timebase: Fraction,
    pub tracks: Vec<Track>,
    /// The last id handed out.
    ids: u64,
}

impl Default for Timeline {
    fn default() -> Self {
        Timeline {
            timebase: DEFAULT_TIMEBASE,
            tracks: Vec::new(),
            ids: 0,
        }
    }
}

impl Timeline {
    /// Change the document.
    ///
    /// Every edit comes through here, so the invariants a project file is
    /// checked against when it loads — a clip with frames in it, an in-point
    /// its source can answer for, no two clips over each other on one track —
    /// cannot be broken by a gesture in the first place. An edit that would
    /// break one does nothing and says why, which for a drag that has gone
    /// somewhere impossible is the whole answer.
    pub fn apply(&mut self, edit: Edit) -> anyhow::Result<()> {
        match edit {
            Edit::Place {
                track,
                source,
                position,
                source_start,
                length,
            } => {
                self.place(track, source, position, source_start, length)?;
            }

            Edit::Append(source) => {
                // Checked before anything moves: `place` cannot then fail, and
                // an edit that fails has to leave the document alone.
                let length = frames(&source)?;
                let position = self.length();

                self.place(0, source, position, 0, length)?;
            }

            Edit::Insert { source, frame } => {
                let length = frames(&source)?;
                let at = self.nearest_edge(frame);

                self.ripple(at, length as isize);
                self.place(0, source, at, 0, length)?;
            }

            Edit::Move {
                clip,
                track,
                position,
            } => {
                let (from, index) = self.locate(clip)?;
                let length = self.tracks[from].clips[index].length;

                if track > self.tracks.len() {
                    bail!("there is no track {track}");
                }
                if let Some(over) = self.covering(track, position, length, Some(clip)) {
                    bail!("that would land on the clip at frame {over}");
                }

                let mut moved = self.tracks[from].clips.remove(index);
                moved.position = position;
                self.insert(track, moved);
            }

            Edit::Trim { clip, edge, frame } => {
                let (t, index) = self.locate(clip)?;
                let trimmed = &self.tracks[t].clips[index];
                let available = trimmed.source.frame_count();

                let (position, source_start, length) = match edge {
                    Edge::In => {
                        if frame >= trimmed.end() {
                            bail!("an in-point has to come before its out-point");
                        }

                        // The window into the source moves with the in-point;
                        // the out-point stays where it is. These two have to
                        // move together or the clip plays the wrong frames.
                        let shift = frame as isize - trimmed.position as isize;
                        let source_start = trimmed.source_start as isize + shift;

                        if source_start < 0 {
                            bail!(
                                "{} has no frames before its start",
                                trimmed.source.path.display()
                            );
                        }

                        (frame, source_start as usize, trimmed.end() - frame)
                    }
                    Edge::Out => {
                        if frame <= trimmed.position {
                            bail!("an out-point has to come after its in-point");
                        }

                        let length = frame - trimmed.position;

                        if trimmed.source_start + length > available {
                            bail!(
                                "{} runs out after {} more frames",
                                trimmed.source.path.display(),
                                available - trimmed.source_start
                            );
                        }

                        (trimmed.position, trimmed.source_start, length)
                    }
                };

                if let Some(over) = self.covering(t, position, length, Some(clip)) {
                    bail!("that would run into the clip at frame {over}");
                }

                let trimmed = &mut self.tracks[t].clips[index];
                trimmed.position = position;
                trimmed.source_start = source_start;
                trimmed.length = length;

                // Trimming the in-point moves the clip, which can reorder it.
                self.tracks[t].clips.sort_by_key(|clip| clip.position);
            }

            Edit::Split { clip, frame } => {
                let (t, index) = self.locate(clip)?;
                let split = &self.tracks[t].clips[index];

                if frame <= split.position || frame >= split.end() {
                    bail!("frame {frame} is not inside that clip");
                }

                // Read the halves out before minting, which needs the document
                // back.
                let head = frame - split.position;
                let source = split.source.clone();
                let source_start = split.source_frame(frame);
                let length = split.end() - frame;

                let id = self.mint();
                let clips = &mut self.tracks[t].clips;
                clips[index].length = head;
                clips.insert(
                    index + 1,
                    Clip {
                        id,
                        source,
                        position: frame,
                        source_start,
                        length,
                    },
                );
            }

            Edit::Delete { clip, ripple } => {
                let (t, index) = self.locate(clip)?;
                let removed = self.tracks[t].clips.remove(index);

                if ripple {
                    self.ripple(removed.end(), -(removed.length as isize));
                }
            }
        }

        Ok(())
    }

    pub fn length(&self) -> usize {
        self.tracks
            .iter()
            .flat_map(|track| &track.clips)
            .map(Clip::end)
            .max()
            .unwrap_or(0)
    }

    pub fn clips_at(&self, frame: usize) -> Vec<&Clip> {
        self.tracks.iter().flat_map(|t| t.clip_at(frame)).collect()
    }

    /// Whether `length` frames could sit at `position` on `track` without
    /// running into anything — what a drag has to know before it lands, asked
    /// of the document so the answer cannot drift from what `apply` allows.
    pub fn has_room(&self, track: usize, position: usize, length: usize, ignore: ClipId) -> bool {
        self.covering(track, position, length, Some(ignore)).is_none()
    }

    /// The first frame at or after `frame` that a clip covers, wrapping round to
    /// wherever the document starts. `None` when there is nothing to play.
    ///
    /// Moving or deleting a clip leaves a hole with nothing in it to decode, so
    /// playback needs somewhere to land other than the middle of one.
    pub fn next_content(&self, frame: usize) -> Option<usize> {
        let clips = || self.tracks.iter().flat_map(|track| &track.clips);

        clips()
            .filter_map(|clip| match clip {
                clip if clip.position <= frame && frame < clip.end() => Some(frame),
                clip if clip.position > frame => Some(clip.position),
                _ => None,
            })
            .min()
            // Nothing at or after it: round to the front.
            .or_else(|| clips().map(|clip| clip.position).min())
    }

    /// The clip edge nearest `frame` on the first track, counting the start of
    /// the document and the end of every clip.
    pub fn nearest_edge(&self, frame: usize) -> usize {
        let edges = self
            .tracks
            .first()
            .into_iter()
            .flat_map(|track| &track.clips)
            .flat_map(|clip| [clip.position, clip.end()]);

        std::iter::once(0)
            .chain(edges)
            .min_by_key(|edge| edge.abs_diff(frame))
            .unwrap_or(0)
    }

    /// The one way a clip comes into the document, and so the one place an id
    /// is minted.
    fn place(
        &mut self,
        track: usize,
        source: Arc<Source>,
        position: usize,
        source_start: usize,
        length: usize,
    ) -> anyhow::Result<ClipId> {
        if length == 0 {
            bail!("a clip needs at least one frame");
        }

        let available = source.frame_count();
        if source_start + length > available {
            bail!(
                "frames {source_start}..{} are past the end of {}, which has {available}",
                source_start + length,
                source.path.display()
            );
        }

        if track > self.tracks.len() {
            bail!("there is no track {track}");
        }
        if let Some(over) = self.covering(track, position, length, None) {
            bail!("that would land on the clip at frame {over}");
        }

        // The first media in an empty document decides what its frames count
        // in. Loading a project sets the timebase itself, afterwards, from what
        // the file says.
        if self.tracks.iter().all(|track| track.clips.is_empty()) {
            self.timebase = source.frame_rate;
        }

        let id = self.mint();
        self.insert(
            track,
            Clip {
                id,
                source,
                position,
                source_start,
                length,
            },
        );

        Ok(id)
    }

    /// Put `clip` on `track` in position order, starting the track if it is one
    /// past the last.
    fn insert(&mut self, track: usize, clip: Clip) {
        if track == self.tracks.len() {
            self.tracks.push(Track { clips: Vec::new() });
        }

        let clips = &mut self.tracks[track].clips;
        let index = clips.partition_point(|existing| existing.position <= clip.position);
        clips.insert(index, clip);
    }

    /// Shift everything from `frame` on by `by`, on every track, so a ripple
    /// keeps the tracks lined up with each other.
    fn ripple(&mut self, frame: usize, by: isize) {
        for track in &mut self.tracks {
            for clip in &mut track.clips {
                if clip.position >= frame {
                    clip.position = clip.position.saturating_add_signed(by);
                }
            }
        }
    }

    /// Where `id` is: which track, and where in that track's clips.
    fn locate(&self, id: ClipId) -> anyhow::Result<(usize, usize)> {
        self.tracks
            .iter()
            .enumerate()
            .find_map(|(t, track)| {
                track
                    .clips
                    .iter()
                    .position(|clip| clip.id == id)
                    .map(|index| (t, index))
            })
            .context("that clip is not in the timeline")
    }

    /// Where a clip already on `track` runs through `position..position + length`,
    /// if one does. `ignore` is the clip being moved, which cannot be in its own
    /// way.
    ///
    /// Leans on what `apply` guarantees: clips on a track are in position order
    /// and never overlap, so their ends rise with their positions and the only
    /// candidates are the few starting before `end`. Every drag will ask this
    /// question once per mouse move, so it is worth not walking the track.
    fn covering(
        &self,
        track: usize,
        position: usize,
        length: usize,
        ignore: Option<ClipId>,
    ) -> Option<usize> {
        let clips = &self.tracks.get(track)?.clips;
        let end = position + length;
        let first = clips.partition_point(|clip| clip.end() <= position);

        clips[first..]
            .iter()
            .take_while(|clip| clip.position < end)
            .find(|clip| Some(clip.id) != ignore)
            .map(|clip| clip.position)
    }

    fn mint(&mut self) -> ClipId {
        self.ids += 1;

        ClipId(self.ids)
    }
}

/// How many frames of `source` there are to work with.
fn frames(source: &Source) -> anyhow::Result<usize> {
    match source.frame_count() {
        0 => bail!("{} has no video frames", source.path.display()),
        frames => Ok(frames),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use super::*;

    /// A source is plain measurements, so a test can state them outright
    /// instead of encoding a file to be probed. 30 fps, so seconds are frames
    /// over thirty.
    fn source(seconds: u64) -> Arc<Source> {
        Arc::new(Source {
            path: PathBuf::from(format!("{seconds}s.mp4")),
            duration: Duration::from_secs(seconds),
            frame_rate: Fraction::new(30, 1),
        })
    }

    /// `(position, source_start, length)` per clip on a track.
    fn windows(timeline: &Timeline, track: usize) -> Vec<(usize, usize, usize)> {
        timeline.tracks[track]
            .clips
            .iter()
            .map(|clip| (clip.position, clip.source_start, clip.length))
            .collect()
    }

    fn ids(timeline: &Timeline) -> Vec<ClipId> {
        timeline.tracks[0].clips.iter().map(|clip| clip.id).collect()
    }

    /// Two clips, 60 frames then 30, back to back from frame zero.
    fn pair() -> Timeline {
        let mut timeline = Timeline::default();
        timeline.apply(Edit::Append(source(2))).unwrap();
        timeline.apply(Edit::Append(source(1))).unwrap();

        timeline
    }

    #[test]
    fn appends_after_everything_already_there() {
        let timeline = pair();

        assert_eq!(windows(&timeline, 0), vec![(0, 0, 60), (60, 0, 30)]);
        assert_eq!(timeline.length(), 90);
        // An empty document takes its timebase from the first media in it.
        assert_eq!(timeline.timebase, Fraction::new(30, 1));
    }

    #[test]
    fn ids_are_unique_and_survive_edits() {
        let mut timeline = pair();
        let before = ids(&timeline);

        timeline.apply(Edit::Insert { source: source(1), frame: 0 }).unwrap();

        // The two original clips kept their ids through the ripple; the new one
        // got an id of its own.
        let after = ids(&timeline);
        assert_eq!(after[1..], before[..]);
        assert!(!before.contains(&after[0]));
    }

    #[test]
    fn insert_ripples_what_follows_and_snaps_to_an_edge() {
        let mut timeline = pair();

        // Frame 55 is inside the first clip; the edge at 60 is the closest, so
        // the new clip lands between the two rather than inside either.
        timeline.apply(Edit::Insert { source: source(1), frame: 55 }).unwrap();

        assert_eq!(
            windows(&timeline, 0),
            vec![(0, 0, 60), (60, 0, 30), (90, 0, 30)]
        );
    }

    #[test]
    fn move_rejects_a_landing_that_is_taken() {
        let mut timeline = pair();
        let second = ids(&timeline)[1];

        // 30..60 is still the first clip's.
        assert!(
            timeline
                .apply(Edit::Move { clip: second, track: 0, position: 30 })
                .is_err()
        );
        assert_eq!(windows(&timeline, 0), vec![(0, 0, 60), (60, 0, 30)]);

        // Past the end is free.
        timeline
            .apply(Edit::Move { clip: second, track: 0, position: 200 })
            .unwrap();
        assert_eq!(windows(&timeline, 0), vec![(0, 0, 60), (200, 0, 30)]);
    }

    #[test]
    fn move_onto_the_track_past_the_last_starts_one() {
        let mut timeline = pair();
        let second = ids(&timeline)[1];

        timeline
            .apply(Edit::Move { clip: second, track: 1, position: 0 })
            .unwrap();

        assert_eq!(windows(&timeline, 0), vec![(0, 0, 60)]);
        assert_eq!(windows(&timeline, 1), vec![(0, 0, 30)]);
        // A track further down than that does not exist to move onto.
        assert!(
            timeline
                .apply(Edit::Move { clip: second, track: 3, position: 0 })
                .is_err()
        );
    }

    #[test]
    fn trimming_the_in_point_moves_the_window_with_it() {
        let mut timeline = pair();
        let first = ids(&timeline)[0];

        timeline
            .apply(Edit::Trim { clip: first, edge: Edge::In, frame: 10 })
            .unwrap();

        // Ten frames later in the timeline is ten frames later in the source,
        // and the out-point has not moved.
        assert_eq!(windows(&timeline, 0)[0], (10, 10, 50));
    }

    #[test]
    fn a_trim_cannot_reach_past_its_source() {
        let mut timeline = Timeline::default();
        timeline.apply(Edit::Append(source(1))).unwrap();
        let clip = ids(&timeline)[0];

        // The whole source is already in the clip: there is nothing before its
        // first frame, and nothing after its last.
        assert!(
            timeline
                .apply(Edit::Trim { clip, edge: Edge::In, frame: 0 })
                .is_ok()
        );
        timeline
            .apply(Edit::Trim { clip, edge: Edge::In, frame: 5 })
            .unwrap();
        assert!(
            timeline
                .apply(Edit::Trim { clip, edge: Edge::Out, frame: 40 })
                .is_err()
        );
        // And an out-point cannot cross its in-point.
        assert!(
            timeline
                .apply(Edit::Trim { clip, edge: Edge::Out, frame: 5 })
                .is_err()
        );
        assert_eq!(windows(&timeline, 0)[0], (5, 5, 25));
    }

    #[test]
    fn split_hands_the_tail_the_right_frames() {
        let mut timeline = Timeline::default();
        timeline
            .apply(Edit::Place {
                track: 0,
                source: source(2),
                position: 0,
                source_start: 10,
                length: 50,
            })
            .unwrap();
        let clip = ids(&timeline)[0];

        timeline.apply(Edit::Split { clip, frame: 20 }).unwrap();

        // The tail picks up in the source exactly where the head left off.
        assert_eq!(windows(&timeline, 0), vec![(0, 10, 20), (20, 30, 30)]);
        // A cut has to fall inside the clip to mean anything.
        assert!(timeline.apply(Edit::Split { clip, frame: 0 }).is_err());
    }

    #[test]
    fn delete_either_leaves_the_gap_or_closes_it() {
        let mut timeline = pair();
        let first = ids(&timeline)[0];

        timeline.apply(Edit::Delete { clip: first, ripple: false }).unwrap();
        assert_eq!(windows(&timeline, 0), vec![(60, 0, 30)]);

        let mut timeline = pair();
        let first = ids(&timeline)[0];

        timeline.apply(Edit::Delete { clip: first, ripple: true }).unwrap();
        assert_eq!(windows(&timeline, 0), vec![(0, 0, 30)]);

        // The clip is gone, so naming it again is not an edit.
        assert!(
            timeline
                .apply(Edit::Delete { clip: first, ripple: false })
                .is_err()
        );
    }

    #[test]
    fn next_content_steps_over_a_gap() {
        let mut timeline = pair();
        let first = ids(&timeline)[0];
        timeline
            .apply(Edit::Delete {
                clip: first,
                ripple: false,
            })
            .unwrap();

        // Frame 0..60 is now a hole: playback has to land on 60.
        assert_eq!(timeline.next_content(0), Some(60));
        assert_eq!(timeline.next_content(70), Some(70));
        // Past the end it wraps to the front, which is where the loop goes.
        assert_eq!(timeline.next_content(90), Some(60));
        assert_eq!(Timeline::default().next_content(0), None);
    }

    #[test]
    fn has_room_ignores_the_clip_asking() {
        let timeline = pair();
        let (first, second) = (ids(&timeline)[0], ids(&timeline)[1]);

        // Where the second clip already is, asked by the first: taken.
        assert!(!timeline.has_room(0, 60, 30, first));
        // Asked by the clip that is already there: its own place is free.
        assert!(timeline.has_room(0, 60, 30, second));
        assert!(timeline.has_room(0, 90, 30, first));
    }

    #[test]
    fn a_rejected_edit_changes_nothing() {
        let mut timeline = pair();
        let before = windows(&timeline, 0);

        // A source with no frames in it, at the end and rippled in.
        let empty = Arc::new(Source {
            path: PathBuf::from("silence.wav"),
            duration: Duration::from_secs(3),
            frame_rate: Fraction::new(0, 1),
        });

        assert!(timeline.apply(Edit::Append(empty.clone())).is_err());
        assert!(
            timeline
                .apply(Edit::Insert { source: empty, frame: 30 })
                .is_err()
        );
        assert_eq!(windows(&timeline, 0), before);
    }
}
