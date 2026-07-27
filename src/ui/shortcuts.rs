use iced::Subscription;
use iced::keyboard::{self, Key, Modifiers, key::Named};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TogglePlayback,
    Step(i64),
    GoToStart,
}

enum Trigger {
    Named(Named),
    Character(&'static str),
}

pub struct Binding {
    trigger: Trigger,
    modifiers: Modifiers,
    action: Action,
}

const BINDINGS: &[Binding] = &[
    Binding {
        trigger: Trigger::Named(Named::Space),
        modifiers: Modifiers::empty(),
        action: Action::TogglePlayback,
    },
    Binding {
        trigger: Trigger::Character("k"),
        modifiers: Modifiers::empty(),
        action: Action::TogglePlayback,
    },
    Binding {
        trigger: Trigger::Named(Named::ArrowLeft),
        modifiers: Modifiers::empty(),
        action: Action::Step(-1),
    },
    Binding {
        trigger: Trigger::Named(Named::ArrowRight),
        modifiers: Modifiers::empty(),
        action: Action::Step(1),
    },
    Binding {
        trigger: Trigger::Named(Named::ArrowLeft),
        modifiers: Modifiers::SHIFT,
        action: Action::Step(-10),
    },
    Binding {
        trigger: Trigger::Named(Named::ArrowRight),
        modifiers: Modifiers::SHIFT,
        action: Action::Step(10),
    },
    Binding {
        trigger: Trigger::Named(Named::Home),
        modifiers: Modifiers::empty(),
        action: Action::GoToStart,
    },
];

fn resolve(key: &Key, modifiers: Modifiers) -> Option<Action> {
    BINDINGS
        .iter()
        .find(|binding| {
            binding.modifiers == modifiers
                && match (&binding.trigger, key) {
                    (Trigger::Named(expected), Key::Named(pressed)) => expected == pressed,
                    (Trigger::Character(expected), Key::Character(pressed)) => {
                        pressed.eq_ignore_ascii_case(expected)
                    }
                    _ => false,
                }
        })
        .map(|binding| binding.action)
}

pub fn subscription() -> Subscription<Action> {
    keyboard::listen().filter_map(|event| match event {
        keyboard::Event::KeyPressed { key, modifiers, .. } => resolve(&key, modifiers),
        _ => None,
    })
}
