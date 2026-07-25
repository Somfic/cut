use anyhow::{Context, anyhow};
use gstreamer::Sample;
use gstreamer_video as gst_video;
use gstreamer_video::prelude::*;
use std::fmt::Debug;
use std::time::Duration;

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub time: Duration,
    pub data: Vec<u8>,
}

impl TryFrom<Sample> for Frame {
    type Error = anyhow::Error;

    fn try_from(sample: Sample) -> Result<Frame, Self::Error> {
        let caps = sample.caps().context("sample had no caps")?;
        let info = gst_video::VideoInfo::from_caps(caps).context("caps were not video/x-raw")?;
        let width = info.width();
        let height = info.height();

        let buffer = sample.buffer().context("sample had no buffer")?;
        let frame = gst_video::VideoFrameRef::from_buffer_ref_readable(buffer, &info)
            .map_err(|_| anyhow!("failed to map buffer as readable video frame"))?;

        let time = buffer.pts().map(Duration::from).unwrap_or_default();

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

        Ok(Frame {
            width,
            height,
            time,
            data,
        })
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
