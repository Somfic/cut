use std::sync::Arc;
use std::time::{Duration, Instant};

use futures_timer::Delay;
use iced::futures::Stream;
use iced::futures::channel::mpsc::{self, Sender};
use iced::futures::{FutureExt, SinkExt, StreamExt, select};

use crate::app::Message;
use crate::demo::{CLIP_SECS, N_CLIPS, Rng, SOURCES};
use crate::media::Source;
use crate::playback::{Player, SeekMode};
use crate::timeline::{Clip, Timeline};

/// How long to wait for a seek to produce a frame before assuming it never will.
const SEEK_TIMEOUT: Duration = Duration::from_millis(500);
/// How long playback can go without a frame before the watchdog assumes the
/// pipeline stalled/hit EOS and recovers. Must exceed the worst-case `Accurate`
/// seek, which decodes a whole GOP before emitting its first frame.
const PLAYBACK_STALL: Duration = Duration::from_secs(3);

/// What the UI can ask the playback worker to do.
pub enum Command {
    TogglePlayback,
    Seek((i64, SeekMode)),
    SeekTo((usize, SeekMode)),
}

/// Map a timeline frame to `(clip index, source frame)`. Clips are `(position,
/// source_start, length)`. Falls back to the last clip's final frame past the end.
fn locate(clips: &[(usize, usize, usize)], tl: usize) -> (usize, usize) {
    for (i, &(position, source_start, length)) in clips.iter().enumerate() {
        if tl >= position && tl < position + length {
            return (i, source_start + (tl - position));
        }
    }
    let i = clips.len().saturating_sub(1);
    let (_position, source_start, length) = clips.get(i).copied().unwrap_or((0, 0, 1));
    (i, source_start + length.saturating_sub(1))
}

pub fn worker() -> impl Stream<Item = Message> {
    iced::stream::channel(64, async |mut output: Sender<Message>| {
        let (command_tx, mut command_rx) = mpsc::channel::<Command>(16);
        output.send(Message::Ready(command_tx)).await.ok();

        // Probe each source for its metadata (cheap, no playback pipeline)
        // and lay them end to end on the timeline.
        let mut sources = Vec::new();
        for path in SOURCES {
            match Source::new(*path) {
                Ok(source) => sources.push(Arc::new(source)),
                Err(e) => eprintln!("could not open {path}: {e}"),
            }
        }
        if sources.is_empty() {
            eprintln!("no sources could be opened");
            return;
        }

        // Lay N clips of the same source, each `CLIP_SECS` long, starting at
        // a random moment in the file. `clip_infos` is (position, source_start,
        // length) in frames — the worker's own copy for cheap mapping.
        let source = sources[0].clone();
        let fps = source.fps;
        let clip_len = ((CLIP_SECS * fps).round() as usize).max(1);
        let max_start = source.frame_count().saturating_sub(clip_len);

        let mut rng = Rng::seeded();
        let mut clip_infos: Vec<(usize, usize, usize)> = Vec::new();
        for i in 0..N_CLIPS {
            let source_start = if max_start > 0 {
                rng.below(max_start)
            } else {
                0
            };
            clip_infos.push((i * clip_len, source_start, clip_len));
        }

        // The same layout as real clips, for the timeline widget.
        let clips = clip_infos
            .iter()
            .map(|&(position, source_start, length)| Clip {
                source: source.clone(),
                position,
                source_start,
                length,
            })
            .collect::<Vec<_>>();
        let timeline = Arc::new(Timeline::single_track(clips));
        output.send(Message::Opened(timeline)).await.ok();

        // One decoder for the shared source: seek it to each clip's in-point,
        // and jump to the next clip when its window runs out (looping at the end).
        let mut player = Player::new();
        let mut frames = match player.open(source) {
            Ok(frames) => frames,
            Err(e) => {
                eprintln!("could not start playback: {e}");
                return;
            }
        };

        let mut current_clip = 0usize;
        // Accurate: the seek's segment starts exactly at the in-point, so
        // gstreamer drops both audio and video from the keyframe up to it —
        // no leading video frames, and no unrelated leading audio.
        player.seek_to_frame(clip_infos[0].1, &mut frames, SeekMode::Accurate);
        player.play();

        let mut in_flight: Option<Instant> = None;
        let mut parked: Option<(usize, SeekMode)> = None;

        loop {
            if in_flight.is_some_and(|at| at.elapsed() > SEEK_TIMEOUT) {
                in_flight = None;
            }

            if in_flight.is_none()
                && let Some((tl_frame, mode)) = parked.take()
            {
                let (clip, source_frame) = locate(&clip_infos, tl_frame);
                current_clip = clip;
                player.seek_to_frame(source_frame, &mut frames, mode);
                in_flight = Some(Instant::now());
            }

            select! {
                cmd = command_rx.select_next_some() => match cmd {
                    Command::TogglePlayback => player.toggle(),
                    Command::Seek((delta, mode)) => {
                        player.seek(delta, &mut frames, mode);
                        in_flight = Some(Instant::now());
                    }
                    Command::SeekTo(seek) => parked = Some(seek),
                },
                frame = frames.select_next_some() => {
                    in_flight = None;

                    let (position, source_start, length) = clip_infos[current_clip];
                    let source_frame = (frame.time.as_secs_f64() * fps).round() as usize;

                    // A KEY_UNIT seek lands on a keyframe at or before the
                    // in-point. Drop the leading frames until we reach it: they
                    // can be corrupt RASL pictures (they reference frames before
                    // the random-access keyframe) and aren't the moment we asked
                    // for. This also makes each clip start frame-accurate.
                    if source_frame < source_start {
                        continue;
                    }

                    // Played the clip's length → jump to the next (looping).
                    if source_frame >= source_start + length {
                        current_clip = (current_clip + 1) % clip_infos.len();
                        player.seek_to_frame(clip_infos[current_clip].1, &mut frames, SeekMode::Accurate);
                        in_flight = Some(Instant::now());
                        continue;
                    }

                    let timeline_frame = position + (source_frame - source_start);
                    output.send(Message::Playhead(timeline_frame)).await.ok();

                    let target = frame.time;
                    let lag_ms = player.position().as_secs_f64() * 1000.0
                        - target.as_secs_f64() * 1000.0;
                    output.send(Message::Lag(lag_ms.round() as i64)).await.ok();

                    if parked.is_none() && target > player.position() {
                        Delay::new(target - player.position()).await;
                    }
                    if output.send(Message::Frame(frame)).await.is_err() { break; }
                }
                // Watchdog: if no frame arrives for a while, the pipeline has
                // likely hit EOS (a clip window reached the end of the file) or
                // a seek stalled. Advance to the next clip to recover instead of
                // hanging forever. During normal playback frames arrive every
                // ~40ms, so this never fires.
                _ = Delay::new(PLAYBACK_STALL).fuse() => {
                    current_clip = (current_clip + 1) % clip_infos.len();
                    player.seek_to_frame(clip_infos[current_clip].1, &mut frames, SeekMode::Fast);
                    in_flight = Some(Instant::now());
                }
                complete => break,
            }
        }
    })
}
