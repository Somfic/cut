use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex, OnceLock};

use cut_engine::media::Frame;
use cut_engine::playback::Controls;
use cut_engine::project::{History, Timeline};

use crate::api::view::ViewDto;
use crate::api::window::WindowDto;

#[derive(Default)]
pub struct State {
    pub slot: FrameSlot,
    pub surface: Surface,
    pub session: Session,
    pub counters: Counters,
    pub events: OnceLock<crate::generated::Events>,
    /// What the window is doing, so a change can be told from a repeat.
    pub window: Mutex<WindowDto>,
}

#[derive(Default)]
pub struct FrameSlot {
    frame: Mutex<Option<Arc<Frame>>>,
    arrived: Condvar,
    uploaded: AtomicU64,
}

impl FrameSlot {
    pub fn put(&self, frame: Arc<Frame>) -> bool {
        let displaced = self.frame.lock().unwrap().replace(frame).is_some();
        self.arrived.notify_one();
        displaced
    }

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

    pub fn generation(&self) -> u64 {
        self.uploaded.load(Ordering::Relaxed)
    }
}

#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Default)]
pub struct Surface {
    pub rect: Mutex<Option<Rect>>,
    pub chrome: Mutex<Option<(f64, f64, f64)>>,
    pub page: Mutex<Option<(f32, f32)>>,
}

#[derive(Default)]
pub struct Session {
    pub timeline: Mutex<Option<Arc<Timeline>>>,
    /// Versions behind the current one, and ahead of it after an undo. Lives
    /// here rather than in the engine because it is per-session state —
    /// nothing about it is written to the project file.
    pub history: Mutex<History<Version>>,
    /// Where the timeline was last left. Updated as the user pans and zooms,
    /// and copied into a version whenever one is recorded.
    pub view: Mutex<ViewDto>,
    pub controls: Mutex<Option<Controls>>,
    pub fps: Mutex<f64>,
}

/// A step of the undo stack: the document, and where it was being looked at.
#[derive(Clone)]
pub struct Version {
    pub timeline: Arc<Timeline>,
    pub view: ViewDto,
}

#[derive(Default)]
pub struct Counters {
    pub frames: AtomicU64,
    pub presents: AtomicU64,
    pub dropped: AtomicU64,
}
