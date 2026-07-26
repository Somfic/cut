use std::time::{Duration, Instant};

pub struct Clock {
    time_base: Duration,
    playing_since: Option<Instant>,
}

impl Clock {
    pub fn new() -> Self {
        Clock {
            time_base: Duration::ZERO,
            playing_since: None,
        }
    }

    pub fn position(&self) -> Duration {
        match self.playing_since {
            Some(t) => self.time_base + t.elapsed(),
            None => self.time_base,
        }
    }

    pub fn is_playing(&self) -> bool {
        self.playing_since.is_some()
    }

    pub fn resume(&mut self) {
        if self.playing_since.is_none() {
            self.playing_since = Some(Instant::now());
        }
    }
    pub fn pause(&mut self) {
        if let Some(t) = self.playing_since.take() {
            self.time_base += t.elapsed();
        }
    }
    pub fn seek_to(&mut self, position: Duration) {
        self.time_base = position;
        if self.playing_since.is_some() {
            self.playing_since = Some(Instant::now());
        }
    }
}
