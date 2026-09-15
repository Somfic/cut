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

    /// Stop playback, if it is running.
    async fn pause(&self);
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

        // The frame asked for, not the one playback is still showing: the
        // request is queued, so reading the engine back would answer with
        // where the playhead was before the seek.
        announce(
            self,
            TransportDto {
                playhead: frame,
                ..dto(self)
            },
        );
    }

    async fn toggle(&self) {
        let playing = match self.session.controls.lock().unwrap().as_mut() {
            Some(controls) => {
                controls.send(Request::TogglePlayback);
                controls.is_playing()
            }
            None => {
                eprintln!("toggle: no controls yet");
                return;
            }
        };

        // Likewise: the engine flips its flag when it picks the request up,
        // and the front end's clock keeps running on a stale `playing`.
        announce(
            self,
            TransportDto {
                playing: !playing,
                ..dto(self)
            },
        );
    }

    async fn pause(&self) {
        if let Some(controls) = self.session.controls.lock().unwrap().as_mut() {
            controls.send(Request::Pause);
        }

        announce(
            self,
            TransportDto {
                playing: false,
                ..dto(self)
            },
        );
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

/// Tell the front end what the engine currently says.
pub fn publish(state: &State) {
    announce(state, dto(state));
}

/// Tell it something the engine has been asked for but has not caught up with.
fn announce(state: &State, transport: TransportDto) {
    if let Some(events) = state.events.get() {
        events.transport.emit_changed(&transport);
    }
}

#[events(namespace = "transport")]
pub trait TransportEvents {
    fn changed(payload: TransportDto);
}
