use iced::widget::{Column, Row, button, column, row, text};

use crate::decoder::decode_frame;

mod decoder;

// state
#[derive(Default)]
struct Counter {
    value: i32,
}

// interactions
#[derive(Debug, Clone, Copy)]
pub enum Message {
    Increment,
    Decrement,
}

impl Counter {
    pub fn view(&self) -> Row<'_, Message> {
        row![
            button("+").on_press(Message::Increment),
            text(self.value).size(50),
            button("-").on_press(Message::Decrement)
        ]
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Increment => self.value += 1,
            Message::Decrement => self.value -= 1,
        }
    }
}

fn main() -> anyhow::Result<()> {
    // setup gstreamer
    gstreamer::init()?;

    let frame = decode_frame("/Users/lucas/Downloads/Naamloos.m4v")?;

    println!("{frame:?}");

    iced::run(Counter::update, Counter::view)?;

    Ok(())
}
