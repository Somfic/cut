use std::path::PathBuf;
use std::time::Duration;

use crate::Rational;

/// A piece of media as the document knows it: where it is and what it
/// measures. Opening it is [`cut_media`](../cut_media/index.html)'s job — a
/// clip holds this, and nothing here touches a decoder.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Source {
    pub path: PathBuf,
    pub duration: Duration,
    /// The rate the file declares, not a divided-out float.
    pub frame_rate: Rational,
}

impl Source {
    pub fn new(path: impl Into<PathBuf>, duration: Duration, frame_rate: Rational) -> Self {
        Source {
            path: path.into(),
            duration,
            frame_rate,
        }
    }

    pub fn fps(&self) -> f64 {
        self.frame_rate.as_f64()
    }

    pub fn frame_count(&self) -> usize {
        (self.duration.as_secs_f64() * self.fps()).round() as usize
    }
}
