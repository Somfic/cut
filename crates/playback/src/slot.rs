//! The handoff from the transport to whoever draws.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};

use cut_media::Frame;

/// Room for exactly one frame. A drawer that falls behind never queues a
/// backlog it would have to catch up on: the newest frame displaces the one
/// waiting, and the displaced one is counted as dropped.
#[derive(Default)]
pub struct FrameSlot {
    frame: Mutex<Option<Arc<Frame>>>,
    arrived: Condvar,
    uploaded: AtomicU64,
}

impl FrameSlot {
    /// True if this displaced a frame nobody took.
    pub fn put(&self, frame: Arc<Frame>) -> bool {
        let displaced = self.frame.lock().unwrap().replace(frame).is_some();
        self.arrived.notify_one();
        displaced
    }

    /// Blocks until there is one.
    pub fn take(&self) -> Arc<Frame> {
        let mut slot = self.frame.lock().unwrap();
        loop {
            if let Some(frame) = slot.take() {
                return frame;
            }
            slot = self.arrived.wait(slot).unwrap();
        }
    }

    pub fn uploaded(&self) {
        self.uploaded.fetch_add(1, Ordering::Relaxed);
    }

    /// Bumped on every upload, so a drawer can tell a new picture from a
    /// repeat of the one already on screen.
    pub fn generation(&self) -> u64 {
        self.uploaded.load(Ordering::Relaxed)
    }
}
