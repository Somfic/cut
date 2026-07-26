use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::clock::Clock;
use crate::timeline::{Timeline, timeline};
use crate::video::{Frame, SeekMode, Video, VideoView};
use futures_timer::Delay;
use gstreamer::prelude::ElementExtManual;
use gstreamer::{self as gst, State, prelude::ElementExt};
use gstreamer::{Fraction, SeekFlags};
use gstreamer_app::AppSinkCallbacks;
use iced::futures::channel::mpsc::{self, Receiver, Sender};
use iced::futures::{SinkExt, StreamExt, select};
use iced::widget::shader;
use iced::{
    Element, Subscription,
    futures::Stream,
    widget::{button, column, container, row, text},
};

/// How long to wait for a seek to produce a frame before assuming it never will.
const SEEK_TIMEOUT: Duration = Duration::from_millis(500);

mod clock;
mod timeline;
mod video;

#[derive(Default)]
struct Screen {
    timeline: Timeline,
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
    Frame(Arc<Frame>),
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

        let controls = row![
            button(if self.playing { "Pause" } else { "Play" }).on_press(Message::TogglePlayback),
        ]
        .spacing(10);

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
        Subscription::run(Self::video_worker)
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

            let (mut video, mut frames) =
                match Video::new("/Users/lucas/Downloads/The Beauty Of Game of Thrones.mp4") {
                    Ok(v) => v,
                    Err(e) => {
                        eprintln!("video failed: {e}");
                        return;
                    }
                };
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
