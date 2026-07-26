use std::time::{Duration, Instant};

pub struct Clock {
    time_base: Duration,
    ticking_since: Option<Instant>,
}

impl Clock {
    pub fn new() -> Self {
        Clock {
            time_base: Duration::ZERO,
            ticking_since: None,
        }
    }

    pub fn position(&self) -> Duration {
        match self.ticking_since {
            Some(t) => self.time_base + t.elapsed(),
            None => self.time_base,
        }
    }

    pub fn is_ticking(&self) -> bool {
        self.ticking_since.is_some()
    }

    pub fn resume(&mut self) {
        if self.ticking_since.is_none() {
            self.ticking_since = Some(Instant::now());
        }
    }
    pub fn pause(&mut self) {
        if let Some(t) = self.ticking_since.take() {
            self.time_base += t.elapsed();
        }
    }
    pub fn seek_to(&mut self, position: Duration) {
        self.time_base = position;
        if self.ticking_since.is_some() {
            self.ticking_since = Some(Instant::now());
        }
    }
}
