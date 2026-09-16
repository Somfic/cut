use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use futures::channel::mpsc::{self, Sender};
use futures::{FutureExt, SinkExt, Stream, StreamExt, select};
use futures_timer::Delay;

use crate::demo;
use crate::media::Frame;
use crate::playback::{Controls, Engine, PlaybackState, Request, SeekMode, VideoStream};
use crate::project::{Timeline, file};
use crate::stream;

/// Everything the transport tells a front end about. Deliberately small: a
/// front end renders these three things and drives the rest through
/// [`Controls`], so it needs no view into the engine's internals.
#[derive(Clone)]
pub enum Event {
    /// The handle for driving playback. Arrives once, first.
    Ready(Controls),
    Opened {
        timeline: Arc<Timeline>,
        /// False when this came from the demo fallback instead of the project
        /// file, which is what makes autosave write it out and gives the
        /// project a file to begin with.
        on_disk: bool,
    },
    Frame(Arc<Frame>),
}

const SEEK_TIMEOUT: Duration = Duration::from_millis(500);
const PLAYBACK_STALL: Duration = Duration::from_secs(3);

/// Move the timeline to `tl_frame`. Every seek, clip boundary and scrub goes
/// through here, so there is one path from "I want this frame" to the decoders.
fn cut(
    engine: &mut Engine,
    timeline: &Timeline,
    tl_frame: usize,
    stream: &mut VideoStream,
    mode: SeekMode,
) {
    if let Err(e) = engine.cut_to(timeline, tl_frame, stream, mode) {
        eprintln!("could not cut to timeline frame {tl_frame}: {e}");
    }
}

/// Read the project file, falling back to the demo timeline when nothing is
/// there yet — saving over that fallback is how the first project gets written.
fn open(project: &Path) -> anyhow::Result<(Timeline, bool)> {
    if project.exists() {
        let timeline =
            file::load(project).with_context(|| format!("could not open {}", project.display()))?;

        Ok((timeline, true))
    } else {
        let timeline = demo::timeline().context("could not build the demo timeline")?;

        Ok((timeline, false))
    }
}

// `use<>`: the returned stream owns a clone of the path and captures nothing
// from the borrow, which is what lets this be the plain `fn(&D) -> S` pointer
// `Subscription::run_with` wants.
pub fn transport(project: &PathBuf) -> impl Stream<Item = Event> + use<> {
    let project = project.clone();

    stream::channel(64, async move |mut output: Sender<Event>| {
        let (command_tx, mut command_rx) = mpsc::channel::<Request>(16);
        let (mut engine, mut stream) = Engine::new();

        // `playing` is the engine's own flag, not a copy of it.
        let state = Arc::new(PlaybackState::new(engine.playing_flag()));
        let controls = Controls::new(command_tx, state.clone());
        output.send(Event::Ready(controls)).await.ok();

        let (mut timeline, on_disk) = match open(&project) {
            Ok((timeline, on_disk)) => (Arc::new(timeline), on_disk),
            Err(e) => {
                eprintln!("{e:#}");
                return;
            }
        };
        output
            .send(Event::Opened {
                timeline: timeline.clone(),
                on_disk,
            })
            .await
            .ok();

        let mut length = timeline.length();
        if length == 0 {
            // Not fatal: an empty document is what importing into a new
            // project starts from.
            eprintln!("timeline is empty — import some media");
        }

        // Accurate: the seek's segment starts exactly at the in-point, so
        // gstreamer drops both audio and video from the keyframe up to it —
        // no leading video frames, and no unrelated leading audio.
        cut(&mut engine, &timeline, 0, &mut stream, SeekMode::Accurate);
        engine.play();

        let mut in_flight: Option<Instant> = None;
        let mut parked: Option<(usize, SeekMode)> = None;
        let mut playhead = 0usize;

        loop {
            if in_flight.is_some_and(|at| at.elapsed() > SEEK_TIMEOUT) {
                in_flight = None;
            }

            if in_flight.is_none()
                && let Some((tl_frame, mode)) = parked.take()
            {
                cut(&mut engine, &timeline, tl_frame, &mut stream, mode);
                in_flight = Some(Instant::now());
            }

            select! {
                cmd = command_rx.select_next_some() => match cmd {
                    Request::TogglePlayback => engine.toggle(),
                    Request::Pause => engine.pause(),
                    Request::Open(opened) => {
                        timeline = opened;
                        length = timeline.length();
                        // Stay where the playhead was if the new document
                        // still reaches that far — an import appends, so it
                        // usually does.
                        parked = Some((
                            playhead.min(length.saturating_sub(1)),
                            SeekMode::Accurate,
                        ));
                    }
                    // Relative seeks resolve against the last playhead and then
                    // park, so every seek takes the same route through `cut`.
                    Request::Step((delta, mode)) => {
                        parked = Some((
                            playhead
                                .saturating_add_signed(delta as isize)
                                .min(length.saturating_sub(1)),
                            mode,
                        ));
                    }
                    Request::Seek(seek) => parked = Some(seek),
                },
                frame = stream.select_next_some() => {
                    in_flight = None;

                    // Which clip produced this frame — and its fps, since frame
                    // times are in that clip's own source, not the timeline's.
                    let Some((position, source_start, clip_len, fps)) = engine
                        .live_clip(0)
                        .map(|c| (c.position, c.source_start, c.length, c.source.fps()))
                    else {
                        continue;
                    };
                    let source_frame = (frame.time.as_secs_f64() * fps).round() as usize;

                    // A KEY_UNIT seek lands on a keyframe at or before the
                    // in-point. Drop the leading frames until we reach it: they
                    // can be corrupt RASL pictures (they reference frames before
                    // the random-access keyframe) and aren't the moment we asked
                    // for. This also discards stragglers from the outgoing
                    // decoder after a cross-source cut.
                    if source_frame < source_start {
                        continue;
                    }

                    // Played the clip's length → cut to the frame just past its
                    // out-point, which is wherever the next clip begins (looping
                    // at the end of the timeline). Timeline arithmetic, so the
                    // worker never has to know about clip indices.
                    if source_frame >= source_start + clip_len {
                        // Not simply the next frame along: a move or a delete
                        // can leave a hole with nothing in it to decode, and
                        // `next_content` wraps at the end of the document.
                        let next = timeline.next_content(position + clip_len).unwrap_or(0);
                        cut(&mut engine, &timeline, next, &mut stream, SeekMode::Accurate);
                        in_flight = Some(Instant::now());
                        continue;
                    }

                    let timeline_frame = position + (source_frame - source_start);
                    playhead = timeline_frame;

                    let target = frame.time;
                    let lag_ms = engine.position().as_secs_f64() * 1000.0
                        - target.as_secs_f64() * 1000.0;
                    state.set_lag_ms(lag_ms.round() as i64);

                    if parked.is_none() && target > engine.position() {
                        Delay::new(target - engine.position()).await;
                    }

                    // After the wait, not before: the playhead should describe
                    // the picture being shown. Published early it runs up to a
                    // frame ahead of the preview, and the UI — which samples it
                    // at display rate — sees it step at odd moments.
                    state.set_playhead(timeline_frame);
                    if output.send(Event::Frame(frame)).await.is_err() { break; }
                }
                // Watchdog: if no frame arrives for a while, the pipeline has
                // likely hit EOS (a clip window reached the end of the file) or
                // a seek stalled. Advance to the next clip to recover instead of
                // hanging forever. During normal playback frames arrive every
                // ~40ms, so this never fires.
                _ = Delay::new(PLAYBACK_STALL).fuse() => {
                    let next = engine
                        .live_clip(0)
                        .and_then(|c| timeline.next_content(c.position + c.length))
                        .unwrap_or(0);
                    cut(&mut engine, &timeline, next, &mut stream, SeekMode::Fast);
                    in_flight = Some(Instant::now());
                }
                complete => break,
            }
        }
    })
}
