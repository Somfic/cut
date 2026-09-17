//! The document: pure data, no decoders, so it can be shared with the UI,
//! snapshotted for undo and written to disk. Every change is an `Edit`.

mod clip;
mod edit;
pub mod file;
mod history;
mod timeline;
mod track;

pub use clip::{Clip, ClipId};
pub use edit::{Edge, Edit, Placement, TrimTo};
pub use history::History;
pub use timeline::Timeline;
pub use track::Track;
