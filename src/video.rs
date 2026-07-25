use anyhow::{Context, anyhow};
use gstreamer as gst;
use gstreamer::Pipeline;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_app::AppSink;
use std::path::PathBuf;

pub struct Video {
    pub path: PathBuf,
    pub pipeline: Pipeline,
    pub sink: AppSink,
}

impl Video {
    pub fn new(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let path_str = path.display().to_string();

        let pipeline = format!(
            "filesrc location={path_str} ! decodebin ! videoconvert ! \
            video/x-raw,format=RGBA ! appsink name=sink"
        );

        let pipeline = gst::parse::launch(&pipeline)
            .context("failed to parse pipeline")?
            .downcast::<gst::Pipeline>()
            .map_err(|_| anyhow!("parsed element was not a Pipeline"))?;

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
