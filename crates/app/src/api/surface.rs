use std::sync::Arc;
use std::sync::atomic::Ordering;

use draad::{api, ty};

use crate::state::{Rect, Shared};

/// Totals, not rates: a smoothed rate is dominated by bursts.
#[ty]
pub struct StatsDto {
    pub frames: u64,
    pub presents: u64,
    pub dropped: u64,
}

#[api(namespace = "surface")]
pub trait SurfaceApi {
    /// Where the page has left room for the video, in physical pixels,
    /// along with its own viewport so the two coordinate spaces can be
    /// checked against each other.
    async fn set_rect(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        page_width: f32,
        page_height: f32,
    );

    /// What to clear the surface to, so the area around the letterboxed
    /// video matches the chrome.
    async fn set_chrome(&self, r: f64, g: f64, b: f64);

    /// Frame counters, for the readout.
    async fn stats(&self) -> StatsDto;
}

#[api]
impl SurfaceApi for Arc<Shared> {
    async fn set_rect(
        &self,
        x: f32,
        y: f32,
        width: f32,
        height: f32,
        page_width: f32,
        page_height: f32,
    ) {
        *self.rect.lock().unwrap() = Some(Rect { x, y, width, height });
        *self.page.lock().unwrap() = Some((page_width, page_height));
    }

    async fn set_chrome(&self, r: f64, g: f64, b: f64) {
        *self.chrome.lock().unwrap() = Some((r, g, b));
    }

    async fn stats(&self) -> StatsDto {
        StatsDto {
            frames: self.frames.load(Ordering::Relaxed),
            presents: self.presents.load(Ordering::Relaxed),
            dropped: self.dropped.load(Ordering::Relaxed),
        }
    }
}
