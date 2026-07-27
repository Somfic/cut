//! The timeline widget: a canvas that draws the document and turns clicks and
//! drags into seeks. Layout constants and event handling live here; painting is
//! in `draw`, viewport state in `state`.

mod draw;
mod state;

use iced::widget::canvas::{self, Frame, Geometry};
use iced::{Element, Point, Rectangle, Renderer, Theme, mouse, window};

use crate::project::Timeline;
use state::State;

const RULER_HEIGHT: f32 = 22.0;
const TRACK_HEIGHT: f32 = 54.0;
const TRACK_GAP: f32 = 6.0;
const CLIP_RADIUS: f32 = 4.0;

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
    on_seek: Option<Box<dyn Fn(usize) -> Message + 'a>>,
}

impl<'a, Message: 'a> TimelineView<'a, Message> {
    pub fn new(timeline: &'a Timeline, playhead: usize, fps: f64) -> Self {
        TimelineView {
            timeline,
            playhead,
            fps: if fps > 0.0 { fps } else { 30.0 },
            on_seek: None,
        }
    }

    pub fn on_seek(mut self, f: impl Fn(usize) -> Message + 'a) -> Self {
        self.on_seek = Some(Box::new(f));
        self
    }

    fn height(&self) -> f32 {
        RULER_HEIGHT + self.timeline.tracks.len() as f32 * (TRACK_HEIGHT + TRACK_GAP)
    }

    fn seek(&self, frame: usize) -> Option<canvas::Action<Message>> {
        self.on_seek
            .as_ref()
            .map(|f| canvas::Action::publish(f(frame)).and_capture())
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
            canvas::Event::Window(window::Event::RedrawRequested(_)) if !state.fitted => {
                state.fitted = true;

                let length = self.timeline.length();
                if length > 0 && bounds.width > 0.0 {
                    state.zoom = (bounds.width / length as f32).clamp(MIN_ZOOM, MAX_ZOOM);
                }

                Some(canvas::Action::request_redraw())
            }
            canvas::Event::Mouse(mouse::Event::ButtonPressed(mouse::Button::Left)) => {
                let point = position?;
                let frame = state.frame_at(point.x);
                state.scrubbing = Some(frame);
                self.seek(frame)
            }
            canvas::Event::Mouse(mouse::Event::CursorMoved { .. }) if state.scrubbing.is_some() => {
                // Keep scrubbing even if the cursor wanders out of the widget;
                // clamp to the edges instead of dropping the drag.
                let x = cursor.position()?.x - bounds.x;
                let frame = state.frame_at(x.clamp(0.0, bounds.width));

                // Zoomed out, many pixels map to one frame — don't flush the
                // pipeline for a seek that wouldn't move the playhead.
                if state.scrubbing == Some(frame) {
                    return Some(canvas::Action::capture());
                }

                state.scrubbing = Some(frame);
                self.seek(frame)
            }
            canvas::Event::Mouse(mouse::Event::ButtonReleased(mouse::Button::Left)) => {
                state.scrubbing.take()?;
                Some(canvas::Action::capture())
            }
            canvas::Event::Mouse(mouse::Event::WheelScrolled { delta }) => {
                let point = position?;

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

        // Bottom layer first: the ruler and playhead sit on top of the tracks.
        self.draw_tracks(&mut frame, state, theme, bounds);
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
        if state.scrubbing.is_some() {
            mouse::Interaction::Grabbing
        } else if cursor.is_over(bounds) {
            mouse::Interaction::Pointer
        } else {
            mouse::Interaction::default()
        }
    }
}
