use crate::app::Event;
use cut_engine::playback::{Request, SeekMode};
use iced::keyboard::{self, Key as RawKey, Modifiers, key::Named as Key};
use iced::{Subscription, window};

/// Files dragged onto the window.
///
/// winit reports no cursor position with a drop, so where the pointer was is
/// not something we can honour — the app picks the target itself. Wayland does
/// not report drops at all, so this is quiet there and the menu is the way in.
pub fn file_drops() -> Subscription<Event> {
    iced::window::events().filter_map(|(_, event)| match event {
        window::Event::FileHovered(_) => Some(Event::DragOver(true)),
        window::Event::FilesHoveredLeft => Some(Event::DragOver(false)),
        window::Event::FileDropped(path) => Some(Event::Dropped(path)),
        _ => None,
    })
}

pub fn keylogger() -> Subscription<Event> {
    keyboard::listen().filter_map(|event| match event {
        keyboard::Event::KeyPressed { key, modifiers, .. } => Some(Event::Keypress(key, modifiers)),
        _ => None,
    })
}

pub struct Bindings {
    bindings: Vec<Binding>,
}

impl Default for Bindings {
    fn default() -> Self {
        Bindings::new(vec![
            Binding::new(Key::Space, Request::TogglePlayback),
            Binding::new("k", Request::TogglePlayback),
            Binding::new(Key::ArrowLeft, Request::Step((-1, SeekMode::Accurate))),
            Binding::new(Key::ArrowRight, Request::Step((1, SeekMode::Accurate))),
            Binding::new(Key::ArrowLeft, Request::Step((-10, SeekMode::Accurate)))
                .with_modifiers(Modifiers::SHIFT),
            Binding::new(Key::ArrowRight, Request::Step((10, SeekMode::Accurate)))
                .with_modifiers(Modifiers::SHIFT),
            Binding::new(Key::Home, Request::Seek((0, SeekMode::Accurate))),
            Binding::new(Key::Escape, Event::Menu(None)),
            // COMMAND is Cmd on the Mac and Ctrl everywhere else, so one binding
            // is the native one on both machines.
            Binding::new("z", Event::Undo).with_modifiers(Modifiers::COMMAND),
            Binding::new("z", Event::Redo).with_modifiers(Modifiers::COMMAND | Modifiers::SHIFT),
            Binding::new(Key::Delete, Event::DeleteSelection),
            Binding::new(Key::Backspace, Event::DeleteSelection),
            Binding::new("s", Event::SplitSelection),
        ])
    }
}

impl Bindings {
    pub fn new(bindings: Vec<Binding>) -> Self {
        Bindings { bindings }
    }

    pub fn resolve(&self, key: &RawKey, modifiers: Modifiers) -> Vec<Event> {
        self.bindings
            .iter()
            .filter(|binding| binding.matches(key, modifiers))
            .map(|binding| binding.event.clone())
            .collect()
    }
}

pub enum Trigger {
    Named(Key),
    Character(&'static str),
}

impl From<Key> for Trigger {
    fn from(value: Key) -> Self {
        Trigger::Named(value)
    }
}

impl From<&'static str> for Trigger {
    fn from(value: &'static str) -> Self {
        Trigger::Character(value)
    }
}

pub struct Binding {
    trigger: Trigger,
    modifiers: Modifiers,
    event: Event,
}

impl Binding {
    pub fn new(trigger: impl Into<Trigger>, event: impl Into<Event>) -> Self {
        Binding {
            trigger: trigger.into(),
            modifiers: Modifiers::empty(),
            event: event.into(),
        }
    }

    pub fn with_modifiers(self, modifiers: Modifiers) -> Self {
        Self { modifiers, ..self }
    }

    fn matches(&self, key: &RawKey, modifiers: Modifiers) -> bool {
        self.modifiers == modifiers
            && match (&self.trigger, key) {
                (Trigger::Named(expected), RawKey::Named(pressed)) => expected == pressed,
                (Trigger::Character(expected), RawKey::Character(pressed)) => {
                    pressed.eq_ignore_ascii_case(expected)
                }
                _ => false,
            }
    }
}
