use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use iced::futures::channel::mpsc::Receiver;
use ringbuf::traits::{Consumer, Observer, Split};
use ringbuf::{HeapProd, HeapRb};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use crate::media::{Frame, Source};
use crate::playback::clock::Clock;
use crate::playback::{Decoder, SeekMode};

const CHANNELS: usize = 2;
const RATE: usize = 48_000;

pub type AudioSink = Arc<Mutex<HeapProd<f32>>>;

pub struct Player {
    clock: Clock,
    decoders: Vec<Decoder>,
    audio_out: AudioSink,
    flush_audio: Arc<AtomicBool>,
    audio_playing: Arc<AtomicBool>,
}

impl Player {
    pub fn new() -> Self {
        let clock = Clock::new(RATE);
        let counter = clock.counter();

        let (audio_producer, mut audio_receiver) = HeapRb::<f32>::new(RATE).split();
        let flush_audio = Arc::new(AtomicBool::new(false));
        let audio_playing = Arc::new(AtomicBool::new(false));

        std::thread::spawn({
            let flush_audio = flush_audio.clone();
            let audio_playing = audio_playing.clone();

            let cushion = RATE * CHANNELS * 120 / 1000; // 120ms cushion
            let mut priming = true;
            let mut gain: f32 = 0.0; // 6ms fade on play/pause
            let fade_step = 1.0 / (RATE as f32 * 0.006);

            move || {
                let device = cpal::default_host()
                    .default_output_device()
                    .expect("no output device");
                let config = cpal::StreamConfig {
                    channels: CHANNELS as u16,
                    sample_rate: RATE as u32,
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
                            counter.fetch_add((filled / CHANNELS) as u64, Ordering::Relaxed);
                            if filled < data.len() {
                                data[filled..].fill(0.0);
                                priming = true; // underran, start priming
                            }

                            // gain ramp to counteract clicking on play/pause
                            let target = if wants_to_play { 1.0 } else { 0.0 };
                            for frame in data.chunks_mut(CHANNELS) {
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
                        None,
                    )
                    .expect("build output stream");
                stream.play().expect("play");
                loop {
                    std::thread::park();
                } // hold the stream alive
            }
        });

        Player {
            clock,
            decoders: Vec::new(),
            audio_out: Arc::new(Mutex::new(audio_producer)),
            flush_audio,
            audio_playing,
        }
    }

    pub fn open(&mut self, source: Arc<Source>) -> anyhow::Result<Receiver<Arc<Frame>>> {
        let (decoder, frames) = Decoder::for_source(source, self.audio_out.clone())?;
        self.decoders.push(decoder);
        Ok(frames)
    }

    fn active(&self) -> Option<&Decoder> {
        self.decoders.first()
    }

    pub fn play(&self) {
        self.audio_playing.store(true, Ordering::Relaxed);
        if let Some(d) = self.active() {
            d.play();
        }
    }

    pub fn pause(&self) {
        self.audio_playing.store(false, Ordering::Relaxed);
        if let Some(d) = self.active() {
            d.pause();
        }
    }

    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause();
        } else {
            self.play();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.audio_playing.load(Ordering::Relaxed)
    }

    pub fn position(&self) -> Duration {
        self.clock.position()
    }

    pub fn seek(&self, delta_frames: i64, frames: &mut Receiver<Arc<Frame>>, mode: SeekMode) {
        let Some(d) = self.active() else { return };
        let fps = d.fps();
        if fps <= 0.0 {
            return;
        }
        let current = (self.clock.position().as_secs_f64() * fps).round() as i64;
        self.seek_to_frame((current + delta_frames).max(0) as usize, frames, mode);
    }

    pub fn seek_to_frame(&self, frame: usize, frames: &mut Receiver<Arc<Frame>>, mode: SeekMode) {
        let Some(d) = self.active() else { return };
        let target = d.seek_to_frame(frame, frames, mode);
        self.clock.seek_to(target); // re-base the master clock
        self.flush_audio.store(true, Ordering::Relaxed); // drop stale audio
    }
}

impl Default for Player {
    fn default() -> Self {
        Self::new()
    }
}
