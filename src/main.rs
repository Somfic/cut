use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::shortcuts::Action;
use crate::timeline::{Timeline, timeline};
use crate::video::{Frame, SeekMode, Video, VideoView};
use futures_timer::Delay;
use iced::futures::channel::mpsc::{self, Sender};
use iced::futures::{SinkExt, StreamExt, select};
use iced::widget::shader;
use iced::{
    Element, Subscription,
    futures::Stream,
    widget::{button, column, container, row, text},
};

const SOURCES: &[&str] = &[
    "/home/lucas/lab/data/cinema/fs/torrents/54f05a5e9c55e90bd2eeb24fadd5726bc51d6295/A Knight of the Seven Kingdoms S01E01 The Hedge Knight 2160p HMAX WEB-DL DDP5 1 H 265-NTb.mkv",
];

/// How long to wait for a seek to produce a frame before assuming it never will.
const SEEK_TIMEOUT: Duration = Duration::from_millis(500);
const LAG_WARN_MS: i64 = 100;

mod clock;
mod shortcuts;
mod timeline;
mod video;

#[derive(Default)]
struct Screen {
    timeline: Arc<Timeline>,
    frame: Option<Arc<Frame>>,
    commands: Option<Sender<Command>>,
    playing: bool,
}

pub enum Command {
    TogglePlayback,
    Seek((i64, SeekMode)),
    SeekTo((usize, SeekMode)),
}

#[derive(Clone)]
pub enum Message {
    Ready(Sender<Command>),
    Opened(Arc<Timeline>),
    Frame(Arc<Frame>),
    Shortcut(Action),
    TogglePlayback,
    Seek((i64, SeekMode)),
    SeekTo((usize, SeekMode)),
}

impl Screen {
    fn fps(&self) -> f64 {
        self.frame
            .as_ref()
            .map(|f| f.fps.numer() as f64 / f.fps.denom() as f64)
            .filter(|fps| *fps > 0.0)
            .unwrap_or(30.0)
    }

    fn playhead(&self) -> usize {
        self.frame
            .as_ref()
            .map(|f| (f.time.as_secs_f64() * self.fps()).round() as usize)
            .unwrap_or(0)
    }
    pub fn view(&self) -> Element<'_, Message> {
        let preview: Element<'_, Message> = match &self.frame {
            Some(frame) => shader(VideoView {
                frame: Some(frame.clone()),
            })
            .width(iced::Fill)
            .height(iced::Fill)
            .into(),
            None => container(text("decoding…"))
                .center_x(iced::Fill)
                .center_y(iced::Fill)
                .into(),
        };

        let lag = self
            .timeline
            .playing_video()
            .map(|v| v.lag_ms())
            .unwrap_or(0);
        let mut controls = row![
            button(if self.playing { "Pause" } else { "Play" }).on_press(Message::TogglePlayback),
        ]
        .spacing(10)
        .align_y(iced::Center);

        // Surface playback falling behind the audio clock — but only while
        // playing, since a paused clock freezes the lag at its last value.
        if self.playing && lag > LAG_WARN_MS {
            controls = controls.push(
                text(format!("⚠ {lag} ms behind")).color(iced::Color::from_rgb(0.9, 0.25, 0.25)),
            );
        }

        let timeline = timeline(&self.timeline, self.playhead(), self.fps())
            .on_seek(|frame| Message::SeekTo((frame, SeekMode::Accurate)));

        column![preview, container(controls).center_x(iced::Fill), timeline,]
            .spacing(10)
            .padding(10)
            .into()
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Ready(tx) => self.commands = Some(tx),
            // The worker starts playback as soon as it has opened the file.
            Message::Opened(timeline) => {
                self.timeline = timeline;
                self.playing = true;
            }
            Message::Shortcut(action) => match action {
                Action::TogglePlayback => self.update(Message::TogglePlayback),
                Action::Step(delta) => self.update(Message::Seek((delta, SeekMode::Accurate))),
                Action::GoToStart => self.update(Message::SeekTo((0, SeekMode::Accurate))),
            },
            Message::Frame(f) => self.frame = Some(f),
            Message::TogglePlayback => {
                self.playing = !self.playing;
                self.send(Command::TogglePlayback);
            }
            Message::Seek((delta, mode)) => self.send(Command::Seek((delta, mode))),
            Message::SeekTo((frame, mode)) => self.send(Command::SeekTo((frame, mode))),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run(Self::video_worker),
            shortcuts::subscription().map(Message::Shortcut),
        ])
    }

    fn send(&mut self, cmd: Command) {
        if let Some(tx) = &mut self.commands {
            let _ = tx.try_send(cmd);
        }
    }

    fn video_worker() -> impl Stream<Item = Message> {
        iced::stream::channel(64, async |mut output| {
            let (command_tx, mut command_rx) = mpsc::channel::<Command>(16);
            output.send(Message::Ready(command_tx)).await.ok();

            // Every source is opened so the timeline knows its length, but only
            // the first one's frames are consumed: playback does not follow the
            // timeline yet, it just plays source zero start to finish.
            let mut sources = Vec::new();
            let mut playback = None;

            for path in SOURCES {
                match Video::new(*path) {
                    Ok((video, frames)) => {
                        let video = Arc::new(video);
                        playback.get_or_insert_with(|| (video.clone(), frames));
                        sources.push(video);
                    }
                    Err(e) => eprintln!("could not open {path}: {e}"),
                }
            }

            let Some((video, mut frames)) = playback else {
                eprintln!("no sources could be opened");
                return;
            };

            // The pipelines have prerolled by now, so every clip's length is known.
            let timeline = Arc::new(Timeline::sequence(sources));
            output.send(Message::Opened(timeline)).await.ok();

            video.play();

            let mut in_flight: Option<Instant> = None;
            let mut parked: Option<(usize, SeekMode)> = None;

            loop {
                if in_flight.is_some_and(|at| at.elapsed() > SEEK_TIMEOUT) {
                    in_flight = None;
                }

                if in_flight.is_none()
                    && let Some((frame, mode)) = parked.take()
                {
                    video.seek_to_frame(frame, &mut frames, mode);
                    in_flight = Some(Instant::now());
                }

                select! {
                    cmd = command_rx.select_next_some() => match cmd {
                        Command::TogglePlayback => video.toggle(),
                        Command::Seek((delta, mode)) => {
                            video.seek(delta, &mut frames, mode);
                            in_flight = Some(Instant::now());
                        }
                        Command::SeekTo(seek) => parked = Some(seek),
                    },
                    frame = frames.select_next_some() => {
                        in_flight = None;

                        let target = frame.time;

                        let lag_ms = video.position().as_secs_f64() * 1000.0
                            - target.as_secs_f64() * 1000.0;
                        video.set_lag(lag_ms.round() as i64);

                        if parked.is_none() && target > video.position() {
                            Delay::new(target - video.position()).await;
                        }
                        if output.send(Message::Frame(frame)).await.is_err() { break; }
                    }
                    complete => break,
                }
            }
        })
    }
}

fn main() -> anyhow::Result<()> {
    // setup gstreamer
    gstreamer::init()?;

    iced::application(
        move || (Screen::default(), iced::Task::none()),
        Screen::update,
        Screen::view,
    )
    .subscription(Screen::subscription)
    .run()?;

    Ok(())
}
