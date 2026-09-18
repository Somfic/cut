use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::Context;
use futures::channel::mpsc::{self, Sender};
use futures::{FutureExt, SinkExt, Stream, StreamExt, select};
use futures_timer::Delay;

use crate::transport::Status;
use crate::{Command, Player, Transport, VideoStream, stream};
use cut_media::{Frame, SeekMode};
use cut_timeline::{Timeline, file};

/// Everything playback tells a front end about. The rest is driven through
/// [`Transport`].
#[derive(Clone)]
pub enum Event {
    /// The handle for driving playback. Arrives once, first.
    Ready(Transport),
    Opened {
        timeline: Arc<Timeline>,
        /// False for a project with no file yet, which is what has autosave
        /// write it out and give it one to begin with.
        on_disk: bool,
    },
    Frame(Arc<Frame>),
}

const SEEK_TIMEOUT: Duration = Duration::from_millis(500);
const PLAYBACK_STALL: Duration = Duration::from_secs(3);

/// One path from "I want this frame" to the decoders: every seek, clip
/// boundary and scrub comes through here.
fn cut(
    player: &mut Player,
    timeline: &Timeline,
    tl_frame: usize,
    stream: &mut VideoStream,
    mode: SeekMode,
) {
    if let Err(e) = player.cut_to(timeline, tl_frame, stream, mode) {
        eprintln!("could not cut to timeline frame {tl_frame}: {e}");
    }
}

/// The project file, or an empty timeline when there is nothing there yet.
fn open(project: &Path) -> anyhow::Result<(Timeline, bool)> {
    if project.exists() {
        let timeline = file::load(project, &|path| cut_media::probe_source(path))
            .with_context(|| format!("could not open {}", project.display()))?;

        Ok((timeline, true))
    } else {
        let timeline = Timeline::default();
        Ok((timeline, false))
    }
}

// `use<>`: the stream owns a clone of the path and captures nothing from the
// borrow, which is what makes this a plain `fn(&D) -> S` pointer.
pub fn run(project: &Path) -> impl Stream<Item = Event> + use<> {
    let project = project.to_path_buf();

    stream::channel(64, async move |mut output: Sender<Event>| {
        let (command_tx, mut command_rx) = mpsc::channel::<Command>(16);
        let (mut player, mut stream) = Player::new();

        // `playing` is the player's own flag, not a copy of it.
        let status = Arc::new(Status::new(player.playing_flag()));
        let transport = Transport::new(command_tx, status.clone());
        output.send(Event::Ready(transport)).await.ok();

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
            // Not fatal: a new project starts here.
            eprintln!("timeline is empty — import some media");
        }

        // Accurate: the segment starts exactly at the in-point, so gstreamer
        // drops the leading video and unrelated audio itself.
        cut(&mut player, &timeline, 0, &mut stream, SeekMode::Accurate);
        player.play();

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
                cut(&mut player, &timeline, tl_frame, &mut stream, mode);
                in_flight = Some(Instant::now());
            }

            select! {
                cmd = command_rx.select_next_some() => match cmd {
                    Command::TogglePlayback => player.toggle(),
                    Command::Pause => player.pause(),
                    Command::Open(opened) => {
                        timeline = opened;
                        length = timeline.length();
                        // Stay where the playhead was, if the new document
                        // still reaches that far.
                        parked = Some((
                            playhead.min(length.saturating_sub(1)),
                            SeekMode::Accurate,
                        ));
                    }
                    // Relative seeks resolve against the last playhead and then
                    // park, so every seek takes the same route through `cut`.
                    Command::Step((delta, mode)) => {
                        parked = Some((
                            playhead
                                .saturating_add_signed(delta as isize)
                                .min(length.saturating_sub(1)),
                            mode,
                        ));
                    }
                    Command::Seek(seek) => parked = Some(seek),
                },
                frame = stream.select_next_some() => {
                    in_flight = None;

                    // Which clip produced this frame — and its fps, since frame
                    // times are in that clip's own source, not the timeline's.
                    let Some((position, source_start, clip_len, fps)) = player
                        .leading_clip()
                        .map(|c| (c.position, c.source_start, c.length, c.source.fps()))
                    else {
                        continue;
                    };
                    let source_frame = (frame.time.as_secs_f64() * fps).round() as usize;

                    // A KEY_UNIT seek lands at or before the in-point: drop
                    // what comes first, which can be corrupt RASL pictures, and
                    // stragglers from the decoder we cut away from.
                    if source_frame < source_start {
                        continue;
                    }

                    if source_frame >= source_start + clip_len {
                        // Played out: on to wherever the next clip begins. Not
                        // the next frame along — an edit can leave a hole with
                        // nothing in it to decode — and it wraps at the end.
                        let next = timeline.next_content(position + clip_len).unwrap_or(0);
                        cut(&mut player, &timeline, next, &mut stream, SeekMode::Accurate);
                        in_flight = Some(Instant::now());
                        continue;
                    }

                    let timeline_frame = position + (source_frame - source_start);
                    playhead = timeline_frame;

                    let target = frame.time;
                    let lag_ms = player.position().as_secs_f64() * 1000.0
                        - target.as_secs_f64() * 1000.0;
                    status.set_lag_ms(lag_ms.round() as i64);

                    if parked.is_none() && target > player.position() {
                        Delay::new(target - player.position()).await;
                    }

                    // After the wait, not before: published early it runs a
                    // frame ahead of the picture it is meant to describe.
                    status.set_playhead(timeline_frame);
                    if output.send(Event::Frame(frame)).await.is_err() { break; }
                }
                // Watchdog: no frame for seconds means EOS or a stalled seek,
                // so move on rather than hang. Never fires while playing.
                _ = Delay::new(PLAYBACK_STALL).fuse() => {
                    let next = player
                        .leading_clip()
                        .and_then(|c| timeline.next_content(c.position + c.length))
                        .unwrap_or(0);
                    cut(&mut player, &timeline, next, &mut stream, SeekMode::Fast);
                    in_flight = Some(Instant::now());
                }
                complete => break,
            }
        }
    })
}
