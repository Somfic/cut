use std::sync::Arc;

use crate::video::Video;

mod widget;

pub use widget::timeline;

#[derive(Default)]
pub struct Timeline {
    pub tracks: Vec<Track>,
}

pub struct Track {
    pub clips: Vec<Clip>,
}

pub struct Clip {
    pub source: Arc<Video>,
    pub position: usize,
    pub source_start: usize,
    pub length: usize,
}

impl Clip {
    pub fn source_frame(&self, frame: usize) -> usize {
        self.source_start + (frame - self.position)
    }
}

impl Timeline {
    pub fn active_clips(&self, frame: usize) -> Vec<&Clip> {
        self.tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| c.position <= frame && frame < (c.position + c.length))
            .collect()
    }
}
