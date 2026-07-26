use anyhow::{Context, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use gstreamer::{self as gst, State};
use gstreamer::{SeekFlags, prelude::*};
use gstreamer_app::AppSink;
use gstreamer_app::{self as gst_app, AppSinkCallbacks};
use gstreamer_video::VideoInfo;
use iced::futures::channel::mpsc::{self, Receiver};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Producer, Split};
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
            "filesrc name=src ! decodebin name=dec \
            dec. ! queue ! videoconvert ! video/x-raw,format=RGBA ! appsink name=video \
            dec. ! queue ! audioconvert ! audioresample ! audio/x-raw,format=F32LE,channels=2,rate=48000 ! appsink name=audio",
        )
        .context("failed to parse pipeline")?
        .downcast::<gst::Pipeline>()
        .map_err(|_| anyhow!("parsed element was not a Pipeline"))?;

        pipeline
            .by_name("src")
            .context("no element named 'src'")?
            .set_property("location", path_str);

        // video
        let video_sink = pipeline
            .by_name("video")
            .context("no element named 'video'")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("'video' was not an AppSink"))?;

        let (frame_sender, frame_receiver) = mpsc::channel::<Arc<Frame>>(4);
        let mut eos_sender = frame_sender.clone();
        video_sink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample({
                    let mut frame_sender = frame_sender.clone();
                    move |sink| {
                        let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                        let frame: Frame = sample.try_into().map_err(|_| gst::FlowError::Error)?;
                        match frame_sender.try_send(Arc::new(frame)) {
                            Ok(()) => Ok(gst::FlowSuccess::Ok),
                            Err(e) if e.is_full() => Ok(gst::FlowSuccess::Ok),
                            Err(_) => Err(gst::FlowError::Eos),
                        }
                    }
                })
                .new_preroll({
                    let mut frame_sender = frame_sender.clone();
                    move |sink| {
                        let sample = sink.pull_preroll().map_err(|_| gst::FlowError::Eos)?;
                        let frame: Frame = sample.try_into().map_err(|_| gst::FlowError::Error)?;
                        match frame_sender.try_send(Arc::new(frame)) {
                            Ok(()) => Ok(gst::FlowSuccess::Ok),
                            Err(e) if e.is_full() => Ok(gst::FlowSuccess::Ok),
                            Err(_) => Err(gst::FlowError::Eos),
                        }
                    }
                })
                .eos(move |_| eos_sender.close_channel())
                .build(),
        );

        // audio
        let audio_sink = pipeline
            .by_name("audio")
            .context("no element named 'audio'")?
            .downcast::<gst_app::AppSink>()
            .map_err(|_| anyhow!("'audio' was not an AppSink"))?;

        let ring = HeapRb::<f32>::new(48_000);
        let (mut audio_sender, mut audio_receiver) = ring.split();
        audio_sink.set_callbacks(
            AppSinkCallbacks::builder()
                .new_sample(move |sink| {
                    let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                    let buffer = sample.buffer().ok_or(gst::FlowError::Error)?;
                    let map = buffer.map_readable().map_err(|_| gst::FlowError::Error)?;
                    let samples: &[f32] = bytemuck::cast_slice(&map);
                    audio_sender.push_slice(samples);
                    Ok(gst::FlowSuccess::Ok)
                })
                .build(),
        );

        // start audio thread
        std::thread::spawn(move || {
            let device = cpal::default_host()
                .default_output_device()
                .expect("no output device");
            let config = cpal::StreamConfig {
                channels: 2,
                sample_rate: 48_000,
                buffer_size: cpal::BufferSize::Default,
            };
            let stream = device
                .build_output_stream(
                    config,
                    move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                        let filled = audio_receiver.pop_slice(data);
                        data[filled..].fill(0.0); // silence on underrun
                    },
                    |err| eprintln!("audio stream error: {err}"),
                    None, // timeout
                )
                .expect("build output stream");
            stream.play().expect("play");
            loop {
                std::thread::park();
            } // hold the stream alive
        }); // detach for now

        Ok((
            Video {
                path,
                pipeline,
                sink: video_sink,
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
        if self.clock.is_ticking() {
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
        let _ = self.pipeline.set_state(State::Null);
    }
}
