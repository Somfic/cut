//! The document: what the user is editing. Pure data — no live resources, no
//! decoders — so it can be shared with the UI, snapshotted for undo, and
//! eventually written to disk. The runtime that *plays* it lives in `playback`.

mod clip;
mod timeline;
mod track;

pub use clip::Clip;
pub use timeline::Timeline;
pub use track::Track;
