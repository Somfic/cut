use anyhow::{Context, anyhow};
use gstreamer::{Fraction, Sample};
use gstreamer_video as gst_video;
use gstreamer_video::VideoFormat;
use gstreamer_video::prelude::*;
use std::fmt::Debug;
use std::time::Duration;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum PixelLayout {
    /// 8-bit: one byte per sample
    Nv12,
    /// 10-bit in the high bits of a 16-bit little-endian
    P010,
}

/// A decoded picture, still in the buffer gstreamer decoded it into.
///
/// Not copied out and not repacked: a texture upload takes a row stride, so
/// the decoder's padded rows can go to the GPU where they lie.
pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub fps: Fraction,
    pub time: Duration,
    pub layout: PixelLayout,
    /// The mapped buffer. Holding it is what keeps the plane slices valid.
    mapped: gst_video::VideoFrame<gst_video::video_frame::Readable>,
}

impl Frame {
    /// The luma plane. Spans padded rows, so read it with `y_stride`.
    pub fn y(&self) -> &[u8] {
        self.plane(0)
    }

    pub fn y_stride(&self) -> u32 {
        self.stride(0)
    }

    /// The interleaved chroma plane (Cb,Cr pairs), and its row stride.
    pub fn uv(&self) -> &[u8] {
        self.plane(1)
    }

    pub fn uv_stride(&self) -> u32 {
        self.stride(1)
    }

    fn plane(&self, index: u32) -> &[u8] {
        self.mapped
            .plane_data(index)
            .expect("plane index checked when the frame was built")
    }

    fn stride(&self, index: usize) -> u32 {
        self.mapped.plane_stride()[index] as u32
    }
}

impl TryFrom<Sample> for Frame {
    type Error = anyhow::Error;

    fn try_from(sample: Sample) -> Result<Frame, Self::Error> {
        let caps = sample.caps().context("sample had no caps")?;
        let info = gst_video::VideoInfo::from_caps(caps).context("caps were not video/x-raw")?;

        let layout = match info.format() {
            VideoFormat::Nv12 => PixelLayout::Nv12,
            VideoFormat::P01010le => PixelLayout::P010,
            other => return Err(anyhow!("unsupported pixel format: {other:?}")),
        };

        // Owned: the mapping outlives this call, so the planes can be
        // uploaded later without a copy.
        let buffer = sample.buffer_owned().context("sample had no buffer")?;
        let time = buffer.pts().map(Duration::from).unwrap_or_default();

        let mapped = gst_video::VideoFrame::from_buffer_readable(buffer, &info)
            .map_err(|_| anyhow!("failed to map buffer as a readable video frame"))?;

        // Fail here rather than at upload time.
        for plane in 0..2 {
            mapped
                .plane_data(plane)
                .map_err(|_| anyhow!("frame has no plane {plane}"))?;
        }

        Ok(Frame {
            width: info.width(),
            height: info.height(),
            time,
            fps: info.fps(),
            layout,
            mapped,
        })
    }
}

impl Debug for Frame {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Frame")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("layout", &self.layout)
            .finish()
    }
}
