//! The handle a front end drives playback with, and what it can read back
//! without waiting: the playhead, how far behind the picture is, and whether
//! the timeline is rolling.

use cut_media::SeekMode;
use cut_timeline::Timeline;
use futures::channel::mpsc::Sender;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicUsize, Ordering};

#[derive(Clone)]
pub enum Command {
    TogglePlayback,
    Pause,
    /// The document changed — play this one from now on.
    Open(Arc<Timeline>),
    Step((i64, SeekMode)),
    Seek((usize, SeekMode)),
}

#[derive(Default)]
pub struct Status {
    playhead: AtomicUsize,
    lag_ms: AtomicI64,
    playing: Arc<AtomicBool>,
}

impl Status {
    pub fn new(playing: Arc<AtomicBool>) -> Self {
        Status {
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

/// Play, pause, seek — the tape-deck end of the machine.
#[derive(Clone)]
pub struct Transport {
    commands: Sender<Command>,
    status: Arc<Status>,
}

impl Transport {
    pub fn new(commands: Sender<Command>, status: Arc<Status>) -> Self {
        Transport { commands, status }
    }

    /// Dropped if the queue is full: a command that cannot be delivered now
    /// is one the next gesture supersedes anyway.
    pub fn send(&mut self, command: Command) {
        let _ = self.commands.try_send(command);
    }

    pub fn playhead(&self) -> usize {
        self.status.playhead.load(Ordering::Relaxed)
    }

    pub fn lag_ms(&self) -> i64 {
        self.status.lag_ms.load(Ordering::Relaxed)
    }

    pub fn is_playing(&self) -> bool {
        self.status.playing.load(Ordering::Relaxed)
    }
}
