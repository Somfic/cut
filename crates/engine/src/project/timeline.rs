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
    /// give up part-way. Callers hold the document behind an `Arc` and need an
    /// owned one to publish anyway.
    pub fn applied(&self, edit: Edit) -> anyhow::Result<Timeline> {
        let mut next = self.clone();
        next.apply(edit)?;

        Ok(next)
    }

    /// Change the document in place, upholding the invariants a project file
    /// is checked against when it loads.
    ///
    /// May stop half-way: a set of clips is dealt with one at a time, and the
    /// first refusal abandons the rest. Anything that cannot live with that
    /// wants `applied`.
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

        // Lifted before the room is judged, or a clip growing by a frame is
        // found to be in its own way.
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

    /// The overwrite rule, spelled out once: a clip covered at one end is
    /// trimmed back, one covered down the middle is split around the span, and
    /// one covered end to end is removed.
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

    /// Every clip comes out before any goes back in, which is the whole
    /// reason a move takes a set: a group shuffling within its own span would
    /// be refused by the room it is itself about to vacate.
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

            // The movers are lifted out, so what is in the way is only ever
            // a clip staying put.
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

#[path = "timeline/tests.rs"]
#[cfg(test)]
mod tests;
