//! Playing a timeline: the crate that turns a position in the document into
//! a picture on its way to the screen.
//!
//! The whole path, in order:
//!
//! ```text
//! transport::cut()        a timeline frame to move to
//!   Engine::cut_to()      fans out over the tracks
//!     Player::cut_to()    picks this track's clip, maps to a source frame
//!       Decoder::seek()   `cut-media` seeks the gstreamer pipeline
//!         ...             decoding happens on gstreamer's threads
//!       VideoSink         a decoded Frame arrives back
//!   transport loop        schedules it against the clock
//!   Event::Frame          handed to whoever is driving the transport
//!     FrameSlot::put()    the handoff to the GPU thread
//! ```
//!
//! Audio leaves by a different door: decoders write straight into
//! `cut-audio`'s sink, and the clock counts what the device consumes. Video
//! is scheduled against that clock, which is what keeps the two together.

mod controls;
mod demo;
mod engine;
mod player;
mod sinks;
mod slot;
mod stream;
mod transport;

// What a front end needs: a stream of events, a handle to drive playback, and
// somewhere to put the frames that come out.
pub use controls::{Controls, Request};
pub use cut_media::SeekMode;
pub use slot::FrameSlot;
pub use transport::{Event, transport};

// The machinery behind them.
pub(crate) use engine::Engine;
pub(crate) use player::Player;
pub(crate) use sinks::{Sinks, VideoStream};
