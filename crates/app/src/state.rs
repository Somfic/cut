use std::path::PathBuf;
use std::sync::atomic::AtomicU64;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Instant;

use cut_playback::{FrameSlot, Transport};
use cut_timeline::{History, Timeline};

use crate::api::view::ViewDto;
use crate::api::window::WindowDto;

#[derive(Default)]
pub struct State {
    pub slot: FrameSlot,
    pub surface: Surface,
    pub session: Session,
    pub counters: Counters,
    pub events: OnceLock<crate::generated::Events>,
    pub handle: OnceLock<tauri::AppHandle>,
    pub window: Mutex<WindowDto>,
}

impl State {
    pub fn emit(&self, event: impl FnOnce(&crate::generated::Events)) {
        if let Some(events) = self.events.get() {
            event(events);
        }
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
    pub history: Mutex<History<HistoryEntry>>,
    pub view: Mutex<ViewDto>,
    pub transport: Mutex<Option<Transport>>,
    pub fps: Mutex<f64>,
    pub project: Mutex<Project>,
}

#[derive(Default)]
pub struct Project {
    pub path: Option<PathBuf>,
    pub saved: bool,
    pub last_edited: Option<Instant>,
}

#[derive(Clone)]
pub struct HistoryEntry {
    pub timeline: Arc<Timeline>,
    pub view: ViewDto,
}

#[derive(Default)]
pub struct Counters {
    pub frames: AtomicU64,
    pub presents: AtomicU64,
    pub dropped: AtomicU64,
}
