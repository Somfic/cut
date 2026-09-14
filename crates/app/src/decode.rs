use std::sync::Arc;
use std::sync::atomic::Ordering;
use std::time::Instant;

use cut_engine::playback;
use futures::StreamExt;

use crate::state::Shared;

/// Run the transport, filling `Shared::frame` for the uploader.
pub fn spawn_decoder(shared: Arc<Shared>, project: std::path::PathBuf) {
    shared.running.store(true, Ordering::Relaxed);

    // On stderr as well as in the page, so the numbers are still readable
    // when the webview is the thing that has gone wrong.
    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            let (mut frames, mut presents, mut dropped) = (0, 0, 0);
            let mut since = Instant::now();

            while shared.running.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_secs(2));

                let now = Instant::now();
                let secs = (now - since).as_secs_f64();
                since = now;

                let (f, p, d) = (
                    shared.frames.load(Ordering::Relaxed),
                    shared.presents.load(Ordering::Relaxed),
                    shared.dropped.load(Ordering::Relaxed),
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
                            *shared.controls.lock().unwrap() = Some(controls);
                        }
                        playback::Event::Opened { timeline, .. } => {
                            *shared.timeline.lock().unwrap() = Some(timeline);
                        }
                        playback::Event::Frame(frame) => {
                            shared.frames.fetch_add(1, Ordering::Relaxed);
                            let fps = frame.fps.numer() as f64 / frame.fps.denom() as f64;
                            if fps > 0.0 {
                                *shared.fps.lock().unwrap() = fps;
                            }

                            let previous = shared.frame.lock().unwrap().replace(frame);
                            if previous.is_some() {
                                shared.dropped.fetch_add(1, Ordering::Relaxed);
                            }
                            shared.arrived.notify_one();
                        }
                    }
                }
            })
        });
    }

}

