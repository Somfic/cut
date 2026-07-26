use std::sync::Arc;
use std::time::{Duration, Instant};

use crate::clock::Clock;
use crate::video::{Frame, Video, VideoView};
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

mod clock;
mod video;

#[derive(Default)]
struct Screen {
    frame: Option<Arc<Frame>>,
    commands: Option<Sender<Command>>,
    playing: bool,
}

pub enum Command {
    TogglePlayback,
    Seek(i64),
}

#[derive(Clone)]
pub enum Message {
    Ready(Sender<Command>),
    Frame(Arc<Frame>),
    TogglePlayback,
    Seek(i64),
}

impl Screen {
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
            button("⏮ -20").on_press(Message::Seek(-20)),
            button(if self.playing { "Pause" } else { "Play" }).on_press(Message::TogglePlayback),
            button("+20 ⏭").on_press(Message::Seek(20)),
        ]
        .spacing(10);

        column![preview, container(controls).center_x(iced::Fill)]
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
            Message::Seek(delta) => self.send(Command::Seek(delta)),
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

            loop {
                select! {
                    cmd = command_rx.select_next_some() => match cmd {
                        Command::TogglePlayback => video.toggle(),
                        Command::Seek(delta)    => video.seek(delta, &mut frames),
                    },
                    frame = frames.select_next_some() => {
                        let target = frame.time;
                        if target > video.position() {
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
