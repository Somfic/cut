use anyhow::{Context, anyhow};
use gstreamer as gst;
use gstreamer::prelude::*;
use gstreamer_app as gst_app;
use gstreamer_video as gst_video;
use gstreamer_video::prelude::*;
use iced::widget::image;
use std::fmt::Debug;
use std::path::PathBuf;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

impl Into<image::Handle> for Frame {
    fn into(self) -> image::Handle {
        image::Handle::from_rgba(self.width, self.height, self.data)
    }
}

impl Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .finish()
    }
}

pub fn decode_frame(path: impl Into<PathBuf>) -> anyhow::Result<Frame> {
    let path = path.into().display().to_string();

    // build pipeline
    let pipeline = format!(
        "filesrc location={path} ! decodebin ! videoconvert ! \
         video/x-raw,format=RGBA ! appsink name=sink"
    );
    let pipeline = gst::parse::launch(&pipeline)
        .context("failed to parse pipeline")?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow!("parsed element was not a Pipeline"))?;

    // target sink
    let appsink = pipeline
        .by_name("sink")
        .context("no element named 'sink'")?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| anyhow!("'sink' was not an AppSink"))?;

    // trigger preroll
    pipeline
        .set_state(gst::State::Paused)
        .context("failed to set pipeline to Paused")?;
    let sample = appsink
        .pull_preroll()
        .context("pull_preroll failed (bad file? missing decoder plugin?)")?;

    // get frame dimensions
    let caps = sample.caps().context("sample had no caps")?;
    let info = gst_video::VideoInfo::from_caps(caps).context("caps were not video/x-raw")?;
    let width = info.width();
    let height = info.height();

    // get frame
    let buffer = sample.buffer().context("sample had no buffer")?;
    let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info)
        .map_err(|_| anyhow!("failed to map buffer as readable video frame"))?;

    // copy over
    let src = frame.plane_data(0).map_err(|_| anyhow!("no plane 0"))?;
    let src_stride = frame.plane_stride()[0] as usize;
    let dst_stride = width as usize * 4;
    let mut data = vec![0u8; dst_stride * height as usize];
    for row in 0..height as usize {
        let s = row * src_stride;
        let d = row * dst_stride;
        data[d..d + dst_stride].copy_from_slice(&src[s..s + dst_stride]);
    }

    let frame = Frame {
        width,
        height,
        data,
    };

    // close pipeline
    pipeline
        .set_state(gst::State::Null)
        .context("failed to set pipeline to Null")?;

    Ok(frame)
}
