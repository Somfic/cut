use anyhow::{Context, anyhow};
use gstreamer::prelude::*;
use gstreamer::{self as gst, Fraction, State};
use gstreamer_app::{self as gst_app};
use gstreamer_video::VideoInfo;
use std::path::{Path, PathBuf};
use std::time::Duration;

pub struct Source {
    pub path: PathBuf,
    pub duration: Duration,
    /// The rational the file declares, not a divided-out float: 29.97 fps is
    /// 30000/1001, and every stored frame index leans on it.
    pub frame_rate: Fraction,
}

impl Source {
    pub fn new(path: impl Into<PathBuf>) -> anyhow::Result<Self> {
        let path = path.into();
        let (duration, frame_rate) = probe(&path)?;
        Ok(Self {
            path,
            duration,
            frame_rate,
        })
    }

    pub fn fps(&self) -> f64 {
        self.frame_rate.numer() as f64 / self.frame_rate.denom() as f64
    }

    pub fn frame_count(&self) -> usize {
        (self.duration.as_secs_f64() * self.fps()).round() as usize
    }
}

fn probe(path: &Path) -> anyhow::Result<(Duration, Fraction)> {
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

    let frame_rate = vsink
        .static_pad("sink")
        .and_then(|pad| pad.current_caps())
        .and_then(|caps| VideoInfo::from_caps(&caps).ok())
        .map(|info| info.fps())
        .filter(|fps| fps.numer() > 0 && fps.denom() > 0)
        .unwrap_or_else(|| Fraction::new(30, 1));

    let _ = pipeline.set_state(State::Null);

    Ok((duration, frame_rate))
}
