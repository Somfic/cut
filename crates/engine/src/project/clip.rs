use std::sync::Arc;

use crate::media::Source;

/// Names a clip for as long as it is in the document: positions and indices
/// both move under an edit, so a selection needs a handle that holds still.
/// Not written to the project file.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub struct ClipId(pub(super) u64);

impl ClipId {
    /// What a front end carries across the serialisation boundary.
    pub fn raw(self) -> u64 {
        self.0
    }

    /// Unchecked: an id for a clip that has gone simply matches nothing.
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
