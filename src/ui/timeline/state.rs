use super::follow::Follow;
use super::{RULER_HEIGHT, TRACK_GAP, TRACK_HEIGHT};
use crate::project::{ClipId, Edge};

/// What the pointer is in the middle of doing, between press and release.
///
/// A drag holds its own pending geometry and the document is left alone until
/// the button comes up: one edit per gesture rather than one per mouse move, so
/// the decoders are not re-cut and autosave is not woken forty times a second.
/// What the drag would do is drawn as a ghost in the meantime.
pub(super) enum Drag {
    /// Dragging the playhead. Holds the last frame asked for, so a scrub that
    /// stays inside one frame doesn't flush the pipeline.
    Scrub { frame: usize },
    /// Carrying a clip. `grab` is how far into the clip it was picked up, so it
    /// doesn't jump to sit under the pointer.
    Move {
        clip: ClipId,
        length: usize,
        grab: usize,
        track: usize,
        position: usize,
    },
    /// Pulling one end of a clip. `frame` is where that end is being taken.
    Trim {
        clip: ClipId,
        track: usize,
        edge: Edge,
        frame: usize,
        /// The end that isn't moving, so the ghost can be drawn from both.
        anchor: usize,
    },
}

/// View state owned by the canvas: the viewport, drag tracking, and how the
/// view follows the playhead. The playhead itself lives in the app, since the
/// video pipeline is the source of truth.
pub struct State {
    /// Frame at the left edge of the widget.
    pub(super) scroll: f32,
    /// Pixels per frame.
    pub(super) zoom: f32,
    /// The gesture in flight, or `None` when the button is up.
    pub(super) drag: Option<Drag>,
    /// Whether the initial fit-to-width has happened. It can't run until we
    /// know both the widget's width and the timeline's length, neither of
    /// which is available when the state is created.
    pub(super) fitted: bool,
    pub(super) follow: Follow,
}

impl Default for State {
    fn default() -> Self {
        State {
            scroll: 0.0,
            // Replaced by the fit-to-width on the first redraw; only has to be
            // non-zero so nothing divides by it in the meantime.
            zoom: 2.0,
            drag: None,
            fitted: false,
            follow: Follow::default(),
        }
    }
}

impl State {
    /// Where a timeline frame sits horizontally, in widget pixels.
    pub(super) fn x_of(&self, frame: f32) -> f32 {
        (frame - self.scroll) * self.zoom
    }

    /// The inverse: which timeline frame a pixel column points at.
    pub(super) fn frame_at(&self, x: f32) -> usize {
        (self.scroll + x / self.zoom).max(0.0) as usize
    }

    /// The top of a track's lane, in widget pixels.
    pub(super) fn top_of(&self, track: usize) -> f32 {
        RULER_HEIGHT + track as f32 * (TRACK_HEIGHT + TRACK_GAP) + TRACK_GAP
    }

    /// Which lane a pixel row is in, or `None` for the ruler and the gaps
    /// between lanes — where a drag has not picked a track.
    pub(super) fn track_at(&self, y: f32) -> Option<usize> {
        if y < RULER_HEIGHT {
            return None;
        }

        let lane = TRACK_HEIGHT + TRACK_GAP;
        let track = ((y - RULER_HEIGHT) / lane).floor().max(0.0) as usize;

        // The gap above each lane belongs to no track.
        (y >= self.top_of(track)).then_some(track)
    }
}
