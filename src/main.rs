use iced::{
    Element,
    widget::{Column, Row, button, column, container, image, row, text},
};

use crate::decoder::decode_frame;

mod decoder;

#[derive(Default)]
struct Screen {
    frame: Option<image::Handle>,
}

enum Message {}

impl Screen {
    pub fn view(&self) -> Element<Message> {
        let content = match &self.frame {
            Some(handle) => Element::from(image(handle.clone())),
            None => Element::from(text("decoding…")),
        };

        container(content)
            .center_x(iced::Fill)
            .center_y(iced::Fill)
            .into()
    }

    pub fn update(&mut self, message: Message) {}
}

fn main() -> anyhow::Result<()> {
    // setup gstreamer
    gstreamer::init()?;

    iced::application(
        move || {
            (
                Screen {
                    frame: Some(
                        decode_frame("/Users/lucas/Downloads/Naamloos.m4v")
                            .unwrap()
                            .into(),
                    ),
                },
                iced::Task::none(),
            )
        },
        Screen::update,
        Screen::view,
    )
    .run()?;

    Ok(())
}
