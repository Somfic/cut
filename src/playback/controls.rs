use crate::playback::SeekMode;
use iced::futures::channel::mpsc::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

#[derive(Clone)]
pub enum Request {
    TogglePlayback,
    Step((i64, SeekMode)),
    Seek((usize, SeekMode)),
}

#[derive(Default)]
pub struct PlaybackState {
    playhead: AtomicUsize,
    lag_ms: AtomicI64,
    playing: Arc<AtomicBool>,
}

impl PlaybackState {
    pub fn new(playing: Arc<AtomicBool>) -> Self {
        PlaybackState {
            playhead: AtomicUsize::new(0),
            lag_ms: AtomicI64::new(0),
            playing,
        }
    }

    pub fn set_playhead(&self, frame: usize) {
        self.playhead.store(frame, Ordering::Relaxed);
    }

    pub fn set_lag_ms(&self, lag: i64) {
        self.lag_ms.store(lag, Ordering::Relaxed);
    }
}

#[derive(Clone)]
pub struct Controls {
    requests: Sender<Request>,
    state: Arc<PlaybackState>,
}

impl Controls {
    pub fn new(requests: Sender<Request>, state: Arc<PlaybackState>) -> Self {
        Controls { requests, state }
    }

    pub fn send(&mut self, request: Request) {
        let _ = self.requests.try_send(request);
    }

    pub fn playhead(&self) -> usize {
        self.state.playhead.load(Ordering::Relaxed)
    }

    pub fn lag_ms(&self) -> i64 {
        self.state.lag_ms.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.state.playing.load(Ordering::Relaxed)
    }
}
