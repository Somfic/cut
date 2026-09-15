use crate::state::State;
use cut_engine::playback::{Controls, Request, SeekMode};
use draad::{api, events, ty};
use std::sync::Arc;

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
impl TransportApi for Arc<State> {
    async fn state(&self) -> TransportDto {
        let controls = self.session.controls.lock().unwrap();

        TransportDto {
            playhead: controls.as_ref().map_or(0, Controls::playhead),
            playing: controls.as_ref().is_some_and(Controls::is_playing),
            fps: self.session.fps.lock().unwrap().max(1.0),
        }
    }

    async fn seek(&self, frame: usize) {
        if let Some(controls) = self.session.controls.lock().unwrap().as_mut() {
            controls.send(Request::Seek((frame, SeekMode::Accurate)));
        }
        publish(self);
    }

    async fn toggle(&self) {
        match self.session.controls.lock().unwrap().as_mut() {
            Some(controls) => controls.send(Request::TogglePlayback),
            None => eprintln!("toggle: no controls yet"),
        }
        publish(self);
    }
}

pub fn dto(state: &State) -> TransportDto {
    let controls = state.session.controls.lock().unwrap();

    TransportDto {
        playhead: controls.as_ref().map_or(0, Controls::playhead),
        playing: controls.as_ref().is_some_and(Controls::is_playing),
        fps: state.session.fps.lock().unwrap().max(24.0),
    }
}

pub fn publish(state: &State) {
    if let Some(events) = state.events.get() {
        events.transport.emit_changed(&dto(state));
    }
}

#[events(namespace = "transport")]
pub trait TransportEvents {
    fn changed(payload: TransportDto);
}
