use std::path::PathBuf;
use std::time::Duration;

use cut_timeline::{Edit, Rational};
use futures::executor::block_on;

use super::*;

/// A document of three ten-second clips, with no file behind them: every
/// edit here is arithmetic on measurements, and probing a real video would
/// only make the test slower and machine-dependent.
fn session() -> Arc<State> {
    let source = Arc::new(Source::new(
        PathBuf::from("ten.mp4"),
        Duration::from_secs(10),
        Rational::new(30, 1),
    ));

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
