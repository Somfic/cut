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
            let (frame_sender, mut frame_receiver) = mpsc::channel::<Arc<Frame>>(4);
            let mut eos_sender = frame_sender.clone();

            let (command_sender, mut command_receiver) = mpsc::channel::<Command>(16);
            output.send(Message::Ready(command_sender)).await.ok();

            let video = match Video::new("/Users/lucas/Downloads/The Beauty Of Game of Thrones.mp4")
            {
                Ok(v) => v,
                Err(e) => {
                    eprintln!("video failed: {e}");
                    return;
                }
            };

            video.sink.set_callbacks(
                AppSinkCallbacks::builder()
                    .new_sample({
                        let mut frame_sender = frame_sender;
                        move |sink| {
                            let sample = sink.pull_sample().map_err(|_| gst::FlowError::Eos)?;
                            let frame: Frame =
                                sample.try_into().map_err(|_| gst::FlowError::Error)?;

                            match frame_sender.try_send(Arc::new(frame)) {
                                Ok(()) => Ok(gst::FlowSuccess::Ok),
                                Err(e) if e.is_full() => Ok(gst::FlowSuccess::Ok),
                                Err(_) => Err(gst::FlowError::Eos),
                            }
                        }
                    })
                    .eos(move |_sink| {
                        eos_sender.close_channel();
                    })
                    .build(),
            );

            let mut clock = Clock::new();
            video.pipeline.set_state(State::Playing).ok();
            clock.resume();

            let mut fps = None;

            loop {
                select! {
                    cmd = command_receiver.select_next_some() => {
                        Self::handle_command(cmd, &video.pipeline, &mut clock, &mut frame_receiver, fps);
                    }
                    frame = frame_receiver.next() => {
                        let Some(frame) = frame else { break }; // eos
                        fps = Some(frame.fps);

                        let target = frame.time;
                        let now = clock.position();
                        if target > now {
                            Delay::new(target - now).await;
                        }
                        if output.send(Message::Frame(frame)).await.is_err() { break; }
                    }
                    complete => break,
                }
            }
        })
    }

    fn handle_command(
        cmd: Command,
        pipeline: &gst::Pipeline,
        clock: &mut Clock,
        frame_receiver: &mut Receiver<Arc<Frame>>,
        fps: Option<Fraction>,
    ) {
        match cmd {
            Command::TogglePlayback => {
                if clock.is_playing() {
                    pipeline.set_state(State::Paused).ok();
                    clock.pause(); // freeze frames AND clock, together
                } else {
                    pipeline.set_state(State::Playing).ok();
                    clock.resume();
                }
            }
            Command::Seek(delta) => {
                let Some(fps) = fps else { return }; // no frame decoded yet
                let fps = fps.numer() as f64 / fps.denom() as f64;
                let current = (clock.position().as_secs_f64() * fps).round() as i64;
                let target_frame = (current + delta).max(0); // clamp at start
                let target = Duration::from_secs_f64(target_frame as f64 / fps);

                let pos = gst::ClockTime::from_nseconds(target.as_nanos() as u64);
                pipeline
                    .seek_simple(SeekFlags::FLUSH | SeekFlags::ACCURATE, pos)
                    .ok();

                while let Ok(_) = frame_receiver.try_recv() {} // drop stale frames
                clock.seek_to(target);
            }
        }
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
