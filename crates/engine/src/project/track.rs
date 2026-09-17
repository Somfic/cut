use crate::project::Clip;

#[derive(Clone)]
pub struct Track {
    pub clips: Vec<Clip>,
    /// Off stops the picture, not the document: the clips stay and stay
    /// editable.
    pub enabled: bool,
}

impl Default for Track {
    fn default() -> Self {
        Track {
            clips: Vec::new(),
            enabled: true,
        }
    }
}

impl Track {
    pub fn clip_at(&self, frame: usize) -> Option<&Clip> {
        self.clips
            .iter()
            .find(|c| frame >= c.position && frame < c.position + c.length)
    }
}
