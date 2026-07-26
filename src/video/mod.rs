use anyhow::{Context, anyhow};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use gstreamer::{self as gst, State};
use gstreamer::{SeekFlags, prelude::*};
use gstreamer_app::AppSink;
use gstreamer_app::{self as gst_app, AppSinkCallbacks};
use gstreamer_video::VideoInfo;
use iced::futures::channel::mpsc::{self, Receiver};
use ringbuf::HeapRb;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicU64, Ordering};
use std::time::Duration;

mod frame;
mod renderer;
mod view;

pub use frame::{Frame, PixelLayout};
pub use renderer::FrameRenderer;
pub use view::VideoView;

use crate::clock::Clock;

pub struct Video {
    pub path: PathBuf,
    pipeline: gst::Pipeline,
    video_sink: AppSink,
    audio_sink: AppSink,
    clock: Clock,
    /// Signed milliseconds the last presented frame trailed the master clock
    /// (positive = behind). Written by the playback loop, read by the UI.
    lag_ms: AtomicI64,
    flush_audio: Arc<AtomicBool>,
    is_playing: AtomicBool,
    audio_playing: Arc<AtomicBool>,
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
            dec. ! queue ! appsink name=video max-buffers=1 drop=true \
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

        let channels = 2; // TODO
        let rate = 48_000; // TODO
        let clock = Clock::new(rate);

        let ring = HeapRb::<f32>::new(rate);
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
        let counter = clock.counter();

        let flush_audio = Arc::new(AtomicBool::new(false));
        let audio_playing = Arc::new(AtomicBool::new(false));

        std::thread::spawn({
            let flush_audio = flush_audio.clone();
            let audio_playing = audio_playing.clone();

            // 120ms of audio cushion
            let cushion = (rate as usize) * channels * 120 / 1000;
            let mut priming = true;

            // 6ms audio fade on pause/play
            let mut gain: f32 = 0.0;
            let fade_step = 1.0 / (rate as f32 * 0.006);

            move || {
                let device = cpal::default_host()
                    .default_output_device()
                    .expect("no output device");
                let config = cpal::StreamConfig {
                    channels: channels as u16,
                    sample_rate: rate as u32,
                    buffer_size: cpal::BufferSize::Default,
                };
                let stream = device
                    .build_output_stream(
                        config,
                        move |data: &mut [f32], _: &cpal::OutputCallbackInfo| {
                            if flush_audio.swap(false, Ordering::Relaxed) {
                                audio_receiver.clear(); // drop stale pre-seek audio
                                priming = true;
                            }

                            let wants_to_play = audio_playing.load(Ordering::Relaxed);

                            if !wants_to_play && gain <= 0.0 {
                                data.fill(0.0);
                                return;
                            }

                            if priming {
                                if audio_receiver.occupied_len() < cushion {
                                    data.fill(0.0); // silence while the cushion refills
                                    return; // counter did not advance
                                }
                                priming = false;
                            }

                            let filled = audio_receiver.pop_slice(data);
                            counter.fetch_add((filled / channels) as u64, Ordering::Relaxed);
                            if filled < data.len() {
                                data[filled..].fill(0.0);
                                priming = true; // underran, start priming
                            }

                            // use gain to counteract clicking on play/pause
                            let target = if wants_to_play { 1.0 } else { 0.0 };
                            for frame in data.chunks_mut(channels) {
                                gain = if gain < target {
                                    (gain + fade_step).min(target)
                                } else {
                                    (gain - fade_step).max(target)
                                };
                                for s in frame {
                                    *s *= gain;
                                }
                            }
                        },
                        |err| eprintln!("audio stream error: {err}"),
                        None, // timeout
                    )
                    .expect("build output stream");
                stream.play().expect("play");
                loop {
                    std::thread::park();
                } // hold the stream alive
            }
        }); // detach for now

        // Preroll before handing the pipeline over: this decodes the first frame,
        // which is what makes caps (and so `fps`) and `duration` available.
        // Without it the first seek would find no fps and silently do nothing.
        pipeline
            .set_state(State::Paused)
            .context("failed to pause pipeline")?;
        pipeline
            .state(gst::ClockTime::from_seconds(5))
            .0
            .context("pipeline failed to preroll")?;

        Ok((
            Video {
                path,
                pipeline,
                video_sink,
                audio_sink,
                clock,
                lag_ms: AtomicI64::new(0),
                flush_audio,
                audio_playing,
                is_playing: AtomicBool::new(false),
            },
            frame_receiver,
        ))
    }

    pub fn play(&self) {
        self.is_playing.store(true, Ordering::Relaxed);
        self.audio_playing.store(true, Ordering::Relaxed);
        self.pipeline.set_state(State::Playing).ok();
    }

    pub fn pause(&self) {
        self.is_playing.store(false, Ordering::Relaxed);
        self.audio_playing.store(false, Ordering::Relaxed);
        self.pipeline.set_state(State::Paused).ok();
    }

    pub fn toggle(&self) {
        if self.is_playing.load(Ordering::Relaxed) {
            self.pause()
        } else {
            self.play()
        }
    }

    pub fn is_playing(&self) -> bool {
        self.is_playing.load(Ordering::Relaxed)
    }

    pub fn duration(&self) -> Option<Duration> {
        self.pipeline
            .query_duration::<gst::ClockTime>()
            .map(|time| Duration::from_nanos(time.nseconds()))
    }

    /// Total length in frames, for laying a clip out on the timeline.
    pub fn frame_count(&self) -> Option<usize> {
        Some((self.duration()?.as_secs_f64() * self.fps()?).round() as usize)
    }

    pub fn position(&self) -> Duration {
        self.clock.position()
    }

    /// Record how far the last presented frame trailed the master clock.
    pub fn set_lag(&self, ms: i64) {
        self.lag_ms.store(ms, Ordering::Relaxed);
    }

    /// Signed milliseconds behind the master clock; positive means the video is
    /// lagging (can't keep up), negative means it's comfortably ahead.
    pub fn lag_ms(&self) -> i64 {
        self.lag_ms.load(Ordering::Relaxed)
    }

    pub fn seek(&self, delta_frames: i64, frames: &mut Receiver<Arc<Frame>>, mode: SeekMode) {
        let Some(fps) = self.fps() else { return };
        let current = (self.clock.position().as_secs_f64() * fps).round() as i64;
        self.seek_to_frame((current + delta_frames).max(0) as usize, frames, mode);
    }

    pub fn seek_to_frame(&self, frame: usize, frames: &mut Receiver<Arc<Frame>>, mode: SeekMode) {
        let Some(fps) = self.fps() else { return };
        let target = Duration::from_secs_f64(frame as f64 / fps);

        let seekflag = match mode {
            SeekMode::Fast => SeekFlags::KEY_UNIT,
            SeekMode::Accurate => SeekFlags::ACCURATE,
        };

        let pos = gst::ClockTime::from_nseconds(target.as_nanos() as u64);
        self.pipeline
            .seek_simple(SeekFlags::FLUSH | seekflag, pos)
            .ok();

        while let Ok(_) = frames.try_recv() {} // flush video
        self.flush_audio.store(true, Ordering::Relaxed); // flush audio

        self.clock.seek_to(target);
    }

    pub fn fps(&self) -> Option<f64> {
        let caps = self.video_sink.static_pad("sink")?.current_caps()?;
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

#[derive(Clone)]
pub enum SeekMode {
    Fast,
    Accurate,
}
