use std::sync::Arc;

use cut_engine::media::Source;
use cut_engine::playback::Request;
use cut_engine::project::{ClipId, Edge, Edit, Timeline};
use draad::{api, ty};

use crate::state::State;

/// Which end of a clip a trim moves.
#[ty]
#[serde(rename_all = "lowercase")]
pub enum EdgeDto {
    In,
    Out,
}

impl From<EdgeDto> for Edge {
    fn from(edge: EdgeDto) -> Self {
        match edge {
            EdgeDto::In => Edge::In,
            EdgeDto::Out => Edge::Out,
        }
    }
}

/// Changing the document.
///
/// One method per `Edit` rather than one method taking a serialised `Edit`:
/// the wire types are unit-only enums, and a flattened struct with every
/// variant's fields made optional would move the "which fields go together"
/// question from the type system to a runtime check on both sides.
///
/// `Edit::Place` is deliberately absent — it exists for loading and pasting,
/// which name a source window the user never types, and neither goes through
/// the front end.
#[api(namespace = "edit")]
pub trait EditApi {
    /// Probe a file and put the whole of it after everything already there.
    async fn append(&self, path: String) -> Result<(), String>;

    /// Probe a file and ripple it in at `frame`, snapped to the nearest clip
    /// edge, pushing what follows later.
    async fn insert(&self, path: String, frame: usize) -> Result<(), String>;

    /// Take a clip somewhere else. `track` may be one past the last, which
    /// starts a new one.
    async fn move_clip(&self, clip: u64, track: usize, position: usize) -> Result<(), String>;

    /// Move one end of a clip to `frame`, leaving the other where it is.
    async fn trim(&self, clip: u64, edge: EdgeDto, frame: usize) -> Result<(), String>;

    /// Move several clips by the same offset, as one edit — one undo step,
    /// and one set of checks, so a group that shuffles inside its own span
    /// is not refused by the room it is about to vacate.
    /// `overwrite` shortens whatever the group lands on to make room, rather
    /// than refusing the move.
    async fn nudge(
        &self,
        clips: Vec<u64>,
        frames: i64,
        tracks: i64,
        overwrite: bool,
    ) -> Result<(), String>;

    /// Move the same end of several clips by the same offset, as one edit.
    async fn stretch(&self, clips: Vec<u64>, edge: EdgeDto, frames: i64) -> Result<(), String>;

    /// Cut a clip in two at `frame`.
    async fn split(&self, clip: u64, frame: usize) -> Result<(), String>;

    /// Remove a clip, either leaving the gap it held or closing it.
    async fn delete(&self, clip: u64, ripple: bool) -> Result<(), String>;

    /// Go back one version. False when there was nothing to go back to, which
    /// is what greys the menu item out rather than an error.
    async fn undo(&self) -> bool;

    /// Go forward one version, after an undo.
    async fn redo(&self) -> bool;
}

#[api]
impl EditApi for Arc<State> {
    async fn append(&self, path: String) -> Result<(), String> {
        apply(self, Edit::Append(source(&path)?))
    }

    async fn insert(&self, path: String, frame: usize) -> Result<(), String> {
        apply(
            self,
            Edit::Insert {
                source: source(&path)?,
                frame,
            },
        )
    }

    async fn move_clip(&self, clip: u64, track: usize, position: usize) -> Result<(), String> {
        apply(
            self,
            Edit::Move {
                clip: ClipId::from_raw(clip),
                track,
                position,
            },
        )
    }

    async fn trim(&self, clip: u64, edge: EdgeDto, frame: usize) -> Result<(), String> {
        apply(
            self,
            Edit::Trim {
                clip: ClipId::from_raw(clip),
                edge: edge.into(),
                frame,
            },
        )
    }

    async fn nudge(
        &self,
        clips: Vec<u64>,
        frames: i64,
        tracks: i64,
        overwrite: bool,
    ) -> Result<(), String> {
        apply(
            self,
            Edit::Nudge {
                clips: clips.into_iter().map(ClipId::from_raw).collect(),
                frames: frames as isize,
                tracks: tracks as isize,
                overwrite,
            },
        )
    }

    async fn stretch(&self, clips: Vec<u64>, edge: EdgeDto, frames: i64) -> Result<(), String> {
        apply(
            self,
            Edit::Stretch {
                clips: clips.into_iter().map(ClipId::from_raw).collect(),
                edge: edge.into(),
                frames: frames as isize,
            },
        )
    }

    async fn split(&self, clip: u64, frame: usize) -> Result<(), String> {
        apply(
            self,
            Edit::Split {
                clip: ClipId::from_raw(clip),
                frame,
            },
        )
    }

    async fn delete(&self, clip: u64, ripple: bool) -> Result<(), String> {
        apply(
            self,
            Edit::Delete {
                clip: ClipId::from_raw(clip),
                ripple,
            },
        )
    }

    async fn undo(&self) -> bool {
        step(self, |history, current| history.undo(current))
    }

    async fn redo(&self) -> bool {
        step(self, |history, current| history.redo(current))
    }
}

fn source(path: &str) -> Result<Arc<Source>, String> {
    Source::new(path)
        .map(Arc::new)
        .map_err(|e| format!("could not open {path}: {e:#}"))
}

/// Carry out an edit on a copy of the document, and publish the result.
///
/// The copy is the point: `Timeline::apply` refuses an edit that would break
/// an invariant, and a refusal has to leave what the user is looking at
/// exactly as it was — including the undo stack, which is only recorded once
/// the edit is known to have worked.
fn apply(state: &State, edit: Edit) -> Result<(), String> {
    let mut slot = state.session.timeline.lock().unwrap();
    let current = slot.clone().ok_or("no document is open")?;

    let mut next = (*current).clone();
    next.apply(edit).map_err(|e| format!("{e:#}"))?;
    let next = Arc::new(next);

    state.session.history.lock().unwrap().record(current);
    *slot = Some(next.clone());
    drop(slot);

    publish(state, next);
    Ok(())
}

/// Undo and redo differ only in which end of the history they take from.
fn step(
    state: &State,
    take: impl FnOnce(&mut cut_engine::project::History, Arc<Timeline>) -> Option<Arc<Timeline>>,
) -> bool {
    let mut slot = state.session.timeline.lock().unwrap();
    let Some(current) = slot.clone() else {
        return false;
    };

    let Some(version) = take(&mut state.session.history.lock().unwrap(), current) else {
        return false;
    };

    *slot = Some(version.clone());
    drop(slot);

    publish(state, version);
    true
}

/// Tell both halves that the document moved: the canvas so it redraws, and
/// playback so the frames keep matching what is on screen.
///
/// The timeline lock is released before this runs — playback's queue is the
/// one place the two locks could otherwise be taken in the opposite order.
fn publish(state: &State, timeline: Arc<Timeline>) {
    if let Some(events) = state.events.get() {
        events
            .timeline
            .emit_changed(&crate::api::timeline::dto(&timeline));
    }

    if let Some(controls) = state.session.controls.lock().unwrap().as_mut() {
        controls.send(Request::Open(timeline));
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::time::Duration;

    use cut_engine::project::Edit;
    use futures::executor::block_on;
    use gstreamer::Fraction;

    use super::*;

    /// A document of three ten-second clips, with no file behind them: every
    /// edit here is arithmetic on measurements, and probing a real video would
    /// only make the test slower and machine-dependent.
    fn session() -> Arc<State> {
        let source = Arc::new(Source {
            path: PathBuf::from("ten.mp4"),
            duration: Duration::from_secs(10),
            frame_rate: Fraction::new(30, 1),
        });

        let mut timeline = Timeline::default();
        for i in 0..3 {
            timeline
                .apply(Edit::Place {
                    track: 0,
                    source: source.clone(),
                    position: i * 300,
                    source_start: 0,
                    length: 300,
                })
                .unwrap();
        }

        let state = Arc::new(State::default());
        *state.session.timeline.lock().unwrap() = Some(Arc::new(timeline));
        state
    }

    fn clips(state: &State) -> Vec<(usize, usize)> {
        let timeline = state.session.timeline.lock().unwrap().clone().unwrap();
        timeline.tracks[0]
            .clips
            .iter()
            .map(|c| (c.position, c.length))
            .collect()
    }

    fn first(state: &State) -> u64 {
        let timeline = state.session.timeline.lock().unwrap().clone().unwrap();
        timeline.tracks[0].clips[0].id.raw()
    }

    #[test]
    fn split_then_undo_restores_the_document() {
        let state = session();
        let before = clips(&state);

        block_on(state.split(first(&state), 150)).unwrap();
        assert_eq!(clips(&state)[..2], [(0, 150), (150, 150)]);

        assert!(block_on(state.undo()));
        assert_eq!(clips(&state), before);

        assert!(block_on(state.redo()));
        assert_eq!(clips(&state)[..2], [(0, 150), (150, 150)]);
    }

    #[test]
    fn a_ripple_delete_closes_the_gap() {
        let state = session();

        block_on(state.delete(first(&state), true)).unwrap();
        assert_eq!(clips(&state), [(0, 300), (300, 300)]);

        assert!(block_on(state.undo()));
        assert_eq!(clips(&state), [(0, 300), (300, 300), (600, 300)]);
    }

    #[test]
    fn a_refused_edit_changes_nothing() {
        let state = session();
        let before = clips(&state);

        // Past the end of a ten-second source: `Timeline::apply` refuses it.
        let refused = block_on(state.trim(first(&state), EdgeDto::Out, 5_000));
        assert!(refused.is_err());
        assert_eq!(clips(&state), before);

        // And nothing was recorded, so there is nothing to undo back to.
        assert!(!block_on(state.undo()));
    }

    #[test]
    fn undo_on_an_untouched_document_says_so() {
        assert!(!block_on(session().undo()));
    }
}
