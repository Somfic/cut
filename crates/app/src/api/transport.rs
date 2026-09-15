use std::sync::Arc;

use cut_engine::playback::{Controls, Request, SeekMode};
use draad::{api, ty};

use crate::state::Shared;

/// A correction, not a feed: the page runs its own clock between polls.
#[ty]
pub struct TransportDto {
    pub playhead: usize,
    pub playing: bool,
    pub fps: f64,
}

#[api(namespace = "transport")]
pub trait TransportApi {
    /// Where the playhead is, and whether it is moving.
    async fn state(&self) -> TransportDto;

    /// Move the playhead to `frame`.
    async fn seek(&self, frame: usize);

    /// Start or stop playback.
    async fn toggle(&self);
}

#[api]
impl TransportApi for Arc<Shared> {
    async fn state(&self) -> TransportDto {
        let controls = self.controls.lock().unwrap();

        TransportDto {
            playhead: controls.as_ref().map_or(0, Controls::playhead),
            playing: controls.as_ref().is_some_and(Controls::is_playing),
            // Never zero: the page divides by it, and the first poll can
            // land before any frame has.
            fps: self.fps.lock().unwrap().max(24.0),
        }
    }

    async fn seek(&self, frame: usize) {
        if let Some(controls) = self.controls.lock().unwrap().as_mut() {
            controls.send(Request::Seek((frame, SeekMode::Accurate)));
        }
    }

    async fn toggle(&self) {
        match self.controls.lock().unwrap().as_mut() {
            Some(controls) => controls.send(Request::TogglePlayback),
            None => eprintln!("toggle: no controls yet"),
        }
    }
}
