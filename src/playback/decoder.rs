use crate::media::{Frame, Source};
use crate::playback::{AudioSink, VideoSink, VideoStream};
use anyhow::{Context, anyhow};
use gstreamer::prelude::*;
use gstreamer::{self as gst, SeekFlags, State};
use gstreamer_app::{self as gst_app, AppSinkCallbacks};
use iced::futures::channel::mpsc::{self, Receiver, Sender};
use ringbuf::traits::Producer;
use std::sync::Arc;
use std::time::Duration;

#[derive(Clone, Copy)]
pub enum SeekMode {
    Fast,
    Accurate,
}

pub struct Decoder {
    source: Arc<Source>,
    pipeline: gst::Pipeline,
}

impl Decoder {
    pub fn for_source(
        source: Arc<Source>,
        audio: AudioSink,
        video: VideoSink,
    ) -> anyhow::Result<Self> {
        let path_str = source
            .path
            .to_str()
            .ok_or_else(|| anyhow!("path is not valid UTF-8: {}", source.path.display()))?;

        let pipeline = gst::parse::launch(
            "filesrc name=src ! decodebin name=dec \
            dec. ! queue ! appsink name=video max-buffers=1 drop=true \
            dec. ! queue ! audioconvert ! audioresample ! audio/x-raw,format=F32LE,layout=interleaved,channels=2,rate=48000 ! appsink name=audio",
        )
        .context("failed to parse pipeline")?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow!("parsed element was not a Pipeline"))?;

        pipeline
            .by_name("src")
            .context("no element named 'src'")?
            .set_property("location", path_str);

        // video → the returned receiver
        let video_sink = pipeline
            .by_name("video")
            .context("no element named 'video'")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("'video' was not an AppSink"))?;

        video_sink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample({
                    let mut video = video.clone();
                    move |sink| {
                        let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        let frame = sample.try_into().map_err(|_| gst::FlowError::Error)?;
                        if video.write(frame) {
                            Ok(gst::FlowSuccess::Ok)
                        } else {
                            Err(gst::FlowError::Eos)
                        }
                    }
                })
                .new_preroll({
                    let mut video = video.clone();
                    move |sink| {
                        let sample = sink.pull_preroll().map_err(|_| gst::FlowError::Eos)?;
                        let frame = sample.try_into().map_err(|_| gst::FlowError::Error)?;
                        if video.write(frame) {
                            Ok(gst::FlowSuccess::Ok)
                        } else {
                            Err(gst::FlowError::Eos)
                        }
                    }
                })
                .build(),
        );

        let audio_sink = pipeline
            .by_name("audio")
            .context("no element named 'audio'")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("'audio' was not an AppSink"))?;

        audio_sink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                    let samples: &[f32] = bytemuck::cast_slice(&map);
                    audio.write(samples);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        // preroll the first frame
        pipeline
            .set_state(State::Paused)
            .context("failed to pause pipeline")?;

        pipeline
            .state(gst::ClockTime::from_seconds(5))
            .0
            .context("pipeline failed to preroll")?;

        Ok(Self { source, pipeline })
    }

    pub fn play(&self) {
        self.pipeline.set_state(State::Playing).ok();
    }

    pub fn pause(&self) {
        self.pipeline.set_state(State::Paused).ok();
    }

    pub fn fps(&self) -> f64 {
        self.source.fps
    }

    pub fn seek_to_frame(
        &self,
        frame: usize,
        stream: &mut VideoStream,
        mode: SeekMode,
    ) -> Duration {
        let fps = if self.source.fps > 0.0 {
            self.source.fps
        } else {
            30.0
        };
        let target = Duration::from_secs_f64(frame as f64 / fps);

        let seekflag = match mode {
            SeekMode::Fast => SeekFlags::KEY_UNIT | SeekFlags::SNAP_BEFORE,
            SeekMode::Accurate => SeekFlags::ACCURATE,
        };
        let pos = gst::ClockTime::from_nseconds(target.as_nanos() as u64);
        self.pipeline
            .seek_simple(SeekFlags::FLUSH | seekflag, pos)
            .ok();

        stream.drain();

        target
    }
}

impl Drop for Decoder {
    fn drop(&mut self) {
        let _ = self.pipeline.set_state(State::Null);
    }
}
