use std::sync::Arc;

use cut_engine::media::Source;
use cut_engine::playback::Request;
use cut_engine::project::{ClipId, Edge, Edit, History, Placement, Timeline, TrimTo};
use draad::{api, ty};

use crate::api::view;
// Re-exported, not just imported: the generated commands for this namespace
// glob this module, and undo answers with one of these.
pub use crate::api::view::ViewDto;
use crate::state::{State, Version};

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

/// Where a clip is being put.
#[ty]
pub struct PlacementDto {
    pub clip: u64,
    pub track: usize,
    pub position: usize,
}

/// Which end of a clip is going where.
#[ty]
pub struct TrimDto {
    pub clip: u64,
    pub edge: EdgeDto,
    pub frame: usize,
}

/// Changing the document.
///
/// One method per `Edit`, and every one that acts on clips takes a set of
/// them: a gesture over a selection is one edit, one set of checks and one
/// step on the undo stack, whether it moved one clip or ten.
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

    /// Put clips where they are named. A `track` may be one past the last,
    /// which starts a new one, and whatever is already in the way is shortened
    /// to make room.
    ///
    /// `continuing` says this is another step of the gesture that sent the
    /// last one — a held arrow key repeating — and so belongs in the undo step
    /// already open rather than in one of its own.
    async fn move_clips(&self, clips: Vec<PlacementDto>, continuing: bool) -> Result<(), String>;

    /// Move one end of each clip, leaving the other where it is. A clip
    /// growing into its neighbour shortens it.
    async fn trim(&self, edges: Vec<TrimDto>) -> Result<(), String>;

    /// Cut clips in two at `frame`.
    async fn split(&self, clips: Vec<u64>, frame: usize) -> Result<(), String>;

    /// Remove clips, either leaving the gaps they held or closing them.
    async fn delete(&self, clips: Vec<u64>, ripple: bool) -> Result<(), String>;

    /// Go back one version, answering with where the timeline was when that
    /// version was current. Nothing when there was nowhere to go back to,
    /// which is not an error.
    async fn undo(&self) -> Option<ViewDto>;

    /// Go forward one version, after an undo.
    async fn redo(&self) -> Option<ViewDto>;
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

    async fn move_clips(&self, clips: Vec<PlacementDto>, continuing: bool) -> Result<(), String> {
        change(
            self,
            continuing,
            Edit::Move(
                clips
                    .into_iter()
                    .map(|p| Placement {
                        clip: ClipId::from_raw(p.clip),
                        track: p.track,
                        position: p.position,
                    })
                    .collect(),
            ),
        )
    }

    async fn trim(&self, edges: Vec<TrimDto>) -> Result<(), String> {
        apply(
            self,
            Edit::Trim(
                edges
                    .into_iter()
                    .map(|e| TrimTo {
                        clip: ClipId::from_raw(e.clip),
                        edge: e.edge.into(),
                        frame: e.frame,
                    })
                    .collect(),
            ),
        )
    }

    async fn split(&self, clips: Vec<u64>, frame: usize) -> Result<(), String> {
        apply(
            self,
            Edit::Split {
                clips: ids(clips),
                frame,
            },
        )
    }

    async fn delete(&self, clips: Vec<u64>, ripple: bool) -> Result<(), String> {
        apply(
            self,
            Edit::Delete {
                clips: ids(clips),
                ripple,
            },
        )
    }

    async fn undo(&self) -> Option<ViewDto> {
        step(self, |history, current| history.undo(current))
    }

    async fn redo(&self) -> Option<ViewDto> {
        step(self, |history, current| history.redo(current))
    }
}

fn ids(clips: Vec<u64>) -> Vec<ClipId> {
    clips.into_iter().map(ClipId::from_raw).collect()
}

fn source(path: &str) -> Result<Arc<Source>, String> {
    Source::new(path)
        .map(Arc::new)
        .map_err(|e| format!("could not open {path}: {e:#}"))
}

/// Carry out an edit and publish the result.
///
/// `applied` hands back the document the edit would leave, so a refusal never
/// reaches the session — including the undo stack, which is only recorded once
/// the edit is known to have worked.
fn apply(state: &State, edit: Edit) -> Result<(), String> {
    change(state, false, edit)
}

/// The same, for an edit that may be another step of the gesture before it.
fn change(state: &State, continuing: bool, edit: Edit) -> Result<(), String> {
    let mut slot = state.session.timeline.lock().unwrap();
    let current = slot.clone().ok_or("no document is open")?;

    let next = Arc::new(current.applied(edit).map_err(|e| format!("{e:#}"))?);

    let mut history = state.session.history.lock().unwrap();
    if !(continuing && history.amend()) {
        history.record(Version {
            timeline: current,
            view: view::current(state),
        });
    }
    drop(history);
    *slot = Some(next.clone());
    drop(slot);

    publish(state, next);
    Ok(())
}

/// Undo and redo differ only in which end of the history they take from.
///
/// Hands back the view recorded with the version, for the front end to return
/// to, or nothing when there was no step to take.
fn step(
    state: &State,
    take: impl FnOnce(&mut History<Version>, Version) -> Option<Version>,
) -> Option<ViewDto> {
    let mut slot = state.session.timeline.lock().unwrap();
    let timeline = slot.clone()?;

    let here = Version {
        timeline,
        view: view::current(state),
    };
    let there = take(&mut state.session.history.lock().unwrap(), here)?;

    *slot = Some(there.timeline.clone());
    *state.session.view.lock().unwrap() = there.view;
    drop(slot);

    publish(state, there.timeline);
    Some(there.view)
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

        block_on(state.split(vec![first(&state)], 150)).unwrap();
        assert_eq!(clips(&state)[..2], [(0, 150), (150, 150)]);

        assert!(block_on(state.undo()).is_some());
        assert_eq!(clips(&state), before);

        assert!(block_on(state.redo()).is_some());
        assert_eq!(clips(&state)[..2], [(0, 150), (150, 150)]);
    }

    #[test]
    fn a_ripple_delete_closes_the_gap() {
        let state = session();

        block_on(state.delete(vec![first(&state)], true)).unwrap();
        assert_eq!(clips(&state), [(0, 300), (300, 300)]);

        assert!(block_on(state.undo()).is_some());
        assert_eq!(clips(&state), [(0, 300), (300, 300), (600, 300)]);
    }

    #[test]
    fn a_refused_edit_changes_nothing() {
        let state = session();
        let before = clips(&state);

        // Past the end of a ten-second source: the engine refuses it.
        let refused = block_on(state.trim(vec![TrimDto {
            clip: first(&state),
            edge: EdgeDto::Out,
            frame: 5_000,
        }]));
        assert!(refused.is_err());
        assert_eq!(clips(&state), before);

        // And nothing was recorded, so there is nothing to undo back to.
        assert!(block_on(state.undo()).is_none());
    }

    #[test]
    fn undo_on_an_untouched_document_says_so() {
        assert!(block_on(session().undo()).is_none());
    }
}
