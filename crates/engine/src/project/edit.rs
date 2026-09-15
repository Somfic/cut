use std::sync::Arc;

use crate::media::Source;
use crate::project::ClipId;

/// A change to the document, and so the list of what editing can do.
///
/// Every edit names a set of clips, even a set of one: a gesture is one thing
/// the user did, and three edits would refuse arrangements that are legal as
/// a group and leave three steps on the undo stack.
///
/// Moving and trimming overwrite what they land on — a trim only narrows a
/// window into a source, so undo gives the frames back. Only `Place` refuses,
/// because a file saying two clips share frames is a file that is wrong.
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
    /// Take clips somewhere else. A `track` may be one past the last, which
    /// starts a new one.
    Move(Vec<Placement>),
    /// Move one end of each clip, leaving the other where it is.
    Trim(Vec<TrimTo>),
    /// Trim one clip's edge to `frame` and close the gap it leaves: what
    /// follows moves back by what the trim took, on every track.
    ///
    /// The one edit that names a single clip rather than a set. A ripple is
    /// defined at one edit point — two of them would have two answers for how
    /// far everything after should move.
    Ripple {
        clip: ClipId,
        edge: Edge,
        frame: usize,
    },
    /// Cut clips in two at `frame`. Clips the frame is not inside are left
    /// alone, so one cut can be aimed at a whole selection.
    Split { clips: Vec<ClipId>, frame: usize },
    /// Remove clips, either leaving the gaps they held or closing them.
    Delete { clips: Vec<ClipId>, ripple: bool },
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
