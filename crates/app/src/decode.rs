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
                // Four times a second, because this is the correction the
                // front end's own clock is steered by: it counts frames
                // locally between these, and a whole second of free-running
                // is a second of drift to swallow in one step.
                std::thread::sleep(std::time::Duration::from_millis(250));

                crate::api::transport::publish(&shared);

                let now = Instant::now();
                let secs = (now - since).as_secs_f64();
                if secs < 1.0 {
                    continue;
                }
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
        let state = shared.clone();
        std::thread::spawn(move || {
            futures::executor::block_on(async move {
                let mut events = Box::pin(playback::transport(&project));

                while let Some(event) = events.next().await {
                    match event {
                        playback::Event::Ready(controls) => {
                            *state.session.controls.lock().unwrap() = Some(controls);
                        }
                        playback::Event::Opened { timeline, on_disk } => {
                            if let Some(events) = state.events.get() {
                                events
                                    .timeline
                                    .emit_changed(&crate::api::timeline::dto(&timeline));
                            }
                            *state.session.timeline.lock().unwrap() = Some(timeline);

                            // The demo fallback is a document nothing has
                            // written yet: calling it unsaved is what has
                            // autosave give the project its file, a couple of
                            // seconds later.
                            crate::api::project::opened(&state, on_disk);
                        }
                        playback::Event::Frame(frame) => {
                            state.counters.frames.fetch_add(1, Ordering::Relaxed);
                            let fps = frame.fps.numer() as f64 / frame.fps.denom() as f64;
                            if fps > 0.0 {
                                *state.session.fps.lock().unwrap() = fps;
                            }

                            if state.slot.put(frame) {
                                state.counters.dropped.fetch_add(1, Ordering::Relaxed);
                            }
                        }
                    }
                }
            })
        });
    }
}
