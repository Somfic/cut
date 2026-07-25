use crate::{decoder::Frame, video::Video};
use gstreamer::{self as gst, State, prelude::ElementExt};
use gstreamer_app::{AppSinkCallbacks, app_sink::AppSinkCallbacksBuilder};
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use iced::{
    Element, Program, Subscription,
    futures::Stream,
    widget::{Column, Row, button, column, container, image, row, text},
};
use std::{thread, time::Duration};

mod decoder;
mod video;

#[derive(Default)]
struct Screen {
    frame: Option<image::Handle>,
}

enum Message {
    Frame(image::Handle),
}

impl Screen {
    pub fn view(&self) -> Element<'_, Message> {
        let content = match &self.frame {
            Some(handle) => Element::from(image(handle.clone())),
            None => Element::from(text("decoding…")),
        };

        container(content)
            .center_x(iced::Fill)
            .center_y(iced::Fill)
            .into()
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Frame(frame) => self.frame = Some(frame),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::run(Self::video_worker)
    }

    fn video_worker() -> impl Stream<Item = Message> {
        iced::stream::channel(64, async |mut output| {
            let (frame_sender, mut frame_receiver) = mpsc::channel::<image::Handle>(4);
            let mut eos_sender = frame_sender.clone();

            let video = match Video::new("/Users/lucas/Downloads/Naamloos.m4v") {
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
                            let image: image::Handle = frame.into();

                            match frame_sender.try_send(image) {
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

            video.pipeline.set_state(State::Playing).unwrap();

            while let Some(handle) = frame_receiver.next().await {
                if output.send(Message::Frame(handle)).await.is_err() {
                    break;
                }
            }
        })
    }
}

fn main() -> anyhow::Result<()> {
    // setup gstreamer
    gstreamer::init()?;

    iced::application(
        move || (Screen { frame: None }, iced::Task::none()),
        Screen::update,
        Screen::view,
    )
    .subscription(Screen::subscription)
    .run()?;

    Ok(())
}
