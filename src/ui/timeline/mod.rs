//! The timeline widget: a canvas that draws the document and turns clicks and
//! drags into seeks and edits. Layout constants and event handling live here;
//! painting is in `draw`, viewport and gesture state in `state`, playhead
//! chasing in `follow`.
//!
//! The widget never changes the document. It works out what a gesture means,
//! and on release publishes one `Edit` for the app to apply — so a drag costs
//! one edit rather than one per mouse move, and `Timeline::apply` stays the
//! only thing that can change anything.

mod draw;
mod follow;
mod state;

use iced::widget::canvas::{self, Frame, Geometry};
use iced::{Element, Point, Rectangle, Renderer, Theme, mouse, window};

use crate::project::{Clip, ClipId, Edge, Edit, Timeline};
use state::{Drag, State};

const RULER_HEIGHT: f32 = 22.0;
const TRACK_HEIGHT: f32 = 54.0;
const TRACK_GAP: f32 = 6.0;
const CLIP_RADIUS: f32 = 4.0;

/// How close to a clip's end the pointer has to be to take hold of it, in
/// pixels. Narrow clips give up a third of their width at most, so there is
/// always some middle left to pick the clip up by.
const EDGE_GRAB: f32 = 6.0;
/// How near an edge a dragged frame is pulled onto it, in pixels. Landing flush
/// against the next clip is usually the point of the drag, and a pixel of slop
/// should not leave a one-frame hole behind.
const SNAP: f32 = 8.0;

/// Zoom bounds, in pixels per frame.
const MIN_ZOOM: f32 = 0.05;
const MAX_ZOOM: f32 = 40.0;

/// Tick spacings we're willing to label, in seconds.
const TICK_STEPS: [f64; 11] = [
    0.5, 1.0, 2.0, 5.0, 10.0, 15.0, 30.0, 60.0, 120.0, 300.0, 600.0,
];
/// Minimum pixels between two labelled ticks.
const TICK_MIN_SPACING: f32 = 90.0;

pub struct TimelineView<'a, Message> {
    timeline: &'a Timeline,
    playhead: usize,
    fps: f64,
    follow_playhead: bool,
    selection: Option<ClipId>,
    on_seek: Option<Box<dyn Fn(usize) -> Message + 'a>>,
    on_select: Option<Box<dyn Fn(ClipId) -> Message + 'a>>,
    on_edit: Option<Box<dyn Fn(Edit) -> Message + 'a>>,
    on_context: Option<Box<dyn Fn(ClipId, Point) -> Message + 'a>>,
}

/// What is under a point.
struct Hit {
    clip: ClipId,
    track: usize,
    position: usize,
    length: usize,
    /// The frame the pointer is actually on, for working out where in the clip
    /// it was picked up.
    frame: usize,
    /// Which end the pointer is close enough to take hold of, if either.
    edge: Option<Edge>,
}

impl<'a, Message: 'a> TimelineView<'a, Message> {
    pub fn new(timeline: &'a Timeline, playhead: usize, fps: f64) -> Self {
        TimelineView {
            timeline,
            playhead,
            fps: if fps > 0.0 { fps } else { 30.0 },
            follow_playhead: false,
            selection: None,
            on_seek: None,
            on_select: None,
            on_edit: None,
            on_context: None,
        }
    }

    /// Whether the view should chase the playhead. The widget doesn't know what
    /// playback is doing — the caller decides when following is wanted.
    pub fn follow_playhead(mut self, follow: bool) -> Self {
        self.follow_playhead = follow;
        self
    }

    pub fn selection(mut self, selection: Option<ClipId>) -> Self {
        self.selection = selection;
        self
    }

    pub fn on_seek(mut self, f: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_seek = Some(Box::new(f));
        self
    }

    pub fn on_select(mut self, f: impl Fn(ClipId) -> Message + 'a) -> Self {
        self.on_select = Some(Box::new(f));
        self
    }

    pub fn on_edit(mut self, f: impl Fn(Edit) -> Message + 'a) -> Self {
        self.on_edit = Some(Box::new(f));
        self
    }

    /// Called with the clip that was right-clicked and where in the window it
    /// happened, so a menu can be put there.
    pub fn on_context(mut self, f: impl Fn(ClipId, Point) -> Message + 'a) -> Self {
        self.on_context = Some(Box::new(f));
        self
    }

    fn height(&self) -> f32 {
        // One lane past the last, so there is somewhere to drag a clip to start
        // a new track.
        RULER_HEIGHT + (self.timeline.tracks.len() + 1) as f32 * (TRACK_HEIGHT + TRACK_GAP)
    }

    fn seek(&self, frame: usize) -> Option<canvas::Action<Message>> {
        self.on_seek
            .as_ref()
            .map(|f| canvas::Action::publish(f(frame)).and_capture())
    }

    fn select(&self, clip: ClipId) -> Option<canvas::Action<Message>> {
        self.on_select
            .as_ref()
            .map(|f| canvas::Action::publish(f(clip)).and_capture())
            .or_else(|| Some(canvas::Action::capture()))
    }

    fn edit(&self, edit: Edit) -> Option<canvas::Action<Message>> {
        self.on_edit
            .as_ref()
            .map(|f| canvas::Action::publish(f(edit)).and_capture())
            .or_else(|| Some(canvas::Action::request_redraw().and_capture()))
    }

    /// The clip `id` names, and which track it is on.
    fn clip(&self, id: ClipId) -> Option<(usize, &Clip)> {
        self.timeline
            .tracks
            .iter()
            .enumerate()
            .find_map(|(track, lane)| {
                lane.clips
                    .iter()
                    .find(|clip| clip.id == id)
                    .map(|clip| (track, clip))
            })
    }

    fn hit(&self, state: &State, point: Point) -> Option<Hit> {
        let track = state.track_at(point.y)?;
        let frame = state.frame_at(point.x);
        let clip = self.timeline.tracks.get(track)?.clip_at(frame)?;

        let zone = EDGE_GRAB.min(clip.length as f32 * state.zoom / 3.0);
        let edge = if (point.x - state.x_of(clip.position as f32)).abs() <= zone {
            Some(Edge::In)
        } else if (state.x_of(clip.end() as f32) - point.x).abs() <= zone {
            Some(Edge::Out)
        } else {
            None
        };

        Some(Hit {
            clip: clip.id,
            track,
            position: clip.position,
            length: clip.length,
            frame,
            edge,
        })
    }

    /// Pull `frame` onto a nearby edge — another clip's end, the playhead, or
    /// the start of the document — when it is within `SNAP` pixels.
    fn snap(&self, state: &State, track: usize, frame: usize, ignore: ClipId) -> usize {
        let tolerance = (SNAP / state.zoom).max(0.5);

        self.timeline
            .tracks
            .get(track)
            .into_iter()
            .flat_map(|lane| &lane.clips)
            .filter(|clip| clip.id != ignore)
            .flat_map(|clip| [clip.position, clip.end()])
            .chain([0, self.playhead])
            .filter(|edge| (*edge as f32 - frame as f32).abs() <= tolerance)
            .min_by_key(|edge| edge.abs_diff(frame))
            .unwrap_or(frame)
    }

    /// Snapping for a clip being carried: either end landing flush is worth the
    /// same, so try the leading edge and then the trailing one.
    fn snap_carried(
        &self,
        state: &State,
        track: usize,
        position: usize,
        length: usize,
        clip: ClipId,
    ) -> usize {
        let head = self.snap(state, track, position, clip);
        if head != position {
            return head;
        }

        let tail = self.snap(state, track, position + length, clip);
        if tail != position + length {
            return tail.saturating_sub(length);
        }

        position
    }

    /// How far a trim can go: not past the end that isn't moving, not past what
    /// the source has, and not into the clip next door.
    fn trim_range(&self, track: usize, clip: ClipId, edge: Edge, anchor: usize) -> Option<(usize, usize)> {
        let (_, current) = self.clip(clip)?;
        let spare = current
            .source
            .frame_count()
            .saturating_sub(current.source_start);
        let (before, after) = self.neighbours(track, clip);

        Some(match edge {
            Edge::In => (
                before.max(current.position.saturating_sub(current.source_start)),
                anchor.saturating_sub(1),
            ),
            Edge::Out => (anchor + 1, after.min(current.position + spare)),
        })
    }

    /// Where the clips either side of `clip` leave off — the walls a trim or a
    /// move runs into.
    fn neighbours(&self, track: usize, clip: ClipId) -> (usize, usize) {
        let Some(clips) = self.timeline.tracks.get(track).map(|lane| &lane.clips) else {
            return (0, usize::MAX);
        };
        let Some(index) = clips.iter().position(|existing| existing.id == clip) else {
            return (0, usize::MAX);
        };

        (
            clips[..index].last().map(Clip::end).unwrap_or(0),
            clips[index + 1..]
                .first()
                .map(|clip| clip.position)
                .unwrap_or(usize::MAX),
        )
    }
}

impl<'a, Message: 'a> From<TimelineView<'a, Message>> for Element<'a, Message> {
    fn from(widget: TimelineView<'a, Message>) -> Self {
        let height = widget.height();

        iced::widget::canvas(widget)
            .width(iced::Fill)
            .height(height)
            .into()
    }
}

impl<Message> canvas::Program<Message> for TimelineView<'_, Message> {
    type State = State;

    fn update(
        &self,
        state: &mut State,
        event: &canvas::Event,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Option<canvas::Action<Message>> {
        let position = cursor.position_in(bounds);

        match event {
            canvas::Event::Window(window::Event::RedrawRequested(now)) => {
                if !state.fitted {
                    state.fitted = true;

                    let length = self.timeline.length();
                    if length > 0 && bounds.width > 0.0 {
                        state.zoom = (bounds.width / length as f32).clamp(MIN_ZOOM, MAX_ZOOM);
                    }

                    return Some(canvas::Action::request_redraw());
                }

                // Not following: a seek while paused moves the playhead, and
                // the view should stay where the user left it.
                if !self.follow_playhead {
                    state.follow.release();
                    return None;
                }

                state
                    .follow
                    .advance(
                        self.playhead,
                        &mut state.scroll,
                        state.zoom,
                        bounds.width,
                        *now,
                    )
                    .then(canvas::Action::request_redraw)
            }

            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let point = position?;

                // Taking hold of anything stops the view chasing the playhead.
                state.follow.release();

                // On a clip's end, that end. On its middle, the clip. Anywhere
                // else, the playhead.
                match self.hit(state, point) {
                    Some(hit) => {
                        state.drag = Some(match hit.edge {
                            Some(edge) => Drag::Trim {
                                clip: hit.clip,
                                track: hit.track,
                                edge,
                                frame: match edge {
                                    Edge::In => hit.position,
                                    Edge::Out => hit.position + hit.length,
                                },
                                anchor: match edge {
                                    Edge::In => hit.position + hit.length,
                                    Edge::Out => hit.position,
                                },
                            },
                            None => Drag::Move {
                                clip: hit.clip,
                                length: hit.length,
                                grab: hit.frame - hit.position,
                                track: hit.track,
                                position: hit.position,
                            },
                        });

                        self.select(hit.clip)
                    }
                    None => {
                        let frame = state.frame_at(point.x);
                        state.drag = Some(Drag::Scrub { frame });

                        self.seek(frame)
                    }
                }
            }

            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if state.drag.is_some() => {
                // Keep the gesture alive even if the cursor wanders out of the
                // widget; clamp to the edges instead of dropping it.
                let point = cursor.position()?;
                let x = (point.x - bounds.x).clamp(0.0, bounds.width);
                let frame = state.frame_at(x);

                match state.drag.take()? {
                    Drag::Scrub { frame: last } => {
                        state.drag = Some(Drag::Scrub { frame });

                        // Zoomed out, many pixels map to one frame — don't
                        // flush the pipeline for a seek that wouldn't move the
                        // playhead.
                        if last == frame {
                            return Some(canvas::Action::capture());
                        }

                        self.seek(frame)
                    }

                    Drag::Move {
                        clip,
                        length,
                        grab,
                        track,
                        ..
                    } => {
                        // One lane past the last is allowed: dropping there
                        // starts a track. Between lanes, keep the last one.
                        let track = state
                            .track_at(point.y - bounds.y)
                            .unwrap_or(track)
                            .min(self.timeline.tracks.len());

                        let wanted = frame.saturating_sub(grab);
                        let position = self.snap_carried(state, track, wanted, length, clip);

                        state.drag = Some(Drag::Move {
                            clip,
                            length,
                            grab,
                            track,
                            position,
                        });

                        Some(canvas::Action::request_redraw().and_capture())
                    }

                    Drag::Trim {
                        clip,
                        track,
                        edge,
                        anchor,
                        ..
                    } => {
                        let (low, high) = self.trim_range(track, clip, edge, anchor)?;
                        let frame = self
                            .snap(state, track, frame.clamp(low, high), clip)
                            .clamp(low, high);

                        state.drag = Some(Drag::Trim {
                            clip,
                            track,
                            edge,
                            frame,
                            anchor,
                        });

                        Some(canvas::Action::request_redraw().and_capture())
                    }
                }
            }

            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                match state.drag.take()? {
                    Drag::Scrub { .. } => Some(canvas::Action::capture()),

                    Drag::Move {
                        clip,
                        length,
                        track,
                        position,
                        ..
                    } => {
                        let home = self.clip(clip);

                        // A drag that ends where it started, or somewhere it
                        // cannot land, is not an edit — and shouldn't cost an
                        // undo step.
                        if home.is_some_and(|(from, clip)| from == track && clip.position == position)
                            || !self.timeline.has_room(track, position, length, clip)
                        {
                            return Some(canvas::Action::request_redraw().and_capture());
                        }

                        self.edit(Edit::Move {
                            clip,
                            track,
                            position,
                        })
                    }

                    Drag::Trim {
                        clip, edge, frame, ..
                    } => {
                        let unmoved = self.clip(clip).is_some_and(|(_, clip)| {
                            frame
                                == match edge {
                                    Edge::In => clip.position,
                                    Edge::Out => clip.end(),
                                }
                        });

                        if unmoved {
                            return Some(canvas::Action::capture());
                        }

                        self.edit(Edit::Trim { clip, edge, frame })
                    }
                }
            }

            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Right)) => {
                let hit = self.hit(state, position?)?;
                // Placed in window coordinates, not the widget's, since the
                // menu is laid over the whole page.
                let at = cursor.position()?;

                self.on_context
                    .as_ref()
                    .map(|f| canvas::Action::publish(f(hit.clip, at)).and_capture())
            }

            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let point = position?;

                // Taking the view by hand stops the chase, and kills any glide
                // still in flight so it doesn't fight the gesture.
                state.follow.release();

                let (dx, dy) = match delta {
                    mouse::ScrollDelta::Lines { x, y } => (x * 16.0, y * 16.0),
                    mouse::ScrollDelta::Pixels { x, y } => (*x, *y),
                };

                if dy.abs() > dx.abs() {
                    // Zoom around the frame under the cursor, so it stays put.
                    let anchor = state.scroll + point.x / state.zoom;
                    state.zoom = (state.zoom * (1.0 + dy / 200.0)).clamp(MIN_ZOOM, MAX_ZOOM);
                    state.scroll = (anchor - point.x / state.zoom).max(0.0);
                } else {
                    state.scroll = (state.scroll - dx / state.zoom).max(0.0);
                }

                Some(canvas::Action::request_redraw().and_capture())
            }
            _ => None,
        }
    }

    fn draw(
        &self,
        state: &State,
        renderer: &Renderer,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> Vec<Geometry> {
        let mut frame = Frame::new(renderer, bounds.size());

        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            theme.extended_palette().background.weakest.color,
        );

        // Bottom layer first: the ruler and playhead sit on top of the tracks,
        // and a gesture's ghost sits on top of everything but them.
        self.draw_tracks(&mut frame, state, theme, bounds);
        self.draw_drag(&mut frame, state, theme);
        self.draw_ruler(&mut frame, state, theme, bounds);
        self.draw_cursor(&mut frame, theme, bounds, cursor);
        self.draw_playhead(&mut frame, state, theme, bounds);

        vec![frame.into_geometry()]
    }

    fn mouse_interaction(
        &self,
        state: &State,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) -> mouse::Interaction {
        match &state.drag {
            Some(Drag::Trim { .. }) => mouse::Interaction::ResizingHorizontally,
            Some(_) => mouse::Interaction::Grabbing,
            None => match cursor.position_in(bounds).and_then(|at| self.hit(state, at)) {
                Some(hit) if hit.edge.is_some() => mouse::Interaction::ResizingHorizontally,
                Some(_) => mouse::Interaction::Grab,
                None if cursor.is_over(bounds) => mouse::Interaction::Pointer,
                None => mouse::Interaction::default(),
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::time::Duration;

    use gstreamer::Fraction;

    use super::*;
    use crate::media::Source;

    /// A source of `seconds` at 30 fps, stated rather than probed.
    fn source(seconds: u64) -> Arc<Source> {
        Arc::new(Source {
            path: PathBuf::from(format!("{seconds}s.mp4")),
            duration: Duration::from_secs(seconds),
            frame_rate: Fraction::new(30, 1),
        })
    }

    /// `(source seconds, position, source_start, length)` per clip, all on the
    /// first track.
    fn timeline(clips: &[(u64, usize, usize, usize)]) -> Timeline {
        let mut timeline = Timeline::default();

        for (seconds, position, source_start, length) in clips {
            timeline
                .apply(Edit::Place {
                    track: 0,
                    source: source(*seconds),
                    position: *position,
                    source_start: *source_start,
                    length: *length,
                })
                .unwrap();
        }

        timeline
    }

    /// The default viewport: two pixels per frame, scrolled to the start. So a
    /// frame is at twice its number, and `SNAP`'s eight pixels are four frames.
    fn view(timeline: &Timeline) -> (TimelineView<'_, ()>, State) {
        (TimelineView::new(timeline, 0, 30.0), State::default())
    }

    #[test]
    fn a_lane_is_the_track_and_the_ruler_is_neither() {
        let timeline = timeline(&[(2, 0, 0, 60)]);
        let (_, state) = view(&timeline);

        assert_eq!(state.track_at(4.0), None, "the ruler");
        assert_eq!(state.track_at(24.0), None, "the gap above the first lane");
        assert_eq!(state.track_at(40.0), Some(0));
        assert_eq!(state.track_at(100.0), Some(1));
    }

    #[test]
    fn the_ends_of_a_clip_are_what_a_trim_takes_hold_of() {
        let timeline = timeline(&[(2, 0, 0, 60)]);
        let (view, state) = view(&timeline);
        let y = state.top_of(0) + 10.0;

        let edge = |x: f32| view.hit(&state, Point::new(x, y)).unwrap().edge;

        assert!(matches!(edge(2.0), Some(Edge::In)));
        assert!(matches!(edge(118.0), Some(Edge::Out)));
        // The middle picks the clip up instead.
        assert!(edge(60.0).is_none());
        // Off the end of the clip is not the clip at all.
        assert!(view.hit(&state, Point::new(130.0, y)).is_none());
        // Neither is the ruler above it.
        assert!(view.hit(&state, Point::new(60.0, 4.0)).is_none());
    }

    #[test]
    fn a_drag_snaps_onto_an_edge_it_is_close_to() {
        let timeline = timeline(&[(2, 0, 0, 60), (1, 60, 0, 30)]);
        let (view, state) = view(&timeline);
        let first = timeline.tracks[0].clips[0].id;

        // Two frames short of the second clip's start, within the four frames
        // `SNAP` comes to at this zoom.
        assert_eq!(view.snap(&state, 0, 58, first), 60);
        // Ten frames short of it is a gap somebody meant to leave.
        assert_eq!(view.snap(&state, 0, 50, first), 50);
    }

    #[test]
    fn a_carried_clip_snaps_by_either_end() {
        // A clip parked out at 400 is the one being carried; the clip at 100 is
        // what it has to snap against. A clip never snaps to its own edges,
        // which is what `ignore` is for.
        let timeline = timeline(&[(2, 100, 0, 60), (1, 400, 0, 30)]);
        let (view, state) = view(&timeline);
        let carried = timeline.tracks[0].clips[1].id;

        // Its trailing edge two frames short of the clip at 100: pulled flush,
        // which puts its in-point thirty frames earlier.
        assert_eq!(view.snap_carried(&state, 0, 68, 30, carried), 70);
        // Its leading edge just past that clip's end: pulled back flush.
        assert_eq!(view.snap_carried(&state, 0, 162, 30, carried), 160);
        // Nowhere near anything: left alone.
        assert_eq!(view.snap_carried(&state, 0, 20, 30, carried), 20);
    }

    #[test]
    fn a_trim_stops_at_the_clip_next_door() {
        let timeline = timeline(&[(3, 0, 10, 50), (1, 60, 0, 30)]);
        let (view, state) = view(&timeline);
        let first = timeline.tracks[0].clips[0].id;
        let _ = state;

        // The source has 80 frames left from its in-point, but the next clip
        // starts at 60, so that is as far as the out-point goes.
        assert_eq!(view.trim_range(0, first, Edge::Out, 0), Some((1, 60)));
    }

    #[test]
    fn a_trim_stops_where_its_source_runs_out() {
        let timeline = timeline(&[(3, 20, 10, 50)]);
        let (view, _) = view(&timeline);
        let only = timeline.tracks[0].clips[0].id;

        // Nothing next door, so the source is the wall: 90 frames from frame
        // 10 reaches frame 100 on the timeline.
        assert_eq!(view.trim_range(0, only, Edge::Out, 20), Some((21, 100)));
        // And the in-point can only reach back the ten frames it skipped.
        assert_eq!(view.trim_range(0, only, Edge::In, 70), Some((10, 69)));
    }
}
