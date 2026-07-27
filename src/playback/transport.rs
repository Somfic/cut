use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_timer::Delay;
use iced::futures::Stream;
use iced::futures::channel::mpsc::{self, Sender};
use iced::futures::{FutureExt, SinkExt, StreamExt, select};

use crate::app::Event;
use crate::demo;
use crate::project::Timeline;
use crate::playback::{Controls, Engine, PlaybackState, Request, SeekMode, VideoStream};

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

pub fn transport() -> impl Stream<Item = Event> {
    iced::stream::channel(64, async |mut output: Sender<Event>| {
        let (command_tx, mut command_rx) = mpsc::channel::<Request>(16);
        let (mut engine, mut stream) = Engine::new();

        // `playing` is the engine's own flag, not a copy of it.
        let state = Arc::new(PlaybackState::new(engine.playing_flag()));
        let controls = Controls::new(command_tx, state.clone());
        output.send(Event::Ready(controls)).await.ok();

        let timeline = match demo::timeline() {
            Ok(timeline) => Arc::new(timeline),
            Err(e) => {
                eprintln!("could not build the demo timeline: {e}");
                return;
            }
        };
        output.send(Event::Opened(timeline.clone())).await.ok();

        let length = timeline.length();
        if length == 0 {
            eprintln!("timeline is empty");
            return;
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
                    // Relative seeks resolve against the last playhead and then
                    // park, so every seek takes the same route through `cut`.
                    Request::Step((delta, mode)) => {
                        parked = Some((
                            playhead.saturating_add_signed(delta as isize).min(length - 1),
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
                        .map(|c| (c.position, c.source_start, c.length, c.source.fps))
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
                        let next = (position + clip_len) % length;
                        cut(&mut engine, &timeline, next, &mut stream, SeekMode::Accurate);
                        in_flight = Some(Instant::now());
                        continue;
                    }

                    let timeline_frame = position + (source_frame - source_start);
                    playhead = timeline_frame;
                    state.set_playhead(timeline_frame);

                    let target = frame.time;
                    let lag_ms = engine.position().as_secs_f64() * 1000.0
                        - target.as_secs_f64() * 1000.0;
                    state.set_lag_ms(lag_ms.round() as i64);

                    if parked.is_none() && target > engine.position() {
                        Delay::new(target - engine.position()).await;
                    }
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
                        .map(|c| (c.position + c.length) % length)
                        .unwrap_or(0);
                    cut(&mut engine, &timeline, next, &mut stream, SeekMode::Fast);
                    in_flight = Some(Instant::now());
                }
                complete => break,
            }
        }
    })
}
