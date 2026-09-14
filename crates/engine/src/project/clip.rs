use std::sync::Arc;

use crate::media::Source;

/// Names a clip for as long as it is in the document.
///
/// Minted by the timeline. Both positions and indices move under an edit — a
/// ripple shifts one, an insert reorders the other — so a gesture that spans
/// several events, or a selection that outlives one, needs a handle that holds
/// still. Ids are not written to the project file; a document that has just
/// been loaded numbers its clips from one again.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClipId(pub(super) u64);

impl ClipId {
    /// The bare number. A front end that cannot hold a `ClipId` — anything
    /// across a serialisation boundary — carries this and hands it back.
    pub fn raw(self) -> u64 {
        self.0
    }

    /// The inverse. Not checked against the document: an id for a clip that
    /// has since gone simply matches nothing.
    pub fn from_raw(id: u64) -> Self {
        ClipId(id)
    }
}

#[derive(Clone)]
pub struct Clip {
    pub id: ClipId,
    pub source: Arc<Source>,
    pub position: usize,
    pub source_start: usize,
    pub length: usize,
}

impl Clip {
    pub fn source_frame(&self, frame: usize) -> usize {
        self.source_start + (frame - self.position)
    }

    /// One past the last frame this clip covers.
    pub fn end(&self) -> usize {
        self.position + self.length
    }
}
