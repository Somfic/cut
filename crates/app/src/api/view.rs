use draad::{api, ty};
use std::sync::Arc;

use crate::state::State;

/// Where the user is looking: the frame at the left edge, and how much of a
/// pixel a frame is worth.
///
/// Kept with every version in the undo stack, so going back a step goes back
/// to the part of the document that step was about — an undo that leaves you
/// somewhere else entirely is an undo you have to go looking for.
#[ty]
#[derive(Copy, Default, PartialEq)]
pub struct ViewDto {
    pub playhead: usize,
    pub scroll: f64,
    pub zoom: f64,
}

#[api(namespace = "view")]
pub trait ViewApi {
    /// Say where the timeline has come to rest. Sent when a pan or a zoom
    /// settles rather than while it is happening: this is only ever read when
    /// an edit is recorded, so a value per frame would be a value per frame
    /// thrown away.
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

/// The view as it stands, with the playhead read from playback rather than
/// from the front end — it is the one part of this the engine owns.
pub fn current(state: &State) -> ViewDto {
    let marked = *state.session.view.lock().unwrap();

    ViewDto {
        playhead: crate::api::transport::dto(state).playhead,
        ..marked
    }
}
