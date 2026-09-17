mod decoder;
mod frame;
mod probe;

pub use decoder::{AudioOut, Decoder, SeekMode, VideoOut};
pub use frame::{Frame, PixelLayout};
pub use probe::probe_source;

pub fn init() -> anyhow::Result<()> {
    gstreamer::init()?;
    Ok(())
}
