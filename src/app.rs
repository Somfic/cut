use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use futures_timer::Delay;
use iced::futures::channel::mpsc::Sender;
use iced::futures::{SinkExt, Stream};
use iced::widget::shader;
use iced::{
    Element, Subscription, Task,
    widget::{button, column, container, mouse_area, row, stack, text},
};

use crate::input::{self, Bindings};
use crate::media::{Frame, Source};
use crate::playback::{Controls, Request, SeekMode, transport};
use crate::project::{ClipId, Edit, History, Timeline, file};
use crate::rate::Rate;
use crate::ui::{Menu, MenuBar, TimelineView, VideoView};

const LAG_WARN_MS: i64 = 100;

/// How long a change may sit unwritten. The write only happens when the
/// document has actually moved on, so an idle editor never touches the disk.
const AUTOSAVE_INTERVAL: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct App {
    /// Where autosave writes, and where the transport looked for a project on
    /// startup.
    project: PathBuf,
    timeline: Arc<Timeline>,
    /// The document as it was last written. Compared by pointer: every change
    /// to a timeline replaces the `Arc` rather than mutating what is in it, so
    /// this is both the dirty flag and the record of what is on disk.
    saved: Option<Arc<Timeline>>,
    /// Set once a failed write has been reported, so a project that cannot be
    /// saved at all does not print the same error every couple of seconds.
    reported: bool,
    /// Versions of the document to go back to. Autosave writes a mistake to
    /// disk within a couple of seconds, so undo is what makes it safe.
    history: History,
    frame: Option<Arc<Frame>>,
    controls: Option<Controls>,
    bindings: Bindings,
    /// Which menu in the bar is showing, if any.
    menu: Option<usize>,
    /// Whether a file is being dragged over the window.
    dragging: bool,
    /// The clip a gesture or a menu is about.
    selection: Option<ClipId>,
    /// An open right-click menu: which clip, and where in the window to put it.
    context: Option<(ClipId, iced::Point)>,
    /// Redraws per second: counted in `view`, so it measures work the UI
    /// actually did rather than what it was asked to do.
    ui_rate: Rate,
    /// Frames per second arriving from the transport.
    video_rate: Rate,
}

#[derive(Clone)]
pub enum Event {
    Ready(Controls),
    Opened {
        timeline: Arc<Timeline>,
        /// False when this came from the demo fallback instead of the project
        /// file, which is what makes autosave write it out and gives the
        /// project a file to begin with.
        on_disk: bool,
    },
    Frame(Arc<Frame>),
    Autosave,
    /// Show this menu in the bar, or `None` to close what is open.
    Menu(Option<usize>),
    Import,
    Picked(Option<PathBuf>),
    Imported(Result<Arc<Source>, String>, Landing),
    DragOver(bool),
    Dropped(PathBuf),
    Undo,
    Redo,
    Select(ClipId),
    /// Carry out an edit the timeline worked out from a gesture.
    Apply(Edit),
    Context(ClipId, iced::Point),
    /// Delete the selected clip, closing the gap it leaves.
    DeleteSelection,
    /// Cut the selected clip in two at the playhead.
    SplitSelection,
    Request(Request),
    Keypress(iced::keyboard::Key, iced::keyboard::Modifiers),
}

/// Where imported media should land.
#[derive(Clone, Copy)]
pub enum Landing {
    /// After everything already in the document.
    End,
    /// Rippled in at this frame. What a drop aims at: the playhead, since the
    /// drop itself cannot say where it happened.
    At(usize),
}

impl From<Request> for Event {
    fn from(request: Request) -> Self {
        Event::Request(request)
    }
}

impl App {
    pub fn new(project: PathBuf) -> Self {
        let mut app = App {
            project,
            ..App::default()
        };

        // Nothing has been opened yet. Marking the empty document that stands in
        // for one as already saved keeps autosave from writing it over a project
        // that merely failed to load.
        app.saved = Some(app.timeline.clone());

        app
    }

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
        // Counted here because `view` runs exactly once per redraw.
        let counters = text(format!(
            "ui {:.0} fps · video {:.0} fps",
            self.ui_rate.tick(),
            self.video_rate.hz()
        ))
        .size(11)
        .color(iced::Color::from_rgb(0.6, 0.6, 0.6));

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

        // TODO: a toggle button for auto-following, so the view can be pinned
        // while playing — `self.auto_follow && self.playing()`.
        let timeline = TimelineView::new(&self.timeline, self.playhead(), self.fps())
            .follow_playhead(self.playing())
            .selection(self.selection)
            .on_seek(|frame| Request::Seek((frame, SeekMode::Accurate)).into())
            .on_select(Event::Select)
            .on_edit(Event::Apply)
            .on_context(Event::Context);

        // Overlaid on the preview's top-left rather than taking a row, so the
        // counters don't change the layout being measured.
        let preview = stack![
            preview,
            container(counters)
                .align_top(iced::Fill)
                .align_left(iced::Fill)
        ];

        let page = column![preview, container(controls).center_x(iced::Fill), timeline,]
            .spacing(10)
            .padding(10);

        // Something for the drag to land on, on the platforms that say a file is
        // overhead at all.
        let page: Element<'_, Event> = match self.dragging {
            true => stack![page, drop_hint()].into(),
            false => page.into(),
        };

        let page = MenuBar::new(Event::Menu)
            .menu("Media", vec![("Import…", Event::Import)])
            .open(self.menu)
            .view(page);

        // The right-click menu goes over even the bar, and a click anywhere
        // else puts it away without reaching what was clicked.
        let Some((clip, at)) = self.context else {
            return page;
        };

        stack![
            mouse_area(page).on_press(Event::Menu(None)),
            Menu::new(vec![
                ("Split at playhead", Event::Apply(Edit::Split { clip, frame: self.playhead() })),
                ("Delete", Event::Apply(Edit::Delete { clip, ripple: true })),
                ("Delete, leave a gap", Event::Apply(Edit::Delete { clip, ripple: false })),
            ])
            .at(at)
        ]
        .into()
    }

    pub fn update(&mut self, message: Event) -> Task<Event> {
        match message {
            Event::Ready(controls) => self.controls = Some(controls),
            Event::Opened { timeline, on_disk } => {
                self.saved = on_disk.then(|| timeline.clone());
                self.reported = false;
                self.history = History::default();
                self.timeline = timeline;
            }
            Event::Frame(f) => {
                self.video_rate.tick();
                self.frame = Some(f);
            }
            Event::Autosave => self.autosave(),
            Event::Menu(menu) => {
                self.menu = menu;
                self.context = None;
            }
            Event::Select(clip) => self.selection = Some(clip),
            Event::Context(clip, at) => {
                self.selection = Some(clip);
                self.context = Some((clip, at));
            }
            Event::Apply(edit) => {
                self.context = None;
                self.edit(edit);
            }
            Event::DeleteSelection => {
                if let Some(clip) = self.selection.take() {
                    self.context = None;
                    // Rippled, so deleting cannot leave a hole with nothing in
                    // it for the decoders to play.
                    self.edit(Edit::Delete { clip, ripple: true });
                }
            }
            Event::SplitSelection => {
                if let Some(clip) = self.selection {
                    self.context = None;
                    self.edit(Edit::Split {
                        clip,
                        frame: self.playhead(),
                    });
                }
            }
            Event::Import => {
                self.menu = None;
                return Task::perform(pick(), Event::Picked);
            }
            // The dialog was dismissed; nothing to import and nothing to say.
            Event::Picked(None) => {}
            Event::Picked(Some(path)) => {
                return Task::perform(probe(path), |source| {
                    Event::Imported(source, Landing::End)
                });
            }
            Event::DragOver(dragging) => self.dragging = dragging,
            Event::Dropped(path) => {
                self.dragging = false;
                // Where the playhead is *now*: by the time the probe finishes,
                // playback may have carried it somewhere else.
                let landing = Landing::At(self.playhead());

                return Task::perform(probe(path), move |source| {
                    Event::Imported(source, landing)
                });
            }
            Event::Imported(Ok(source), landing) => self.edit(match landing {
                Landing::End => Edit::Append(source),
                Landing::At(frame) => Edit::Insert { source, frame },
            }),
            Event::Imported(Err(e), _) => eprintln!("could not import {e}"),
            Event::Undo => {
                if let Some(timeline) = self.history.undo(self.timeline.clone()) {
                    self.publish(timeline);
                }
            }
            Event::Redo => {
                if let Some(timeline) = self.history.redo(self.timeline.clone()) {
                    self.publish(timeline);
                }
            }
            Event::Request(request) => self.with_controls(|c| c.send(request)),
            Event::Keypress(key, modifiers) => {
                let events = self.bindings.resolve(&key, modifiers);

                return Task::batch(events.into_iter().map(|event| self.update(event)));
            }
        }

        Task::none()
    }

    pub fn subscription(&self) -> Subscription<Event> {
        Subscription::batch([
            Subscription::run_with(self.project.clone(), transport),
            input::keylogger(),
            input::file_drops(),
            Subscription::run(autosave_ticks),
        ])
    }

    /// Carry out `edit`. What it produces is what the transport plays, what
    /// undo comes back to, and what autosave writes.
    fn edit(&mut self, edit: Edit) {
        let mut timeline = (*self.timeline).clone();

        if let Err(e) = timeline.apply(edit) {
            // The document is untouched, which for a gesture that has gone
            // somewhere impossible is the whole answer. The reason goes to the
            // log until there is somewhere in the window to show it.
            eprintln!("{e:#}");
            return;
        }

        self.history.record(self.timeline.clone());
        self.publish(Arc::new(timeline));

        // A clip that is no longer there cannot stay selected.
        if self
            .selection
            .is_some_and(|clip| self.clip_gone(clip))
        {
            self.selection = None;
        }
    }

    /// Whether `clip` has left the document.
    fn clip_gone(&self, clip: ClipId) -> bool {
        !self
            .timeline
            .tracks
            .iter()
            .flat_map(|track| &track.clips)
            .any(|existing| existing.id == clip)
    }

    /// Hand a document to the transport, and let autosave notice it.
    fn publish(&mut self, timeline: Arc<Timeline>) {
        self.timeline = timeline.clone();
        self.with_controls(|controls| controls.send(Request::Open(timeline)));
    }

    /// Write the document, if it has changed since the last write.
    fn autosave(&mut self) {
        if self
            .saved
            .as_ref()
            .is_some_and(|saved| Arc::ptr_eq(saved, &self.timeline))
        {
            return;
        }

        match file::save(&self.timeline, &self.project) {
            Ok(()) => {
                self.saved = Some(self.timeline.clone());
                self.reported = false;
                println!("saved {}", self.project.display());
            }
            // Left dirty on purpose: the next tick tries again, in case what
            // is in the way — a full disk, a volume that went away — clears.
            Err(e) => {
                if !self.reported {
                    eprintln!("could not save {}: {e:#}", self.project.display());
                    self.reported = true;
                }
            }
        }
    }

    fn with_controls(&mut self, f: impl FnOnce(&mut Controls)) {
        if let Some(controls) = &mut self.controls {
            f(controls);
        }
    }
}

/// Drives autosave. Built from `futures_timer` like the transport rather than
/// with `iced::time::every`, which would pull in an async-runtime feature
/// nothing else here needs.
fn autosave_ticks() -> impl Stream<Item = Event> {
    iced::stream::channel(1, async |mut output: Sender<Event>| {
        loop {
            Delay::new(AUTOSAVE_INTERVAL).await;

            if output.send(Event::Autosave).await.is_err() {
                break;
            }
        }
    })
}

/// Ask for a file to import. Runs as a task so the window keeps drawing while
/// the dialog is up.
async fn pick() -> Option<PathBuf> {
    rfd::AsyncFileDialog::new()
        .set_title("Import media")
        .add_filter("Media", &["mp4", "mov", "m4v", "mkv", "webm", "avi"])
        .pick_file()
        .await
        .map(|file| file.path().to_path_buf())
}

/// Opening a file with gstreamer to measure it takes long enough to be worth
/// keeping off the update loop.
async fn probe(path: PathBuf) -> Result<Arc<Source>, String> {
    Source::new(&path)
        .map(Arc::new)
        .map_err(|e| format!("{}: {e:#}", path.display()))
}

/// Shown while a file is over the window. It names the target because a drop
/// carries no position of its own — the pointer is not where it lands.
fn drop_hint<'a>() -> Element<'a, Event> {
    let label = container(text("Drop to insert at the playhead").size(14))
        .padding([10, 16])
        .style(|theme: &iced::Theme| {
            let palette = theme.extended_palette();

            container::Style {
                background: Some(palette.background.weak.color.into()),
                border: iced::border::rounded(6)
                    .color(palette.primary.base.color)
                    .width(1),
                ..container::Style::default()
            }
        });

    container(label).center_x(iced::Fill).center_y(iced::Fill).into()
}
