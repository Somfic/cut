use anyhow::{Context, anyhow};
use gstreamer::{self as gst, State};
use gstreamer::{SeekFlags, prelude::*};
use gstreamer_app::AppSink;
use gstreamer_app::{self as gst_app, AppSinkCallbacks};
use gstreamer_video::VideoInfo;
use iced::futures::channel::mpsc::{self, Receiver};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

mod frame;
mod renderer;
mod view;

pub use frame::Frame;
pub use renderer::FrameRenderer;
pub use view::VideoView;

use crate::clock::Clock;

pub struct Video {
    pub path: PathBuf,
    pipeline: gst::Pipeline,
    sink: AppSink,
    clock: Clock,
}

impl Video {
    pub fn new(path: impl Into<PathBuf>) -> anyhow::Result<(Self, Receiver<Arc<Frame>>)> {
        let path = path.into();
        let path_str = path
            .to_str()
            .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", path.display()))?;

        // build pipeline
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

        // app sink
        let sink = pipeline
            .by_name("sink")
            .context("no element named 'sink'")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("'sink' was not an AppSink"))?;

        // callbacks
        let (mut frame_sender, frame_receiver) = mpsc::channel::<Arc<Frame>>(4);
        let mut eos_sender = frame_sender.clone();
        sink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let frame: Frame = sample.try_into().map_err(|_| gst::FlowError::Error)?;
                    match frame_sender.try_send(Arc::new(frame)) {
                        Ok(()) => Ok(gst::FlowSuccess::Ok),
                        Err(e) if e.is_full() => Ok(gst::FlowSuccess::Ok),
                        Err(_) => Err(gst::FlowError::Eos),
                    }
                })
                .eos(move |_| eos_sender.close_channel())
                .build(),
        );

        Ok((
            Video {
                path,
                pipeline,
                sink,
                clock: Clock::new(),
            },
            frame_receiver,
        ))
    }

    pub fn play(&mut self) {
        self.pipeline.set_state(State::Playing).ok();
        self.clock.resume();
    }

    pub fn pause(&mut self) {
        self.pipeline.set_state(State::Paused).ok();
        self.clock.pause();
    }

    pub fn toggle(&mut self) {
        if self.clock.is_playing() {
            self.pause()
        } else {
            self.play()
        }
    }

    pub fn position(&self) -> Duration {
        self.clock.position()
    }

    pub fn seek(&mut self, delta_frames: i64, frames: &mut Receiver<Arc<Frame>>) {
        let Some(fps) = self.fps() else { return };
        let current = (self.clock.position().as_secs_f64() * fps).round() as i64;
        let target_frame = (current + delta_frames).max(0);
        let target = Duration::from_secs_f64(target_frame as f64 / fps);

        let pos = gst::ClockTime::from_nseconds(target.as_nanos() as u64);
        self.pipeline
            .seek_simple(SeekFlags::FLUSH | SeekFlags::ACCURATE, pos) // TODO: quick vs accurate seeking
            .ok();

        while let Ok(_) = frames.try_recv() {}

        self.clock.seek_to(target);
    }

    pub fn fps(&self) -> Option<f64> {
        let caps = self.sink.static_pad("sink")?.current_caps()?;
        let info = VideoInfo::from_caps(&caps).ok()?;
        let fps = info.fps();
        Some(fps.numer() as f64 / fps.denom() as f64)
    }
}

impl Drop for Video {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}
