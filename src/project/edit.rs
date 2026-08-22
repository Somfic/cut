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
