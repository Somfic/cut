//! The document: what the user is editing. Pure data — no live resources, no
//! decoders — so it can be shared with the UI, snapshotted for undo, and
//! written to disk. Every change to it is an `Edit` carried out by
//! `Timeline::apply`. The runtime that *plays* it lives in `playback`, and the
//! on-disk shape in `file`.

mod clip;
mod edit;
pub mod file;
mod history;
mod timeline;
mod track;

pub use clip::{Clip, ClipId};
pub use edit::{Edge, Edit};
pub use history::History;
pub use timeline::Timeline;
pub use track::Track;
