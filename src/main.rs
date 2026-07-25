use std::sync::Arc;

use crate::video::{Frame, Video, VideoView};
use gstreamer::{self as gst, State, prelude::ElementExt};
use gstreamer_app::AppSinkCallbacks;
use iced::futures::channel::mpsc;
use iced::futures::{SinkExt, StreamExt};
use iced::widget::shader;
use iced::{
    Element, Subscription,
    futures::Stream,
    widget::{container, text},
};

mod video;

#[derive(Default)]
struct Screen {
    frame: Option<Arc<Frame>>,
}

enum Message {
    Frame(Arc<Frame>),
}

impl Screen {
    pub fn view(&self) -> Element<'_, Message> {
        match &self.frame {
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
        }
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
            let (frame_sender, mut frame_receiver) = mpsc::channel::<Arc<Frame>>(4);
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
