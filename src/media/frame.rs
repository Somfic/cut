use anyhow::{Context, anyhow};
use gstreamer::{BufferRef, Fraction, Sample};
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

pub struct Frame {
    pub width: u32,
    pub height: u32,
    pub fps: Fraction,
    pub time: Duration,
    pub layout: PixelLayout,
    /// Raw luma plane, tightly packed (no row padding).
    pub y: Vec<u8>,
    /// Raw interleaved chroma plane, tightly packed.
    pub uv: Vec<u8>,
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

        let (layout, bytes_per_sample) = match info.format() {
            VideoFormat::Nv12 => (PixelLayout::Nv12, 1usize),
            VideoFormat::P01010le => (PixelLayout::P010, 2usize),
            other => return Err(anyhow!("unsupported pixel format: {other:?}")),
        };

        let w = width as usize;
        let h = height as usize;
        // Both planes are `width` samples wide (chroma = width/2 Cb,Cr pairs);
        // luma spans all rows, chroma spans half.
        let row_bytes = w * bytes_per_sample;
        let y = pack_plane(&frame, 0, row_bytes, h)?;
        let uv = pack_plane(&frame, 1, row_bytes, h / 2)?;

        Ok(Frame {
            width,
            height,
            time,
            fps: info.fps(),
            layout,
            y,
            uv,
        })
    }
}

fn pack_plane(
    frame: &gst_video::VideoFrameRef<&BufferRef>,
    idx: u32,
    row_bytes: usize,
    rows: usize,
) -> anyhow::Result<Vec<u8>> {
    let src = frame
        .plane_data(idx)
        .map_err(|_| anyhow!("no plane {idx}"))?;
    let src_stride = frame.plane_stride()[idx as usize] as usize;
    let mut out = vec![0u8; row_bytes * rows];
    for row in 0..rows {
        let s = row * src_stride;
        let d = row * row_bytes;
        out[d..d + row_bytes].copy_from_slice(&src[s..s + row_bytes]);
    }
    Ok(out)
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
