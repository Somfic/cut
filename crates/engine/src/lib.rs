mod demo;

pub mod media;
pub mod playback;
pub mod project;
pub mod stream;

pub fn init() -> anyhow::Result<()> {
    gstreamer::init()?;
    Ok(())
}
