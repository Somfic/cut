use iced::widget::canvas::{self, Frame, Geometry, Path, Stroke, Text};
use iced::{Color, Element, Point, Rectangle, Renderer, Size, Theme, border, mouse, window};

use crate::timeline::Timeline;

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

pub struct TimelineWidget<'a, Message> {
    timeline: &'a Timeline,
    playhead: usize,
    fps: f64,
    on_seek: Option<Box<dyn Fn(usize) -> Message + 'a>>,
}

/// View state owned by the canvas: viewport and drag tracking. The playhead
/// itself lives in the app, since the video pipeline is the source of truth.
#[derive(Debug)]
pub struct State {
    /// Frame at the left edge of the widget.
    scroll: f32,
    /// Pixels per frame.
    zoom: f32,
    /// The last frame we asked for while dragging, or `None` when not dragging.
    /// Remembered to suppress repeat seeks for a frame we're already on.
    scrubbing: Option<usize>,
    /// Whether the initial fit-to-width has happened. It can't run until we
    /// know both the widget's width and the timeline's length, neither of
    /// which is available when the state is created.
    fitted: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            scroll: 0.0,
            zoom: 2.0,
            scrubbing: None,
            fitted: false,
        }
    }
}

impl State {
    fn x_of(&self, frame: f32) -> f32 {
        (frame - self.scroll) * self.zoom
    }

    fn frame_at(&self, x: f32) -> usize {
        (self.scroll + x / self.zoom).max(0.0) as usize
    }
}

pub fn timeline<'a, Message: 'a>(
    timeline: &'a Timeline,
    playhead: usize,
    fps: f64,
) -> TimelineWidget<'a, Message> {
    TimelineWidget {
        timeline,
        playhead,
        fps: if fps > 0.0 { fps } else { 30.0 },
        on_seek: None,
    }
}

impl<'a, Message: 'a> TimelineWidget<'a, Message> {
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

impl<'a, Message: 'a> From<TimelineWidget<'a, Message>> for Element<'a, Message> {
    fn from(widget: TimelineWidget<'a, Message>) -> Self {
        let height = widget.height();

        iced::widget::canvas(widget)
            .width(iced::Fill)
            .height(height)
            .into()
    }
}

impl<Message> canvas::Program<Message> for TimelineWidget<'_, Message> {
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
        let palette = theme.extended_palette();
        let mut frame = Frame::new(renderer, bounds.size());

        frame.fill_rectangle(
            Point::ORIGIN,
            bounds.size(),
            palette.background.weakest.color,
        );

        // Tracks, bottom layer first so the ruler and playhead sit on top.
        for (index, track) in self.timeline.tracks.iter().enumerate() {
            let top = RULER_HEIGHT + index as f32 * (TRACK_HEIGHT + TRACK_GAP) + TRACK_GAP;

            frame.fill_rectangle(
                Point::new(0.0, top),
                Size::new(bounds.width, TRACK_HEIGHT),
                palette.background.weak.color,
            );

            for clip in &track.clips {
                let x = state.x_of(clip.position as f32);
                let width = clip.length as f32 * state.zoom;

                // Cheap horizontal culling — timelines get long.
                if x + width < 0.0 || x > bounds.width {
                    continue;
                }

                let body = Path::rounded_rectangle(
                    Point::new(x, top),
                    Size::new(width, TRACK_HEIGHT),
                    border::Radius::from(CLIP_RADIUS),
                );

                frame.fill(&body, palette.primary.weak.color);
                frame.stroke(
                    &body,
                    Stroke::default()
                        .with_color(palette.primary.strong.color)
                        .with_width(1.0),
                );

                let label = clip
                    .source
                    .path
                    .file_stem()
                    .map(|stem| stem.to_string_lossy().into_owned())
                    .unwrap_or_default();

                // Only label when there's room for something readable.
                if width > 40.0 {
                    frame.with_clip(
                        Rectangle::new(Point::new(x, top), Size::new(width, TRACK_HEIGHT)),
                        |frame| {
                            frame.fill_text(Text {
                                content: label,
                                position: Point::new(x + 6.0, top + 5.0),
                                color: palette.primary.weak.text,
                                size: 12.0.into(),
                                max_width: width - 12.0,
                                ..Text::default()
                            });
                        },
                    );
                }
            }
        }

        // Ruler.
        frame.fill_rectangle(
            Point::ORIGIN,
            Size::new(bounds.width, RULER_HEIGHT),
            palette.background.weak.color,
        );

        let seconds_per_pixel = 1.0 / (state.zoom as f64 * self.fps);
        let step = TICK_STEPS
            .iter()
            .copied()
            .find(|step| (step / seconds_per_pixel) as f32 >= TICK_MIN_SPACING)
            .unwrap_or(*TICK_STEPS.last().unwrap());

        let first = (state.scroll as f64 / self.fps / step).floor() as i64;
        let mut tick = first.max(0);

        loop {
            let seconds = tick as f64 * step;
            let x = state.x_of((seconds * self.fps) as f32);

            if x > bounds.width {
                break;
            }
            tick += 1;

            if x < 0.0 {
                continue;
            }

            frame.fill_rectangle(
                Point::new(x, 0.0),
                Size::new(1.0, bounds.height),
                Color {
                    a: 0.35,
                    ..palette.background.strong.color
                },
            );

            frame.fill_text(Text {
                content: timecode(seconds),
                position: Point::new(x + 4.0, 4.0),
                color: palette.background.weak.text,
                size: 11.0.into(),
                ..Text::default()
            });
        }

        // Hover indicator.
        if let Some(point) = cursor.position_in(bounds) {
            frame.fill_rectangle(
                Point::new(point.x.round(), RULER_HEIGHT),
                Size::new(1.0, bounds.height - RULER_HEIGHT),
                Color {
                    a: 0.25,
                    ..palette.background.strong.color
                },
            );
        }

        // Playhead.
        let x = state.x_of(self.playhead as f32);

        if x >= 0.0 && x <= bounds.width {
            frame.fill_rectangle(
                Point::new(x.round() - 0.5, 0.0),
                Size::new(2.0, bounds.height),
                palette.danger.base.color,
            );

            let head = Path::new(|builder| {
                builder.move_to(Point::new(x - 6.0, 0.0));
                builder.line_to(Point::new(x + 6.0, 0.0));
                builder.line_to(Point::new(x, 9.0));
                builder.close();
            });

            frame.fill(&head, palette.danger.base.color);
        }

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

fn timecode(seconds: f64) -> String {
    let total = seconds.max(0.0);
    let minutes = (total / 60.0).floor() as u64;
    let rest = total - minutes as f64 * 60.0;

    if rest.fract().abs() < 0.01 {
        format!("{minutes}:{:02}", rest as u64)
    } else {
        format!("{minutes}:{rest:04.1}")
    }
}
