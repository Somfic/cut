use iced::widget::{Column, button, column, text};

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
    pub fn view(&self) -> Column<'_, Message> {
        column![
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

fn main() -> iced::Result {
    iced::run(Counter::update, Counter::view)
}
