mod clock;
mod controls;
mod decoder;
mod engine;
mod player;
mod sinks;
mod transport;

// What a front end needs: a stream of events, and a handle to drive playback.
pub use controls::{Controls, Request};
pub use decoder::SeekMode;
pub use transport::{Event, transport};

// The machinery behind them.
pub(crate) use clock::Clock;
pub(crate) use controls::PlaybackState;
pub(crate) use decoder::Decoder;
pub(crate) use engine::Engine;
pub(crate) use player::Player;
pub(crate) use sinks::{AudioSink, Sinks, VideoSink, VideoStream};
