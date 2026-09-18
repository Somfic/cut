//! Playing a timeline: the crate that turns a position in the document into
//! a picture on its way to the screen.
//!
//! The whole path, in order:
//!
//! ```text
//! run::cut()                 a timeline frame to move to
//!   Player::cut_to()         fans out over the tracks
//!     TrackPlayer::cut_to()  picks this track's clip, maps to a source frame
//!       Decoder::seek()      `cut-media` seeks the gstreamer pipeline
//!         ...                decoding happens on gstreamer's threads
//!       VideoSink            a decoded Frame arrives back
//!   run loop                 schedules it against the clock
//!   Event::Frame             handed to whoever is driving playback
//!     FrameSlot::put()       the handoff to the GPU thread
//! ```
//!
//! Three layers, each named for what it plays: [`Player`] plays a timeline,
//! a `TrackPlayer` plays a track, and a `Decoder` plays a file. [`Transport`]
//! is the handle a front end drives the whole thing with.
//!
//! Audio leaves by a different door: decoders write straight into
//! `cut-audio`'s sink, and the clock counts what the device consumes. Video
//! is scheduled against that clock, which is what keeps the two together.

mod player;
mod run;
mod sinks;
mod slot;
mod stream;
mod track_player;
mod transport;

// What a front end needs: a stream of events, a handle to drive playback, and
// somewhere to put the frames that come out.
pub use cut_media::SeekMode;
pub use run::{Event, run};
pub use slot::FrameSlot;
pub use transport::{Command, Transport};

// The machinery behind them.
pub(crate) use player::Player;
pub(crate) use sinks::{Outputs, VideoStream};
pub(crate) use track_player::TrackPlayer;
