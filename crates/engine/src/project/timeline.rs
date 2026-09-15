use std::sync::Arc;

use anyhow::{Context, bail};
use gstreamer::Fraction;

use crate::media::Source;
use crate::project::{Clip, ClipId, Edge, Edit, Placement, Track};

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
    /// The document as this edit would leave it, or why it cannot.
    ///
    /// The copy is what makes a refusal clean: an edit over a set of clips can
    /// give up part-way, and what it had already moved has to be thrown away
    /// rather than left behind. Callers hold the document behind an `Arc` and
    /// need an owned one to publish anyway, so this is the copy they were
    /// going to make regardless.
    pub fn applied(&self, edit: Edit) -> anyhow::Result<Timeline> {
        let mut next = self.clone();
        next.apply(edit)?;

        Ok(next)
    }

    /// Change the document in place.
    ///
    /// Every edit comes through here, so the invariants a project file is
    /// checked against when it loads — a clip with frames in it, an in-point
    /// its source can answer for, no two clips over each other on one track —
    /// cannot be broken by a gesture in the first place. An edit that would
    /// break one says why.
    ///
    /// It may stop half-way: a set of clips is dealt with one at a time, and
    /// the first refusal abandons the rest. Anything that cannot live with
    /// that wants `applied` — which is everything except the bulk loads that
    /// build a document from nothing.
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

            Edit::Move(clips) => self.relocate(&clips)?,

            Edit::Trim(edges) => {
                for edge in edges {
                    self.trim(edge.clip, edge.edge, edge.frame)?;
                }
            }

            Edit::Split { clips, frame } => {
                for clip in clips {
                    self.split(clip, frame)?;
                }
            }

            Edit::Delete { clips, ripple } => {
                for clip in clips {
                    let (t, index) = self.locate(clip)?;
                    let removed = self.tracks[t].clips.remove(index);

                    if ripple {
                        self.ripple(removed.end(), -(removed.length as isize));
                    }
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

    /// Move one end of one clip. The body of `Edit::Trim`, kept apart so a
    /// group trim runs the same checks rather than a second copy of them.
    fn trim(&mut self, clip: ClipId, edge: Edge, frame: usize) -> anyhow::Result<()> {
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

        // Lifted before the room is judged, the way a move is: a clip growing
        // by a frame is otherwise found to be in its own way.
        let mut trimmed = self.tracks[t].clips.remove(index);
        self.clear_span(t, position, position + length)?;

        trimmed.position = position;
        trimmed.source_start = source_start;
        trimmed.length = length;
        self.insert(t, trimmed);

        Ok(())
    }

    /// Cut a clip in two. The head keeps its id, the tail gets a new one.
    fn split(&mut self, clip: ClipId, frame: usize) -> anyhow::Result<()> {
        let (t, index) = self.locate(clip)?;
        let split = &self.tracks[t].clips[index];

        if frame <= split.position || frame >= split.end() {
            bail!("frame {frame} is not inside that clip");
        }

        // Read the halves out before minting, which needs the document back.
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

        Ok(())
    }

    /// Make room between `from` and `to` on `track` by shortening whatever is
    /// already there.
    ///
    /// The overwrite rule, and the one place it is spelled out: a clip the
    /// span covers at one end is trimmed back to meet it, one covered down
    /// the middle is split around it, and one covered end to end is removed —
    /// there is nothing left of it to keep. A trim only narrows the window
    /// into a source, so undoing any of this gives the frames back.
    fn clear_span(&mut self, track: usize, from: usize, to: usize) -> anyhow::Result<()> {
        let Some(lane) = self.tracks.get(track) else {
            return Ok(());
        };

        // Read the overlaps out first: every branch below reshapes the track.
        let overlapping: Vec<(ClipId, usize, usize)> = lane
            .clips
            .iter()
            .filter(|clip| clip.position < to && from < clip.end())
            .map(|clip| (clip.id, clip.position, clip.end()))
            .collect();

        for (id, position, end) in overlapping {
            match (position < from, end > to) {
                // Covered end to end.
                (false, false) => self.tracks[track].clips.retain(|clip| clip.id != id),
                // Covered down the middle: a piece is left either side.
                (true, true) => {
                    self.split(id, from)?;

                    let (_, index) = self.locate(id)?;
                    let tail = self.tracks[track].clips[index + 1].id;
                    self.trim(tail, Edge::In, to)?;
                }
                // Covered at one end or the other.
                (true, false) => self.trim(id, Edge::Out, from)?,
                (false, true) => self.trim(id, Edge::In, to)?,
            }
        }

        Ok(())
    }

    /// Put clips where they are named.
    ///
    /// Every clip comes out before any goes back in, which is the whole
    /// reason a move takes a set: a group shuffling within its own span would
    /// otherwise be refused by the room it is itself about to vacate.
    fn relocate(&mut self, clips: &[Placement]) -> anyhow::Result<()> {
        let mut lifted = Vec::with_capacity(clips.len());
        for placement in clips {
            let (from, index) = self.locate(placement.clip)?;
            lifted.push((placement, self.tracks[from].clips.remove(index)));
        }

        for (placement, mut clip) in lifted {
            let (track, position) = (placement.track, placement.position);
            if track > self.tracks.len() {
                bail!("there is no track {track}");
            }

            // The movers are already lifted out, so what is in the way here is
            // only ever a clip staying put — which is what makes it safe to
            // shorten one without checking whether it is about to move too.
            self.clear_span(track, position, position + clip.length)?;

            clip.position = position;
            self.insert(track, clip);
        }

        Ok(())
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
    use crate::project::TrimTo;

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
        timeline.tracks[0]
            .clips
            .iter()
            .map(|clip| clip.id)
            .collect()
    }

    /// The one-clip forms, which most tests want and every edit now spells
    /// as a set.
    fn moved(clip: ClipId, track: usize, position: usize) -> Edit {
        Edit::Move(vec![Placement {
            clip,
            track,
            position,
        }])
    }

    fn trimmed(clip: ClipId, edge: Edge, frame: usize) -> Edit {
        Edit::Trim(vec![TrimTo { clip, edge, frame }])
    }

    fn cut(clip: ClipId, frame: usize) -> Edit {
        Edit::Split {
            clips: vec![clip],
            frame,
        }
    }

    fn deleted(clip: ClipId, ripple: bool) -> Edit {
        Edit::Delete {
            clips: vec![clip],
            ripple,
        }
    }

    /// Two clips, 60 frames then 30, back to back from frame zero.
    fn pair() -> Timeline {
        let mut timeline = Timeline::default();
        timeline.apply(Edit::Append(source(2))).unwrap();
        timeline.apply(Edit::Append(source(1))).unwrap();

        timeline
    }

    #[test]
    fn a_group_moves_into_its_own_old_span() {
        let mut timeline = pair();
        let group = ids(&timeline);

        // The first clip is put exactly where the second one still is. One
        // clip at a time could not do this; lifting both out first can.
        timeline
            .apply(Edit::Move(vec![
                Placement {
                    clip: group[0],
                    track: 0,
                    position: 30,
                },
                Placement {
                    clip: group[1],
                    track: 0,
                    position: 90,
                },
            ]))
            .unwrap();

        assert_eq!(windows(&timeline, 0), vec![(30, 0, 60), (90, 0, 30)]);
    }

    #[test]
    fn a_group_move_overwrites_what_it_lands_on() {
        let mut timeline = pair();
        // A third clip at 90..120 that is not part of the group.
        timeline.apply(Edit::Append(source(1))).unwrap();
        let group = ids(&timeline)[..2].to_vec();

        timeline
            .apply(Edit::Move(vec![
                Placement {
                    clip: group[0],
                    track: 0,
                    position: 30,
                },
                Placement {
                    clip: group[1],
                    track: 0,
                    position: 90,
                },
            ]))
            .unwrap();

        // The second clip lands exactly over the third, which is covered end
        // to end and goes.
        assert_eq!(windows(&timeline, 0), vec![(30, 0, 60), (90, 0, 30)]);
        assert_eq!(ids(&timeline), group);
    }

    #[test]
    fn a_group_moves_down_a_track() {
        let mut timeline = pair();
        let group = ids(&timeline);

        timeline
            .apply(Edit::Move(vec![
                Placement {
                    clip: group[0],
                    track: 1,
                    position: 0,
                },
                Placement {
                    clip: group[1],
                    track: 1,
                    position: 60,
                },
            ]))
            .unwrap();

        assert!(timeline.tracks[0].clips.is_empty());
        assert_eq!(windows(&timeline, 1), vec![(0, 0, 60), (60, 0, 30)]);
    }

    #[test]
    fn a_group_trim_moves_every_out_point() {
        let mut timeline = Timeline::default();
        // Spaced out, so shortening one does not depend on the other.
        timeline.apply(Edit::Append(source(2))).unwrap();
        timeline
            .apply(Edit::Place {
                track: 0,
                source: source(2),
                position: 120,
                source_start: 0,
                length: 60,
            })
            .unwrap();
        let group = ids(&timeline);

        timeline
            .apply(Edit::Trim(vec![
                TrimTo {
                    clip: group[0],
                    edge: Edge::Out,
                    frame: 50,
                },
                TrimTo {
                    clip: group[1],
                    edge: Edge::Out,
                    frame: 170,
                },
            ]))
            .unwrap();

        assert_eq!(windows(&timeline, 0), vec![(0, 0, 50), (120, 0, 50)]);
    }

    #[test]
    fn a_group_trim_one_clip_cannot_do_changes_nothing() {
        let mut timeline = pair();
        let group = ids(&timeline);
        let before = windows(&timeline, 0);

        // The 60-frame source has nothing past its end, so the first clip
        // cannot grow — and the second must not grow on its own either.
        let refused = timeline.applied(Edit::Trim(vec![
            TrimTo {
                clip: group[0],
                edge: Edge::Out,
                frame: 70,
            },
            TrimTo {
                clip: group[1],
                edge: Edge::Out,
                frame: 100,
            },
        ]));

        assert!(refused.is_err());
        assert_eq!(windows(&timeline, 0), before);
    }

    #[test]
    fn a_trim_can_grow_into_its_neighbour() {
        let mut timeline = Timeline::default();
        // A 60-frame window onto a 120-frame source, with a clip hard against
        // its end: the frames to grow into exist, the room does not.
        timeline
            .apply(Edit::Place {
                track: 0,
                source: source(4),
                position: 0,
                source_start: 0,
                length: 60,
            })
            .unwrap();
        timeline.apply(Edit::Append(source(1))).unwrap();
        let (first, second) = (ids(&timeline)[0], ids(&timeline)[1]);

        timeline.apply(trimmed(first, Edge::Out, 75)).unwrap();

        // The first clip took fifteen frames from the second, which keeps the
        // rest — and its own window into the source moves with its in-point.
        assert_eq!(windows(&timeline, 0), vec![(0, 0, 75), (75, 15, 15)]);
        assert_eq!(ids(&timeline), vec![first, second]);
    }

    #[test]
    fn overwriting_trims_what_it_lands_on() {
        let mut timeline = pair();
        let second = ids(&timeline)[1];

        // The 30-frame clip lands halfway into the 60-frame one, which keeps
        // only the frames before it.
        timeline.apply(moved(second, 0, 30)).unwrap();

        assert_eq!(windows(&timeline, 0), vec![(0, 0, 30), (30, 0, 30)]);
    }

    #[test]
    fn overwriting_down_the_middle_leaves_a_piece_either_side() {
        let mut timeline = Timeline::default();
        timeline.apply(Edit::Append(source(4))).unwrap();
        timeline
            .apply(Edit::Place {
                track: 0,
                source: source(1),
                position: 200,
                source_start: 0,
                length: 30,
            })
            .unwrap();
        let dropped = ids(&timeline)[1];

        // 30 frames dropped at 45, inside a clip running 0..120.
        timeline.apply(moved(dropped, 0, 45)).unwrap();

        // The halves keep their own windows into the source: 0..45, then the
        // frames from 75 on.
        assert_eq!(
            windows(&timeline, 0),
            vec![(0, 0, 45), (45, 0, 30), (75, 75, 45)]
        );
    }

    #[test]
    fn overwriting_end_to_end_takes_the_clip_away() {
        let mut timeline = Timeline::default();
        timeline
            .apply(Edit::Place {
                track: 0,
                source: source(1),
                position: 0,
                source_start: 0,
                length: 30,
            })
            .unwrap();
        timeline.apply(Edit::Append(source(2))).unwrap();
        let long = ids(&timeline)[1];

        // The 60-frame clip lands over the 30-frame one, which has nothing
        // left to show.
        timeline.apply(moved(long, 0, 0)).unwrap();

        assert_eq!(windows(&timeline, 0), vec![(0, 0, 60)]);
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

        timeline
            .apply(Edit::Insert {
                source: source(1),
                frame: 0,
            })
            .unwrap();

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
        timeline
            .apply(Edit::Insert {
                source: source(1),
                frame: 55,
            })
            .unwrap();

        assert_eq!(
            windows(&timeline, 0),
            vec![(0, 0, 60), (60, 0, 30), (90, 0, 30)]
        );
    }

    #[test]
    fn a_move_onto_a_taken_landing_shortens_what_is_there() {
        let mut timeline = pair();
        let second = ids(&timeline)[1];

        // 30..60 is the first clip's, and the move takes it: the frames a
        // gesture asks for are the frames it gets.
        timeline.apply(moved(second, 0, 30)).unwrap();
        assert_eq!(windows(&timeline, 0), vec![(0, 0, 30), (30, 0, 30)]);

        // Past the end there is nothing to take.
        timeline.apply(moved(second, 0, 200)).unwrap();
        assert_eq!(windows(&timeline, 0), vec![(0, 0, 30), (200, 0, 30)]);
    }

    #[test]
    fn move_onto_the_track_past_the_last_starts_one() {
        let mut timeline = pair();
        let second = ids(&timeline)[1];

        timeline.apply(moved(second, 1, 0)).unwrap();

        assert_eq!(windows(&timeline, 0), vec![(0, 0, 60)]);
        assert_eq!(windows(&timeline, 1), vec![(0, 0, 30)]);
        // A track further down than that does not exist to move onto.
        assert!(timeline.apply(moved(second, 3, 0)).is_err());
    }

    #[test]
    fn trimming_the_in_point_moves_the_window_with_it() {
        let mut timeline = pair();
        let first = ids(&timeline)[0];

        timeline.apply(trimmed(first, Edge::In, 10)).unwrap();

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
        assert!(timeline.apply(trimmed(clip, Edge::In, 0)).is_ok());
        timeline.apply(trimmed(clip, Edge::In, 5)).unwrap();
        assert!(timeline.apply(trimmed(clip, Edge::Out, 40)).is_err());
        // And an out-point cannot cross its in-point.
        assert!(timeline.apply(trimmed(clip, Edge::Out, 5)).is_err());
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

        timeline.apply(cut(clip, 20)).unwrap();

        // The tail picks up in the source exactly where the head left off.
        assert_eq!(windows(&timeline, 0), vec![(0, 10, 20), (20, 30, 30)]);
        // A cut has to fall inside the clip to mean anything.
        assert!(timeline.apply(cut(clip, 0)).is_err());
    }

    #[test]
    fn delete_either_leaves_the_gap_or_closes_it() {
        let mut timeline = pair();
        let first = ids(&timeline)[0];

        timeline.apply(deleted(first, false)).unwrap();
        assert_eq!(windows(&timeline, 0), vec![(60, 0, 30)]);

        let mut timeline = pair();
        let first = ids(&timeline)[0];

        timeline.apply(deleted(first, true)).unwrap();
        assert_eq!(windows(&timeline, 0), vec![(0, 0, 30)]);

        // The clip is gone, so naming it again is not an edit.
        assert!(timeline.apply(deleted(first, false)).is_err());
    }

    #[test]
    fn next_content_steps_over_a_gap() {
        let mut timeline = pair();
        let first = ids(&timeline)[0];
        timeline.apply(deleted(first, false)).unwrap();

        // Frame 0..60 is now a hole: playback has to land on 60.
        assert_eq!(timeline.next_content(0), Some(60));
        assert_eq!(timeline.next_content(70), Some(70));
        // Past the end it wraps to the front, which is where the loop goes.
        assert_eq!(timeline.next_content(90), Some(60));
        assert_eq!(Timeline::default().next_content(0), None);
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
                .apply(Edit::Insert {
                    source: empty,
                    frame: 30
                })
                .is_err()
        );
        assert_eq!(windows(&timeline, 0), before);
    }
}
