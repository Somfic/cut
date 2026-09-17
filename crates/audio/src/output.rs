use crate::Clock;
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapProd, HeapRb};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

pub const CHANNELS: usize = 2;
pub const RATE: usize = 48_000;

#[derive(Clone)]
pub struct AudioSink(Arc<Mutex<HeapProd<f32>>>);

impl AudioSink {
    pub fn write(&self, samples: &[f32]) {
        if let Ok(mut prod) = self.0.lock() {
            prod.push_slice(samples);
        }
    }
}

pub struct Output {
    pub sink: AudioSink,
    pub clock: Arc<Clock>,
    pub flush: Arc<AtomicBool>,
    pub playing: Arc<AtomicBool>,
}

impl Output {
    pub fn start() -> Self {
        let clock = Arc::new(Clock::new(RATE));
        let counter = clock.counter();

        let (producer, mut receiver) = HeapRb::<f32>::new(RATE).split();
        let flush = Arc::new(AtomicBool::new(false));
        let playing = Arc::new(AtomicBool::new(false));

        std::thread::spawn({
            let flush = flush.clone();
            let playing = playing.clone();

            let cushion = RATE * CHANNELS * 120 / 1000; // 120ms cushion
            let mut priming = true;
            let mut gain: f32 = 0.0;
            let fade_step = 1.0 / (RATE as f32 * 0.006); // 6ms fade on play/pause

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
                            if flush.swap(false, Ordering::Relaxed) {
                                receiver.clear(); // drop stale pre-seek audio
                                priming = true;
                            }

                            let wants_to_play = playing.load(Ordering::Relaxed);

                            if !wants_to_play && gain <= 0.0 {
                                data.fill(0.0);
                                return;
                            }

                            if priming {
                                if receiver.occupied_len() < cushion {
                                    data.fill(0.0); // silence while the cushion refills
                                    return; // counter did not advance
                                }
                                priming = false;
                            }

                            let filled = receiver.pop_slice(data);
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

                // hold the stream alive
                loop {
                    std::thread::park();
                }
            }
        });

        Output {
            sink: AudioSink(Arc::new(Mutex::new(producer))),
            clock,
            flush,
            playing,
        }
    }
}
