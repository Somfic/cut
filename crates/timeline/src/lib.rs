//! The document: pure data, no decoders, so it can be shared with the UI,
//! snapshotted for undo and written to disk. Every change is an [`Edit`].
//!
//! Nothing here opens a file. A [`Source`] is what the document knows about
//! a piece of media — path, duration, rate — and `cut-media` is what turns a
//! path into one.

mod clip;
mod edit;
pub mod file;
mod history;
mod rational;
mod source;
mod timeline;
mod track;

pub use clip::{Clip, ClipId};
pub use edit::{Edge, Edit, Placement, TrimTo};
pub use history::History;
pub use rational::Rational;
pub use source::Source;
pub use timeline::{DEFAULT_TIMEBASE, Timeline};
pub use track::Track;
