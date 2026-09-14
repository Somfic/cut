use iced::widget::canvas::{Frame, Path, Stroke, Text};
use iced::{Color, Point, Rectangle, Size, Theme, border, mouse};

use super::state::{Drag, State};
use super::{
    CLIP_RADIUS, RULER_HEIGHT, TICK_MIN_SPACING, TICK_STEPS, TRACK_HEIGHT, TimelineView,
};
use cut_engine::project::Edge;

impl<Message> TimelineView<'_, Message> {
    pub(super) fn draw_tracks(
        &self,
        frame: &mut Frame,
        state: &State,
        theme: &Theme,
        bounds: Rectangle,
    ) {
        let palette = theme.extended_palette();

        // One lane past the last, dimmer: somewhere to drop a clip to start a
        // track, and a hint that dropping there does something.
        for index in 0..=self.timeline.tracks.len() {
            let top = state.top_of(index);
            let empty = index == self.timeline.tracks.len();

            frame.fill_rectangle(
                Point::new(0.0, top),
                Size::new(bounds.width, TRACK_HEIGHT),
                match empty {
                    true => Color {
                        a: 0.4,
                        ..palette.background.weak.color
                    },
                    false => palette.background.weak.color,
                },
            );

            let Some(track) = self.timeline.tracks.get(index) else {
                continue;
            };

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

                let selected = self.selection == Some(clip.id);

                frame.fill(&body, palette.primary.weak.color);
                frame.stroke(
                    &body,
                    Stroke::default()
                        .with_color(match selected {
                            true => palette.primary.base.color,
                            false => palette.primary.strong.color,
                        })
                        .with_width(match selected {
                            true => 2.5,
                            false => 1.0,
                        }),
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
    }

    /// What the gesture in flight would do, drawn over the document it has not
    /// changed yet: where a carried clip would land, or where a trimmed end
    /// would come to. Red when it could not land there at all.
    pub(super) fn draw_drag(&self, frame: &mut Frame, state: &State, theme: &Theme) {
        let palette = theme.extended_palette();

        let (track, position, length, allowed) = match &state.drag {
            Some(Drag::Move {
                clip,
                length,
                track,
                position,
                ..
            }) => (
                *track,
                *position,
                *length,
                self.timeline.has_room(*track, *position, *length, *clip),
            ),
            Some(Drag::Trim {
                clip,
                track,
                edge,
                frame: at,
                anchor,
            }) => {
                let (position, end) = match edge {
                    Edge::In => (*at, *anchor),
                    Edge::Out => (*anchor, *at),
                };
                let length = end.saturating_sub(position);

                (
                    *track,
                    position,
                    length,
                    self.timeline.has_room(*track, position, length, *clip),
                )
            }
            _ => return,
        };

        let top = state.top_of(track);
        let ghost = Path::rounded_rectangle(
            Point::new(state.x_of(position as f32), top),
            Size::new(length as f32 * state.zoom, TRACK_HEIGHT),
            border::Radius::from(CLIP_RADIUS),
        );
        let colour = match allowed {
            true => palette.primary.base.color,
            false => palette.danger.base.color,
        };

        frame.fill(&ghost, Color { a: 0.25, ..colour });
        frame.stroke(
            &ghost,
            Stroke::default().with_color(colour).with_width(2.0),
        );
    }

    /// Ticks at an adaptive spacing: the coarsest step that still leaves at
    /// least `TICK_MIN_SPACING` pixels between labels at the current zoom.
    pub(super) fn draw_ruler(
        &self,
        frame: &mut Frame,
        state: &State,
        theme: &Theme,
        bounds: Rectangle,
    ) {
        let palette = theme.extended_palette();

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
    }

    pub(super) fn draw_cursor(
        &self,
        frame: &mut Frame,
        theme: &Theme,
        bounds: Rectangle,
        cursor: mouse::Cursor,
    ) {
        let palette = theme.extended_palette();

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
    }

    pub(super) fn draw_playhead(
        &self,
        frame: &mut Frame,
        state: &State,
        theme: &Theme,
        bounds: Rectangle,
    ) {
        let palette = theme.extended_palette();
        let x = state.x_of(self.playhead as f32);

        if x < 0.0 || x > bounds.width {
            return;
        }

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
