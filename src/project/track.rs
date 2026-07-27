use crate::project::Clip;

pub struct Track {
    pub clips: Vec<Clip>,
}

impl Track {
    pub fn clip_at(&self, frame: usize) -> Option<&Clip> {
        self.clips
            .iter()
            .find(|c| frame >= c.position && frame < c.position + c.length)
    }
}
