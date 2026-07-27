use std::sync::Arc;

use iced::widget::shader;
use iced::{
    Element, Subscription,
    widget::{button, column, container, row, text},
};

use crate::input::{self, Bindings};
use crate::media::Frame;
use crate::playback::{Controls, Request, SeekMode, transport};
use crate::project::Timeline;
use crate::ui::{TimelineView, VideoView};

const LAG_WARN_MS: i64 = 100;

#[derive(Default)]
pub struct App {
    timeline: Arc<Timeline>,
    frame: Option<Arc<Frame>>,
    controls: Option<Controls>,
    bindings: Bindings,
}

#[derive(Clone)]
pub enum Event {
    Ready(Controls),
    Opened(Arc<Timeline>),
    Frame(Arc<Frame>),
    Request(Request),
    Keypress(iced::keyboard::Key, iced::keyboard::Modifiers),
}

impl From<Request> for Event {
    fn from(request: Request) -> Self {
        Event::Request(request)
    }
}

impl App {
    fn fps(&self) -> f64 {
        self.frame
            .as_ref()
            .map(|f| f.fps.numer() as f64 / f.fps.denom() as f64)
            .filter(|fps| *fps > 0.0)
            .unwrap_or(30.0)
    }

    fn playhead(&self) -> usize {
        self.controls.as_ref().map_or(0, Controls::playhead)
    }

    fn playing(&self) -> bool {
        self.controls.as_ref().is_some_and(Controls::is_playing)
    }

    fn lag_ms(&self) -> i64 {
        self.controls.as_ref().map_or(0, Controls::lag_ms)
    }

    pub fn view(&self) -> Element<'_, Event> {
        let preview: Element<'_, Event> = match &self.frame {
            Some(frame) => shader(VideoView {
                frame: Some(frame.clone()),
            })
            .width(iced::Fill)
            .height(iced::Fill)
            .into(),
            None => container(text("decoding…"))
                .center_x(iced::Fill)
                .center_y(iced::Fill)
                .into(),
        };

        let lag = self.lag_ms();
        let mut controls = row![
            button(if self.playing() { "Pause" } else { "Play" })
                .on_press(Request::TogglePlayback.into()),
        ]
        .spacing(10)
        .align_y(iced::Center);

        if self.playing() && lag > LAG_WARN_MS {
            controls = controls.push(
                text(format!("⚠ {lag} ms behind")).color(iced::Color::from_rgb(0.9, 0.25, 0.25)),
            );
        }

        let timeline = TimelineView::new(&self.timeline, self.playhead(), self.fps())
            .on_seek(|frame| Request::Seek((frame, SeekMode::Accurate)).into());

        column![preview, container(controls).center_x(iced::Fill), timeline,]
            .spacing(10)
            .padding(10)
            .into()
    }

    pub fn update(&mut self, message: Event) {
        match message {
            Event::Ready(controls) => self.controls = Some(controls),
            Event::Opened(timeline) => self.timeline = timeline,
            Event::Frame(f) => self.frame = Some(f),
            Event::Request(request) => self.with_controls(|c| c.send(request)),
            Event::Keypress(key, modifiers) => {
                for event in self.bindings.resolve(&key, modifiers) {
                    self.update(event);
                }
            }
        }
    }

    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::batch([Subscription::run(transport), input::keylogger()])
    }

    fn with_controls(&mut self, f: impl FnOnce(&mut Controls)) {
        if let Some(controls) = &mut self.controls {
            f(controls);
        }
    }
}
