use crate::{media::Frame, playback::Clock};
use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use iced::futures::Stream;
use iced::futures::channel::mpsc::{self, Receiver, Sender};
use iced::futures::stream::FusedStream;
use ringbuf::traits::{Consumer, Observer, Producer, Split};
use ringbuf::{HeapProd, HeapRb};
use std::pin::Pin;
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex, atomic::AtomicBool};
use std::task::{Context, Poll};

const CHANNELS: usize = 2;
const RATE: usize = 48_000;

#[derive(Clone)]
pub struct AudioSink(Arc<Mutex<HeapProd<f32>>>);

impl AudioSink {
    pub fn write(&self, samples: &[f32]) {
        if let Ok(mut prod) = self.0.lock() {
            prod.push_slice(samples);
        }
    }
}

#[derive(Clone)]
pub struct VideoSink(Sender<Arc<Frame>>);

impl VideoSink {
    pub fn write(&mut self, frame: Frame) -> bool {
        match self.0.try_send(Arc::new(frame)) {
            Ok(()) => true,
            Err(e) => e.is_full(),
        }
    }
}

pub struct VideoStream(Receiver<Arc<Frame>>);

impl VideoStream {
    pub fn drain(&mut self) {
        while let Ok(_) = self.0.try_recv() {}
    }
}

impl Stream for VideoStream {
    type Item = Arc<Frame>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(cx)
    }
}

impl FusedStream for VideoStream {
    fn is_terminated(&self) -> bool {
        self.0.is_terminated()
    }
}

#[derive(Clone)]
pub struct Sinks {
    pub clock: Arc<Clock>,
    pub audio_sink: AudioSink,
    pub flush_audio: Arc<AtomicBool>,
    pub audio_playing: Arc<AtomicBool>,
    pub video_sink: VideoSink,
}

impl Sinks {
    pub fn new() -> (Self, VideoStream) {
        let clock = Arc::new(Clock::new(RATE));
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

        // 4 frame buffer
        let (sender, receiver) = mpsc::channel::<Arc<Frame>>(4);

        let sinks = Sinks {
            clock,
            audio_sink: AudioSink(Arc::new(Mutex::new(audio_producer))),
            flush_audio,
            audio_playing,
            video_sink: VideoSink(sender),
        };

        (sinks, VideoStream(receiver))
    }
}
