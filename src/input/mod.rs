use crate::app::Event;
use crate::playback::{Request, SeekMode};
use iced::keyboard::{self, Key as RawKey, Modifiers, key::Named as Key};
use iced::{Subscription, mouse};

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
