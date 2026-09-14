use std::sync::{
    Arc,
    atomic::{AtomicU64, Ordering},
};
use std::time::Duration;

pub struct Clock {
    frames_played: Arc<AtomicU64>,
    sample_rate: usize,
    /// nanoseconds
    seek_base: AtomicU64,
    frames_at_seek: AtomicU64,
}

impl Clock {
    pub fn new(sample_rate: usize) -> Self {
        Clock {
            frames_played: Arc::new(AtomicU64::new(0)),
            sample_rate,
            seek_base: AtomicU64::new(0),
            frames_at_seek: AtomicU64::new(0),
        }
    }

    pub fn counter(&self) -> Arc<AtomicU64> {
        Arc::clone(&self.frames_played)
    }

    pub fn position(&self) -> Duration {
        let base = Duration::from_nanos(self.seek_base.load(Ordering::Relaxed));
        let at_seek = self.frames_at_seek.load(Ordering::Relaxed);
        let frames = self.frames_played.load(Ordering::Relaxed);

        let since_seek = frames.saturating_sub(at_seek);
        base + Duration::from_secs_f64(since_seek as f64 / self.sample_rate as f64)
    }

    pub fn seek_to(&self, position: Duration) {
        self.frames_at_seek.store(
            self.frames_played.load(Ordering::Relaxed),
            Ordering::Relaxed,
        );
        self.seek_base
            .store(position.as_nanos() as u64, Ordering::Relaxed);
    }
}
