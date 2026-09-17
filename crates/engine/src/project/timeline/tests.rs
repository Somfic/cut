use std::path::PathBuf;
use std::time::Duration;

use super::*;
use crate::project::TrimTo;

/// A source is plain measurements, so a test can state them outright
/// instead of encoding a file to be probed. 30 fps, so seconds are frames
/// over thirty.
fn source(seconds: u64) -> Arc<Source> {
    Arc::new(Source {
        path: PathBuf::from(format!("{seconds}s.mp4")),
        duration: Duration::from_secs(seconds),
        frame_rate: Fraction::new(30, 1),
    })
}

/// `(position, source_start, length)` per clip on a track.
fn windows(timeline: &Timeline, track: usize) -> Vec<(usize, usize, usize)> {
    timeline.tracks[track]
        .clips
        .iter()
        .map(|clip| (clip.position, clip.source_start, clip.length))
        .collect()
}

fn ids(timeline: &Timeline) -> Vec<ClipId> {
    timeline.tracks[0]
        .clips
        .iter()
        .map(|clip| clip.id)
        .collect()
}

/// The one-clip forms, which most tests want and every edit now spells
/// as a set.
fn moved(clip: ClipId, track: usize, position: usize) -> Edit {
    Edit::Move(vec![Placement {
        clip,
        track,
        position,
    }])
}

fn trimmed(clip: ClipId, edge: Edge, frame: usize) -> Edit {
    Edit::Trim(vec![TrimTo { clip, edge, frame }])
}

fn cut(clip: ClipId, frame: usize) -> Edit {
    Edit::Split {
        clips: vec![clip],
        frame,
    }
}

fn deleted(clip: ClipId, ripple: bool) -> Edit {
    Edit::Delete {
        clips: vec![clip],
        ripple,
    }
}

/// Two clips, 60 frames then 30, back to back from frame zero.
fn pair() -> Timeline {
    let mut timeline = Timeline::default();
    timeline.apply(Edit::Append(source(2))).unwrap();
    timeline.apply(Edit::Append(source(1))).unwrap();

    timeline
}

#[test]
fn a_group_moves_into_its_own_old_span() {
    let mut timeline = pair();
    let group = ids(&timeline);

    // The first clip is put exactly where the second one still is. One
    // clip at a time could not do this; lifting both out first can.
    timeline
        .apply(Edit::Move(vec![
            Placement {
                clip: group[0],
                track: 0,
                position: 30,
            },
            Placement {
                clip: group[1],
                track: 0,
                position: 90,
            },
        ]))
        .unwrap();

    assert_eq!(windows(&timeline, 0), vec![(30, 0, 60), (90, 0, 30)]);
}

#[test]
fn a_group_move_overwrites_what_it_lands_on() {
    let mut timeline = pair();
    // A third clip at 90..120 that is not part of the group.
    timeline.apply(Edit::Append(source(1))).unwrap();
    let group = ids(&timeline)[..2].to_vec();

    timeline
        .apply(Edit::Move(vec![
            Placement {
                clip: group[0],
                track: 0,
                position: 30,
            },
            Placement {
                clip: group[1],
                track: 0,
                position: 90,
            },
        ]))
        .unwrap();

    // The second clip lands exactly over the third, which is covered end
    // to end and goes.
    assert_eq!(windows(&timeline, 0), vec![(30, 0, 60), (90, 0, 30)]);
    assert_eq!(ids(&timeline), group);
}

#[test]
fn a_group_moves_down_a_track() {
    let mut timeline = pair();
    let group = ids(&timeline);

    timeline
        .apply(Edit::Move(vec![
            Placement {
                clip: group[0],
                track: 1,
                position: 0,
            },
            Placement {
                clip: group[1],
                track: 1,
                position: 60,
            },
        ]))
        .unwrap();

    assert!(timeline.tracks[0].clips.is_empty());
    assert_eq!(windows(&timeline, 1), vec![(0, 0, 60), (60, 0, 30)]);
}

#[test]
fn a_group_trim_moves_every_out_point() {
    let mut timeline = Timeline::default();
    // Spaced out, so shortening one does not depend on the other.
    timeline.apply(Edit::Append(source(2))).unwrap();
    timeline
        .apply(Edit::Place {
            track: 0,
            source: source(2),
            position: 120,
            source_start: 0,
            length: 60,
        })
        .unwrap();
    let group = ids(&timeline);

    timeline
        .apply(Edit::Trim(vec![
            TrimTo {
                clip: group[0],
                edge: Edge::Out,
                frame: 50,
            },
            TrimTo {
                clip: group[1],
                edge: Edge::Out,
                frame: 170,
            },
        ]))
        .unwrap();

    assert_eq!(windows(&timeline, 0), vec![(0, 0, 50), (120, 0, 50)]);
}

#[test]
fn a_group_trim_one_clip_cannot_do_changes_nothing() {
    let timeline = pair();
    let group = ids(&timeline);
    let before = windows(&timeline, 0);

    // The 60-frame source has nothing past its end, so the first clip
    // cannot grow — and the second must not grow on its own either.
    let refused = timeline.applied(Edit::Trim(vec![
        TrimTo {
            clip: group[0],
            edge: Edge::Out,
            frame: 70,
        },
        TrimTo {
            clip: group[1],
            edge: Edge::Out,
            frame: 100,
        },
    ]));

    assert!(refused.is_err());
    assert_eq!(windows(&timeline, 0), before);
}

#[test]
fn a_ripple_trim_of_the_head_keeps_the_clip_where_it_was() {
    let mut timeline = pair();
    let first = ids(&timeline)[0];

    // The playhead is 20 frames into a clip running 0..60: those frames go,
    // and the clip stays where it started.
    timeline
        .apply(Edit::Ripple {
            clip: first,
            edge: Edge::In,
            frame: 20,
        })
        .unwrap();

    // 40 frames of source starting 20 in, and the clip after it moved back.
    assert_eq!(windows(&timeline, 0), vec![(0, 20, 40), (40, 0, 30)]);
    assert_eq!(timeline.length(), 70);
}

#[test]
fn a_ripple_trim_of_the_tail_pulls_what_follows_back() {
    let mut timeline = pair();
    let first = ids(&timeline)[0];

    timeline
        .apply(Edit::Ripple {
            clip: first,
            edge: Edge::Out,
            frame: 40,
        })
        .unwrap();

    assert_eq!(windows(&timeline, 0), vec![(0, 0, 40), (40, 0, 30)]);
    assert_eq!(timeline.length(), 70);
}

#[test]
fn a_ripple_trim_needs_the_frame_inside_the_clip() {
    let timeline = pair();
    let first = ids(&timeline)[0];
    let before = windows(&timeline, 0);

    for frame in [0, 60, 200] {
        assert!(
            timeline
                .applied(Edit::Ripple {
                    clip: first,
                    edge: Edge::In,
                    frame,
                })
                .is_err()
        );
    }

    assert_eq!(windows(&timeline, 0), before);
}

#[test]
fn a_trim_can_grow_into_its_neighbour() {
    let mut timeline = Timeline::default();
    // A 60-frame window onto a 120-frame source, with a clip hard against
    // its end: the frames to grow into exist, the room does not.
    timeline
        .apply(Edit::Place {
            track: 0,
            source: source(4),
            position: 0,
            source_start: 0,
            length: 60,
        })
        .unwrap();
    timeline.apply(Edit::Append(source(1))).unwrap();
    let (first, second) = (ids(&timeline)[0], ids(&timeline)[1]);

    timeline.apply(trimmed(first, Edge::Out, 75)).unwrap();

    // The first clip took fifteen frames from the second, which keeps the
    // rest — and its own window into the source moves with its in-point.
    assert_eq!(windows(&timeline, 0), vec![(0, 0, 75), (75, 15, 15)]);
    assert_eq!(ids(&timeline), vec![first, second]);
}

#[test]
fn overwriting_trims_what_it_lands_on() {
    let mut timeline = pair();
    let second = ids(&timeline)[1];

    // The 30-frame clip lands halfway into the 60-frame one, which keeps
    // only the frames before it.
    timeline.apply(moved(second, 0, 30)).unwrap();

    assert_eq!(windows(&timeline, 0), vec![(0, 0, 30), (30, 0, 30)]);
}

#[test]
fn overwriting_down_the_middle_leaves_a_piece_either_side() {
    let mut timeline = Timeline::default();
    timeline.apply(Edit::Append(source(4))).unwrap();
    timeline
        .apply(Edit::Place {
            track: 0,
            source: source(1),
            position: 200,
            source_start: 0,
            length: 30,
        })
        .unwrap();
    let dropped = ids(&timeline)[1];

    // 30 frames dropped at 45, inside a clip running 0..120.
    timeline.apply(moved(dropped, 0, 45)).unwrap();

    // The halves keep their own windows into the source: 0..45, then the
    // frames from 75 on.
    assert_eq!(
        windows(&timeline, 0),
        vec![(0, 0, 45), (45, 0, 30), (75, 75, 45)]
    );
}

#[test]
fn overwriting_end_to_end_takes_the_clip_away() {
    let mut timeline = Timeline::default();
    timeline
        .apply(Edit::Place {
            track: 0,
            source: source(1),
            position: 0,
            source_start: 0,
            length: 30,
        })
        .unwrap();
    timeline.apply(Edit::Append(source(2))).unwrap();
    let long = ids(&timeline)[1];

    // The 60-frame clip lands over the 30-frame one, which has nothing
    // left to show.
    timeline.apply(moved(long, 0, 0)).unwrap();

    assert_eq!(windows(&timeline, 0), vec![(0, 0, 60)]);
}

#[test]
fn appends_after_everything_already_there() {
    let timeline = pair();

    assert_eq!(windows(&timeline, 0), vec![(0, 0, 60), (60, 0, 30)]);
    assert_eq!(timeline.length(), 90);
    // An empty document takes its timebase from the first media in it.
    assert_eq!(timeline.timebase, Fraction::new(30, 1));
}

#[test]
fn ids_are_unique_and_survive_edits() {
    let mut timeline = pair();
    let before = ids(&timeline);

    timeline
        .apply(Edit::Insert {
            source: source(1),
            frame: 0,
        })
        .unwrap();

    // The two original clips kept their ids through the ripple; the new one
    // got an id of its own.
    let after = ids(&timeline);
    assert_eq!(after[1..], before[..]);
    assert!(!before.contains(&after[0]));
}

#[test]
fn insert_ripples_what_follows_and_snaps_to_an_edge() {
    let mut timeline = pair();

    // Frame 55 is inside the first clip; the edge at 60 is the closest, so
    // the new clip lands between the two rather than inside either.
    timeline
        .apply(Edit::Insert {
            source: source(1),
            frame: 55,
        })
        .unwrap();

    assert_eq!(
        windows(&timeline, 0),
        vec![(0, 0, 60), (60, 0, 30), (90, 0, 30)]
    );
}

#[test]
fn a_move_onto_a_taken_landing_shortens_what_is_there() {
    let mut timeline = pair();
    let second = ids(&timeline)[1];

    // 30..60 is the first clip's, and the move takes it: the frames a
    // gesture asks for are the frames it gets.
    timeline.apply(moved(second, 0, 30)).unwrap();
    assert_eq!(windows(&timeline, 0), vec![(0, 0, 30), (30, 0, 30)]);

    // Past the end there is nothing to take.
    timeline.apply(moved(second, 0, 200)).unwrap();
    assert_eq!(windows(&timeline, 0), vec![(0, 0, 30), (200, 0, 30)]);
}

#[test]
fn move_onto_the_track_past_the_last_starts_one() {
    let mut timeline = pair();
    let second = ids(&timeline)[1];

    timeline.apply(moved(second, 1, 0)).unwrap();

    assert_eq!(windows(&timeline, 0), vec![(0, 0, 60)]);
    assert_eq!(windows(&timeline, 1), vec![(0, 0, 30)]);
    // A track further down than that does not exist to move onto.
    assert!(timeline.apply(moved(second, 3, 0)).is_err());
}

#[test]
fn trimming_the_in_point_moves_the_window_with_it() {
    let mut timeline = pair();
    let first = ids(&timeline)[0];

    timeline.apply(trimmed(first, Edge::In, 10)).unwrap();

    // Ten frames later in the timeline is ten frames later in the source,
    // and the out-point has not moved.
    assert_eq!(windows(&timeline, 0)[0], (10, 10, 50));
}

#[test]
fn a_trim_cannot_reach_past_its_source() {
    let mut timeline = Timeline::default();
    timeline.apply(Edit::Append(source(1))).unwrap();
    let clip = ids(&timeline)[0];

    // The whole source is already in the clip: there is nothing before its
    // first frame, and nothing after its last.
    assert!(timeline.apply(trimmed(clip, Edge::In, 0)).is_ok());
    timeline.apply(trimmed(clip, Edge::In, 5)).unwrap();
    assert!(timeline.apply(trimmed(clip, Edge::Out, 40)).is_err());
    // And an out-point cannot cross its in-point.
    assert!(timeline.apply(trimmed(clip, Edge::Out, 5)).is_err());
    assert_eq!(windows(&timeline, 0)[0], (5, 5, 25));
}

#[test]
fn split_hands_the_tail_the_right_frames() {
    let mut timeline = Timeline::default();
    timeline
        .apply(Edit::Place {
            track: 0,
            source: source(2),
            position: 0,
            source_start: 10,
            length: 50,
        })
        .unwrap();
    let clip = ids(&timeline)[0];

    timeline.apply(cut(clip, 20)).unwrap();

    // The tail picks up in the source exactly where the head left off.
    assert_eq!(windows(&timeline, 0), vec![(0, 10, 20), (20, 30, 30)]);
    // A cut has to fall inside the clip to mean anything.
    assert!(timeline.apply(cut(clip, 0)).is_err());
}

#[test]
fn delete_either_leaves_the_gap_or_closes_it() {
    let mut timeline = pair();
    let first = ids(&timeline)[0];

    timeline.apply(deleted(first, false)).unwrap();
    assert_eq!(windows(&timeline, 0), vec![(60, 0, 30)]);

    let mut timeline = pair();
    let first = ids(&timeline)[0];

    timeline.apply(deleted(first, true)).unwrap();
    assert_eq!(windows(&timeline, 0), vec![(0, 0, 30)]);

    // The clip is gone, so naming it again is not an edit.
    assert!(timeline.apply(deleted(first, false)).is_err());
}

#[test]
fn next_content_steps_over_a_gap() {
    let mut timeline = pair();
    let first = ids(&timeline)[0];
    timeline.apply(deleted(first, false)).unwrap();

    // Frame 0..60 is now a hole: playback has to land on 60.
    assert_eq!(timeline.next_content(0), Some(60));
    assert_eq!(timeline.next_content(70), Some(70));
    // Past the end it wraps to the front, which is where the loop goes.
    assert_eq!(timeline.next_content(90), Some(60));
    assert_eq!(Timeline::default().next_content(0), None);
}

#[test]
fn a_rejected_edit_changes_nothing() {
    let mut timeline = pair();
    let before = windows(&timeline, 0);

    // A source with no frames in it, at the end and rippled in.
    let empty = Arc::new(Source {
        path: PathBuf::from("silence.wav"),
        duration: Duration::from_secs(3),
        frame_rate: Fraction::new(0, 1),
    });

    assert!(timeline.apply(Edit::Append(empty.clone())).is_err());
    assert!(
        timeline
            .apply(Edit::Insert {
                source: empty,
                frame: 30
            })
            .is_err()
    );
    assert_eq!(windows(&timeline, 0), before);
}
