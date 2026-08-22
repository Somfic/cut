//! One menu panel, used by the bar along the top and by a right-click.
//!
//! Both are a list of labelled things to do, so they are the same widget put in
//! different places: the bar hangs one under a title, a right-click drops one
//! wherever the pointer was.

use iced::widget::{Space, button, column, container, row, text};
use iced::{Element, Fill, Point, Theme, border};

pub(super) const ITEM_SIZE: f32 = 13.0;
pub(super) const CORNER: f32 = 5.0;
const WIDTH: f32 = 220.0;

pub struct Menu<'a, Message> {
    items: Vec<(&'a str, Message)>,
}

impl<'a, Message: Clone + 'a> Menu<'a, Message> {
    pub fn new(items: Vec<(&'a str, Message)>) -> Self {
        Menu { items }
    }

    /// The panel on its own, for a caller that knows where to put it.
    pub(super) fn panel(self) -> Element<'a, Message> {
        let mut items = column![].width(Fill);

        for (label, message) in self.items {
            items = items.push(
                button(text(label).size(ITEM_SIZE).width(Fill))
                    .padding([4.0, 8.0])
                    .style(item_style)
                    .on_press(message),
            );
        }

        container(items)
            .width(WIDTH)
            .padding(4)
            .style(panel_style)
            .into()
    }

    /// The panel at a point in the window — a right-click knows exactly where
    /// it happened, so this one does not have to estimate.
    pub fn at(self, point: Point) -> Element<'a, Message> {
        column![
            Space::new().height(point.y),
            row![Space::new().width(point.x), self.panel()]
        ]
        .into()
    }
}

pub(super) fn panel_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(palette.background.weakest.color.into()),
        border: border::rounded(CORNER)
            .color(palette.background.strong.color)
            .width(1),
        ..container::Style::default()
    }
}

fn item_style(theme: &Theme, status: button::Status) -> button::Style {
    let palette = theme.extended_palette();

    match status {
        button::Status::Hovered | button::Status::Pressed => button::Style {
            background: Some(palette.primary.base.color.into()),
            text_color: palette.primary.base.text,
            border: border::rounded(CORNER - 1.0),
            ..button::Style::default()
        },
        _ => button::Style {
            border: border::rounded(CORNER - 1.0),
            ..button::text(theme, status)
        },
    }
}
