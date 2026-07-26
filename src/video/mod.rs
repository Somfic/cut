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
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
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
    video_sink: AppSink,
    audio_sink: AppSink,
    clock: Clock,
    flush_audio: Arc<AtomicBool>,
    is_playing: bool,
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
            dec. ! queue ! videoconvert ! video/x-raw,format=RGBA ! appsink name=video max-buffers=1 drop=true \
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

        Ok((
            Video {
                path,
                pipeline,
                video_sink,
                audio_sink,
                clock,
                flush_audio,
                audio_playing,
                is_playing: false,
            },
            frame_receiver,
        ))
    }

    pub fn play(&mut self) {
        self.is_playing = true;
        self.audio_playing.store(true, Ordering::Relaxed);
        self.pipeline.set_state(State::Playing).ok();
    }

    pub fn pause(&mut self) {
        self.is_playing = false;
        self.audio_playing.store(false, Ordering::Relaxed);
        self.pipeline.set_state(State::Paused).ok();
    }

    pub fn toggle(&mut self) {
        if self.is_playing {
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
