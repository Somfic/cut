use std::sync::Arc;

use iced::futures::channel::mpsc::Sender;
use iced::widget::shader;
use iced::{
    Element, Subscription,
    widget::{button, column, container, row, text},
};

use crate::media::Frame;
use crate::playback::{Command, SeekMode, worker};
use crate::timeline::Timeline;
use crate::ui::shortcuts::{self, Action};
use crate::ui::{VideoView, timeline::timeline};

const LAG_WARN_MS: i64 = 100;

#[derive(Default)]
pub struct Screen {
    timeline: Arc<Timeline>,
    frame: Option<Arc<Frame>>,
    commands: Option<Sender<Command>>,
    playing: bool,
    /// Latest playback lag reported by the worker (ms behind the clock).
    lag_ms: i64,
    /// Timeline frame the worker is currently presenting.
    playhead: usize,
}

#[derive(Clone)]
pub enum Message {
    Ready(Sender<Command>),
    Opened(Arc<Timeline>),
    Frame(Arc<Frame>),
    Lag(i64),
    Playhead(usize),
    Shortcut(Action),
    TogglePlayback,
    Seek((i64, SeekMode)),
    SeekTo((usize, SeekMode)),
}

impl Screen {
    fn fps(&self) -> f64 {
        self.frame
            .as_ref()
            .map(|f| f.fps.numer() as f64 / f.fps.denom() as f64)
            .filter(|fps| *fps > 0.0)
            .unwrap_or(30.0)
    }

    fn playhead(&self) -> usize {
        self.playhead
    }

    pub fn view(&self) -> Element<'_, Message> {
        let preview: Element<'_, Message> = match &self.frame {
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

        let lag = self.lag_ms;
        let mut controls = row![
            button(if self.playing { "Pause" } else { "Play" }).on_press(Message::TogglePlayback),
        ]
        .spacing(10)
        .align_y(iced::Center);

        // Surface playback falling behind the audio clock — but only while
        // playing, since a paused clock freezes the lag at its last value.
        if self.playing && lag > LAG_WARN_MS {
            controls = controls.push(
                text(format!("⚠ {lag} ms behind")).color(iced::Color::from_rgb(0.9, 0.25, 0.25)),
            );
        }

        let timeline = timeline(&self.timeline, self.playhead(), self.fps())
            .on_seek(|frame| Message::SeekTo((frame, SeekMode::Accurate)));

        column![preview, container(controls).center_x(iced::Fill), timeline,]
            .spacing(10)
            .padding(10)
            .into()
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Ready(tx) => self.commands = Some(tx),
            // The worker starts playback as soon as it has opened the file.
            Message::Opened(timeline) => {
                self.timeline = timeline;
                self.playing = true;
            }
            Message::Shortcut(action) => match action {
                Action::TogglePlayback => self.update(Message::TogglePlayback),
                Action::Step(delta) => self.update(Message::Seek((delta, SeekMode::Accurate))),
                Action::GoToStart => self.update(Message::SeekTo((0, SeekMode::Accurate))),
            },
            Message::Frame(f) => self.frame = Some(f),
            Message::Lag(ms) => self.lag_ms = ms,
            Message::Playhead(frame) => self.playhead = frame,
            Message::TogglePlayback => {
                self.playing = !self.playing;
                self.send(Command::TogglePlayback);
            }
            Message::Seek((delta, mode)) => self.send(Command::Seek((delta, mode))),
            Message::SeekTo((frame, mode)) => self.send(Command::SeekTo((frame, mode))),
        }
    }

    pub fn subscription(&self) -> Subscription<Message> {
        Subscription::batch([
            Subscription::run(worker),
            shortcuts::subscription().map(Message::Shortcut),
        ])
    }

    fn send(&mut self, cmd: Command) {
        if let Some(tx) = &mut self.commands {
            let _ = tx.try_send(cmd);
        }
    }
}
