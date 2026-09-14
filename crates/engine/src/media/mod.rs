mod frame;
mod source;

pub use frame::{Frame, PixelLayout};
pub use source::Source;

/// Re-exported because it is part of this crate's public surface: both
/// [`Source::frame_rate`] and [`Frame::fps`] are rationals, and a front end
/// that names one should not need its own gstreamer dependency to do it.
pub use gstreamer::Fraction;
