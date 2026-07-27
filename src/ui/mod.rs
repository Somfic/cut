//! The widgets. Each module is private and re-exports its entry point here, so
//! `ui`'s surface is the set of views the app can compose.

mod timeline;
mod video;

pub use timeline::TimelineView;
pub use video::VideoView;
