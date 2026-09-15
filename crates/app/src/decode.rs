use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use cut_engine::playback;
use futures::StreamExt;

use crate::state::State;

pub fn spawn_decoder(shared: Arc<State>, project: std::path::PathBuf) {
    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            let (mut frames, mut presents, mut dropped) = (0, 0, 0);
            let mut since = Instant::now();

            loop {
                std::thread::sleep(std::time::Duration::from_secs(2));

                let now = Instant::now();
                let secs = (now - since).as_secs_f64();
                since = now;

                let (f, p, d) = (
                    shared.counters.frames.load(Ordering::Relaxed),
                    shared.counters.presents.load(Ordering::Relaxed),
                    shared.counters.dropped.load(Ordering::Relaxed),
                );
                eprintln!(
                    "video {:.1} fps · present {:.1} fps · dropped {:.1}/s",
                    (f - frames) as f64 / secs,
                    (p - presents) as f64 / secs,
                    (d - dropped) as f64 / secs,
                );
                (frames, presents, dropped) = (f, p, d);
            }
        });
    }

    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            futures::executor::block_on(async move {
                let mut events = Box::pin(playback::transport(&project));

                while let Some(event) = events.next().await {
                    match event {
                        playback::Event::Ready(controls) => {
                            *shared.session.controls.lock().unwrap() = Some(controls);
                        }
                        playback::Event::Opened { timeline, .. } => {
                            *shared.session.timeline.lock().unwrap() = Some(timeline);
                        }
                        playback::Event::Frame(frame) => {
                            shared.counters.frames.fetch_add(1, Ordering::Relaxed);
                            let fps = frame.fps.numer() as f64 / frame.fps.denom() as f64;
                            if fps > 0.0 {
                                *shared.session.fps.lock().unwrap() = fps;
                            }

                            if shared.slot.put(frame) {
                                shared.counters.dropped.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            })
        });
    }
}
