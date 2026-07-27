use std::sync::Arc;

use crate::media::Source;

#[derive(Clone)]
pub struct Clip {
    pub source: Arc<Source>,
    pub position: usize,
    pub source_start: usize,
    pub length: usize,
}

impl Clip {
    pub fn source_frame(&self, frame: usize) -> usize {
        self.source_start + (frame - self.position)
    }
}
