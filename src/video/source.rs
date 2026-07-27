use anyhow::{Context, anyhow};
use gstreamer::prelude::*;
use gstreamer::{self as gst, State};
use gstreamer_app::{self as gst_app};
use gstreamer_video::VideoInfo;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Source {
    pub path: PathBuf,
    pub duration: Duration,
    pub fps: f64,
}

impl Source {
    pub fn new(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let (duration, fps) = probe(&path)?;
        Ok(Self {
            path,
            duration,
            fps,
        })
    }

    pub fn frame_count(&self) -> usize {
        (self.duration.as_secs_f64() * self.fps).round() as usize
    }
}

fn probe(path: &Path) -> anyhow::Result<(Duration, f64)> {
    let path_str = path
        .to_str()
        .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))?;

    let pipeline = gst::parse::launch(
        "filesrc name=src ! decodebin name=d \
         d. ! queue ! appsink name=vsink \
         d. ! queue ! audioconvert ! fakesink",
    )
    .context("failed to parse probe pipeline")?
    .downcast::<gst::Pipeline>()
    .map_err(|_| anyhow!("parsed element was not a Pipeline"))?;

    pipeline
        .by_name("src")
        .context("no element named 'src'")?
        .set_property("location", path_str);

    let vsink = pipeline
        .by_name("vsink")
        .context("no element named 'vsink'")?
        .downcast::<gst_app::AppSink>()
        .map_err(|_| anyhow!("'vsink' was not an AppSink"))?;

    pipeline
        .set_state(State::Paused)
        .context("failed to pause probe pipeline")?;
    pipeline
        .state(gst::ClockTime::from_seconds(5))
        .0
        .context("probe pipeline failed to preroll")?;

    let duration = pipeline
        .query_duration::<gst::ClockTime>()
        .map(|t| Duration::from_nanos(t.nseconds()))
        .unwrap_or_default();

    let fps = vsink
        .static_pad("sink")
        .and_then(|pad| pad.current_caps())
        .and_then(|caps| VideoInfo::from_caps(&caps).ok())
        .map(|info| {
            let f = info.fps();
            f.numer() as f64 / f.denom() as f64
        })
        .filter(|f| *f > 0.0)
        .unwrap_or(30.0);

    let _ = pipeline.set_state(State::Null);

    Ok((duration, fps))
}
