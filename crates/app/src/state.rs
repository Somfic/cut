use std::sync::atomic::{AtomicBool, AtomicU64};
use std::sync::{Arc, Condvar, Mutex};

use cut_engine::media::Frame;
use cut_engine::playback::Controls;
use cut_engine::project::Timeline;

/// Where the video goes, in physical pixels. Owned by the page, which
/// reports the rect of the element standing in for the preview.
#[derive(Default, Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Default)]
pub struct Shared {
    /// Latest-wins: a preview that falls behind should skip, not queue up.
    pub frame: Mutex<Option<Arc<Frame>>>,
    /// Signalled on a new frame, so the uploader sleeps rather than polls.
    pub arrived: Condvar,
    pub timeline: Mutex<Option<Arc<Timeline>>>,
    pub rect: Mutex<Option<Rect>>,
    pub chrome: Mutex<Option<(f64, f64, f64)>>,
    /// The page's viewport. Only a check: it should equal the surface size,
    /// and a lasting disagreement means the two have stopped sharing a space.
    pub page: Mutex<Option<(f32, f32)>>,
    pub controls: Mutex<Option<Controls>>,
    /// Bumped per upload, so the presenter can skip a turn with nothing new.
    pub uploaded: AtomicU64,
    /// Totals, not rates: a smoothed rate is dominated by bursts.
    pub frames: AtomicU64,
    pub presents: AtomicU64,
    /// Replaced before the uploader ever took them.
    pub dropped: AtomicU64,
    pub fps: Mutex<f64>,
    pub running: AtomicBool,
}

