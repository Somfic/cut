use iced::Subscription;
use iced::keyboard::{self, Key, Modifiers, key::Named};

/// What a key press asks the app to do.
///
/// Deliberately separate from `Message`: the binding table describes intent, and
/// the app decides how to carry it out. That keeps the table readable and makes
/// it the one place to look when a shortcut is wrong.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    TogglePlayback,
    /// Step the playhead by a number of frames, negative for backwards.
    Step(i64),
    GoToStart,
}

/// The key half of a binding. Characters are matched case-insensitively, so
/// `Character("k")` also fires when shift is what produced the `K`.
enum Trigger {
    Named(Named),
    Character(&'static str),
}

pub struct Binding {
    trigger: Trigger,
    /// Matched exactly, so `Cmd+Space` will not trigger a bare `Space` binding.
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

/// Listens for bound key presses. Map the result into your own message type.
///
/// Built on `keyboard::listen`, which only reports presses no widget consumed —
/// so a focused text input keeps its keys instead of triggering shortcuts.
pub fn subscription() -> Subscription<Action> {
    // The closure has to stay non-capturing: iced rejects stateful ones here.
    keyboard::listen().filter_map(|event| match event {
        keyboard::Event::KeyPressed { key, modifiers, .. } => resolve(&key, modifiers),
        _ => None,
    })
}
