use anyhow::{Context, anyhow};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_app::AppSink;
use std::path::PathBuf;

mod frame;
mod renderer;
mod view;

pub use frame::Frame;
pub use renderer::FrameRenderer;
pub use view::VideoView;

pub struct Video {
    pub path: PathBuf,
    pub pipeline: gst::Pipeline,
    pub sink: AppSink,
}

impl Video {
    pub fn new(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))?;

        let pipeline = gst::parse::launch(
            "filesrc name=src ! decodebin ! videoconvert ! \
            video/x-raw,format=RGBA ! appsink name=sink",
        )
        .context("failed to parse pipeline")?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow!("parsed element was not a Pipeline"))?;

        pipeline
            .by_name("src")
            .context("no element named 'src'")?
            .set_property("location", path_str);

        let sink = pipeline
            .by_name("sink")
            .context("no element named 'sink'")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("'sink' was not an AppSink"))?;

        Ok(Video {
            path,
            pipeline,
            sink,
        })
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
