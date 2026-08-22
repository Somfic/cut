//! The menu bar: a strip of titles along the top, one open at a time, with its
//! items dropped underneath.
//!
//! The panel floats over the page in a `stack` instead of sitting in the
//! layout, so opening a menu moves nothing. iced only lets a widget place a
//! real overlay from inside a `Widget` impl, so the panel's x position is
//! estimated from the width of the titles to its left — a couple of pixels
//! either way don't show on a dropdown, and this stays a composition of
//! ordinary widgets.

use iced::widget::{Space, button, column, container, mouse_area, row, stack, text};
use iced::{Center, Element, Fill, Theme, border};

use super::menu::{self, Menu};

/// Height of the strip, and so where a panel starts.
const BAR_HEIGHT: f32 = 28.0;
const TITLE_SIZE: f32 = menu::ITEM_SIZE;
const TITLE_PADDING: f32 = 10.0;
/// Rough advance width of the default font at `TITLE_SIZE`, for lining a panel
/// up under its title.
const TITLE_CHAR_WIDTH: f32 = 7.0;
const CORNER: f32 = menu::CORNER;

pub struct MenuBar<'a, Message> {
    menus: Vec<Titled<'a, Message>>,
    open: Option<usize>,
    on_open: Box<dyn Fn(Option<usize>) -> Message + 'a>,
}

/// One title on the bar and what hangs under it.
struct Titled<'a, Message> {
    title: &'a str,
    items: Vec<(&'a str, Message)>,
}

impl<'a, Message: Clone + 'a> MenuBar<'a, Message> {
    /// `on_open` is told which menu should be showing, or `None` to close. The
    /// bar holds no state of its own: which menu is open belongs to the app,
    /// where a keypress or a finished action can also close it.
    pub fn new(on_open: impl Fn(Option<usize>) -> Message + 'a) -> Self {
        MenuBar {
            menus: Vec::new(),
            open: None,
            on_open: Box::new(on_open),
        }
    }

    pub fn menu(mut self, title: &'a str, items: Vec<(&'a str, Message)>) -> Self {
        self.menus.push(Titled { title, items });
        self
    }

    pub fn open(mut self, open: Option<usize>) -> Self {
        self.open = open;
        self
    }

    /// Put the bar above `page`, and any open menu over it.
    pub fn view(self, page: impl Into<Element<'a, Message>>) -> Element<'a, Message> {
        let mut titles = row![].align_y(Center);

        for (index, menu) in self.menus.iter().enumerate() {
            let open = self.open == Some(index);

            let title = button(text(menu.title).size(TITLE_SIZE))
                .padding([4.0, TITLE_PADDING])
                .style(move |theme, status| title_style(theme, status, open))
                .on_press((self.on_open)(if open { None } else { Some(index) }));

            // With a menu already open, sliding along the bar switches to
            // whichever title the pointer reaches, the way a menu bar behaves.
            titles = titles.push(match self.open {
                Some(_) if !open => Element::from(
                    mouse_area(title).on_enter((self.on_open)(Some(index))),
                ),
                _ => Element::from(title),
            });
        }

        let bar = container(titles)
            .width(Fill)
            .height(BAR_HEIGHT)
            .style(bar_style);

        // A click anywhere else dismisses the menu, and goes no further — the
        // same as clicking away from an open menu anywhere else.
        let page = match self.open {
            Some(_) => Element::from(mouse_area(page).on_press((self.on_open)(None))),
            None => page.into(),
        };

        let Some(index) = self.open else {
            return column![bar, page].into();
        };

        let items = self.menus[index]
            .items
            .iter()
            .map(|(label, message)| (*label, message.clone()))
            .collect();

        let panel = column![
            Space::new().height(BAR_HEIGHT),
            row![Space::new().width(self.offset(index)), Menu::new(items).panel()]
        ];

        stack![column![bar, page], panel].into()
    }

    /// Where the panel for `index` should start: the width of every title to
    /// its left.
    fn offset(&self, index: usize) -> f32 {
        self.menus[..index]
            .iter()
            .map(|menu| menu.title.chars().count() as f32 * TITLE_CHAR_WIDTH + 2.0 * TITLE_PADDING)
            .sum()
    }
}

fn bar_style(theme: &Theme) -> container::Style {
    let palette = theme.extended_palette();

    container::Style {
        background: Some(palette.background.weak.color.into()),
        ..container::Style::default()
    }
}

fn title_style(theme: &Theme, status: button::Status, open: bool) -> button::Style {
    let palette = theme.extended_palette();
    let lit = open || matches!(status, button::Status::Hovered | button::Status::Pressed);

    button::Style {
        background: lit.then(|| palette.background.strong.color.into()),
        border: border::rounded(CORNER),
        ..button::text(theme, status)
    }
}

