use std::sync::Arc;

use crate::ClipId;
use crate::Source;

/// A change to the document, and so the list of what editing can do. Every
/// edit takes a set of clips, even a set of one: a gesture is one step on the
/// undo stack. Moving and trimming overwrite what they land on.
#[derive(Clone)]
pub enum Edit {
    /// An exact window of a source, which is what loading and pasting need.
    Place {
        track: usize,
        source: Arc<Source>,
        position: usize,
        source_start: usize,
        length: usize,
    },
    /// Put the whole of a source after everything already there.
    Append(Arc<Source>),
    /// Ripple the whole of a source in at the clip edge nearest `frame`.
    Insert { source: Arc<Source>, frame: usize },
    /// Take clips somewhere else. A `track` one past the last starts one.
    Move(Vec<Placement>),
    /// Move one end of each clip, leaving the other where it is.
    Trim(Vec<TrimTo>),
    /// Trim one clip's edge to `frame` and close the gap, on every track. The
    /// one edit taking a single clip: two edit points would give two answers
    /// for how far the rest should move.
    Ripple {
        clip: ClipId,
        edge: Edge,
        frame: usize,
    },
    /// Cut clips in two at `frame`, leaving alone any it is not inside.
    Split { clips: Vec<ClipId>, frame: usize },
    /// Remove clips, either leaving the gaps they held or closing them.
    Delete { clips: Vec<ClipId>, ripple: bool },
    /// Take a track in or out of playback, leaving its clips alone.
    Enable { track: usize, enabled: bool },
}

/// Where a clip is being put.
#[derive(Clone, Copy)]
pub struct Placement {
    pub clip: ClipId,
    pub track: usize,
    pub position: usize,
}

/// Which end of a clip is going where.
#[derive(Clone, Copy)]
pub struct TrimTo {
    pub clip: ClipId,
    pub edge: Edge,
    pub frame: usize,
}

/// Which end of a clip a trim moves.
#[derive(Clone, Copy)]
pub enum Edge {
    In,
    Out,
}
