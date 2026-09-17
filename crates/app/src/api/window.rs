use draad::{api, events, ty};
use std::sync::Arc;

use crate::state::State;

/// What the window is doing
#[ty]
#[derive(Copy, Default, PartialEq)]
pub struct WindowDto {
    /// macOS hides the traffic lights in fullscreen, so the strip the page
    /// keeps clear for them is only worth keeping clear when it is not.
    pub fullscreen: bool,
}

#[api(namespace = "window")]
pub trait WindowApi {
    /// How the window is now, for the first paint. Changes arrive as events.
    async fn state(&self) -> WindowDto;
}

#[api]
impl WindowApi for Arc<State> {
    async fn state(&self) -> WindowDto {
        *self.window.lock().unwrap()
    }
}

#[events(namespace = "window")]
pub trait WindowEvents {
    fn changed(payload: WindowDto);
}

/// Publish the window's state, if it is not the one already published.
pub fn publish(state: &State, window: WindowDto) {
    let mut last = state.window.lock().unwrap();
    if *last == window {
        return;
    }

    *last = window;
    drop(last);

    state.emit(|events| events.window.emit_changed(&window));
}
