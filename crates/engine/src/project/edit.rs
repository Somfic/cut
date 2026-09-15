use std::sync::Arc;

use crate::media::Source;
use crate::project::ClipId;

/// A change to the document.
///
/// `Timeline::apply` is the only thing that carries one out, so this is also
/// the list of what editing can do — and the one place to add to when it can
/// do more.
#[derive(Clone)]
pub enum Edit {
    /// Put an exact window of a source on a track. What loading a project and
    /// pasting both come down to.
    Place {
        track: usize,
        source: Arc<Source>,
        position: usize,
        source_start: usize,
        length: usize,
    },
    /// Put the whole of a source after everything already there.
    Append(Arc<Source>),
    /// Ripple the whole of a source in at `frame`, snapped to the nearest clip
    /// edge, pushing what follows later.
    Insert { source: Arc<Source>, frame: usize },
    /// Take a clip somewhere else. `track` may be one past the last, which
    /// starts a new one.
    Move {
        clip: ClipId,
        track: usize,
        position: usize,
    },
    /// Move one end of a clip to `frame`, leaving the other where it is.
    Trim {
        clip: ClipId,
        edge: Edge,
        frame: usize,
    },
    /// Move several clips by the same offset, as one edit.
    ///
    /// Not a `Move` each: a group that shuffles within its own span would be
    /// refused by the room it is itself about to vacate, and a half-applied
    /// group is a document nobody asked for. Every clip here is lifted out
    /// before any is put down, and the whole thing lands or none of it does.
    Nudge {
        clips: Vec<ClipId>,
        frames: isize,
        /// How many lanes down, negative for up.
        tracks: isize,
        /// What to do about whatever is already where the group lands:
        /// shorten it to make room, or refuse the move. Overwriting takes
        /// no frames away that a trim could not give back, except from a
        /// clip covered end to end, which has nothing left to show.
        overwrite: bool,
    },
    /// Move the same end of several clips by the same offset, as one edit.
    /// `Trim` for a group, with the same all-or-nothing rule as `Nudge`.
    Stretch {
        clips: Vec<ClipId>,
        edge: Edge,
        frames: isize,
    },
    /// Cut a clip in two at `frame`.
    Split { clip: ClipId, frame: usize },
    /// Remove a clip, either leaving the gap it held or closing it.
    Delete { clip: ClipId, ripple: bool },
}

/// Which end of a clip a trim moves.
#[derive(Clone, Copy)]
pub enum Edge {
    In,
    Out,
}
