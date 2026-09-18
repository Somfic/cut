use crate::state::State;
use cut_playback::{Command, SeekMode, Transport};
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
        let transport = self.session.transport.lock().unwrap();

        TransportDto {
            playhead: transport.as_ref().map_or(0, Transport::playhead),
            playing: transport.as_ref().is_some_and(Transport::is_playing),
            fps: self.session.fps.lock().unwrap().max(1.0),
        }
    }

    async fn seek(&self, frame: usize) {
        if let Some(transport) = self.session.transport.lock().unwrap().as_mut() {
            transport.send(Command::Seek((frame, SeekMode::Accurate)));
        }

        // The frame asked for: the request is queued, so reading the engine
        // back would answer with where the playhead was before it.
        announce(
            self,
            TransportDto {
                playhead: frame,
                ..dto(self)
            },
        );
    }

    async fn toggle(&self) {
        let playing = match self.session.transport.lock().unwrap().as_mut() {
            Some(transport) => {
                transport.send(Command::TogglePlayback);
                transport.is_playing()
            }
            None => {
                eprintln!("toggle: no transport yet");
                return;
            }
        };

        // Likewise: the engine flips its flag when it picks the request up.
        announce(
            self,
            TransportDto {
                playing: !playing,
                ..dto(self)
            },
        );
    }

    async fn pause(&self) {
        if let Some(transport) = self.session.transport.lock().unwrap().as_mut() {
            transport.send(Command::Pause);
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
    let transport = state.session.transport.lock().unwrap();

    TransportDto {
        playhead: transport.as_ref().map_or(0, Transport::playhead),
        playing: transport.as_ref().is_some_and(Transport::is_playing),
        fps: state.session.fps.lock().unwrap().max(24.0),
    }
}

/// Tell the front end what the engine currently says.
pub fn publish(state: &State) {
    announce(state, dto(state));
}

/// Tell it something the engine has been asked for but has not caught up with.
fn announce(state: &State, transport: TransportDto) {
    state.emit(|events| events.transport.emit_changed(&transport));
}

#[events(namespace = "transport")]
pub trait TransportEvents {
    fn changed(payload: TransportDto);
}
