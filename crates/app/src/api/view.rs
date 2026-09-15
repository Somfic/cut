use draad::{api, ty};
use std::sync::Arc;

use crate::state::State;

/// Where the user is looking, kept with every version in the undo stack: an
/// undo that leaves you somewhere else is one you have to go looking for.
#[ty]
#[derive(Copy, Default, PartialEq)]
pub struct ViewDto {
    pub playhead: usize,
    pub scroll: f64,
    pub zoom: f64,
}

#[api(namespace = "view")]
pub trait ViewApi {
    /// Sent when a pan or zoom settles: this is read only when an edit is
    /// recorded, so a value per frame would be one thrown away per frame.
    async fn mark(&self, scroll: f64, zoom: f64);
}

#[api]
impl ViewApi for Arc<State> {
    async fn mark(&self, scroll: f64, zoom: f64) {
        let mut view = self.session.view.lock().unwrap();
        view.scroll = scroll;
        view.zoom = zoom;
    }
}

/// The playhead comes from playback, the one part of this the engine owns.
pub fn current(state: &State) -> ViewDto {
    let marked = *state.session.view.lock().unwrap();

    ViewDto {
        playhead: crate::api::transport::dto(state).playhead,
        ..marked
    }
}
