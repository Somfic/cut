use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

pub struct Clock {
    frames_played: Arc<AtomicU64>,
    sample_rate: usize,
    seek_base: Duration,
    frames_at_seek: u64,
}

impl Clock {
    pub fn new(sample_rate: usize) -> Self {
        Clock {
            frames_played: Arc::new(AtomicU64::new(0)),
            sample_rate,
            seek_base: Duration::ZERO,
            frames_at_seek: 0,
        }
    }

    pub fn counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.frames_played)
    }

    pub fn position(&self) -> Duration {
        let frames = self.frames_played.load(Ordering::Relaxed);
        let since_seek = frames.saturating_sub(self.frames_at_seek);
        self.seek_base + Duration::from_secs_f64(since_seek as f64 / self.sample_rate as f64)
    }

    pub fn seek_to(&mut self, position: Duration) {
        self.seek_base = position;
        self.frames_at_seek = self.frames_played.load(Ordering::Relaxed);
    }
}
