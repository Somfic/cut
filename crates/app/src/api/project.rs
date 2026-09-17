//! The session's side of the project file: which one is open, whether what is
//! in memory matches it, and the two ways it gets written.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use cut_timeline::{History, Timeline, file};
use draad::{api, events, ty};
use futures::channel::oneshot;
use tauri_plugin_dialog::DialogExt;

use crate::api::edit;
use crate::state::{Project, State};

const EXTENSION: &str = "cut";
/// What a document that has never been saved is called.
const UNTITLED: &str = "Untitled.cut";

/// How long editing has to stop for before autosave writes: long enough that a
/// held arrow key is one save rather than thirty.
const AUTOSAVE_AFTER: Duration = Duration::from_secs(2);
const AUTOSAVE_POLL: Duration = Duration::from_millis(500);

/// What the title bar shows.
#[ty]
#[derive(Default, PartialEq)]
pub struct ProjectDto {
    /// Absent until the document has been saved somewhere.
    pub path: Option<String>,
    /// Just the file name, which is what a title bar has room for.
    pub name: String,
    pub saved: bool,
}

#[api(namespace = "project")]
pub trait ProjectApi {
    /// Which project is open, for the first paint. Changes arrive as events.
    async fn get(&self) -> ProjectDto;

    /// Write the document where it came from, asking for a place to put it
    /// when it has never been saved.
    async fn save(&self) -> Result<(), String>;

    /// Ask either way, and edit it as that file from here on.
    async fn save_as(&self) -> Result<(), String>;

    /// Replace the document with one off disk. Cancelling is not an error.
    async fn open(&self) -> Result<(), String>;
}

#[api]
impl ProjectApi for Arc<State> {
    async fn get(&self) -> ProjectDto {
        dto(self)
    }

    async fn save(&self) -> Result<(), String> {
        // Let go of the lock before any `await`: a dialog stays up as long as
        // the user leaves it up.
        let path = self.session.project.lock().unwrap().path.clone();

        save_to(self, path).await
    }

    async fn save_as(&self) -> Result<(), String> {
        save_to(self, None).await
    }

    async fn open(&self) -> Result<(), String> {
        let Some(path) = pick(self, Intent::Open).await else {
            return Ok(());
        };

        let timeline = file::load(&path, &|path| cut_media::probe_source(path)).map_err(fault)?;
        adopt(self, Arc::new(timeline), path);
        Ok(())
    }
}

/// Write to `path`, or wherever the user picks when there isn't one.
async fn save_to(state: &State, path: Option<PathBuf>) -> Result<(), String> {
    let path = match path {
        Some(path) => path,
        None => match pick(state, Intent::Save).await {
            Some(picked) => picked,
            None => return Ok(()),
        },
    };

    write(state, &path).map_err(fault)
}

/// Serialise the document to `path` and take that as where it now lives, so a
/// Save As moves the session rather than leaving the next ⌘S on the old file.
pub fn write(state: &State, path: &Path) -> anyhow::Result<()> {
    // Cloned out of the lock: an edit arriving mid-write should not wait on
    // the disk.
    let timeline = state
        .session
        .timeline
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow!("no document is open"))?;

    file::save(&timeline, path).with_context(|| format!("could not save {}", path.display()))?;

    set(state, |project| {
        project.path = Some(path.to_path_buf());
        matches_disk(project);
    });

    Ok(())
}

/// Make `timeline` the document, as `path`. The undo stack goes with the
/// document it described.
pub fn adopt(state: &State, timeline: Arc<Timeline>, path: PathBuf) {
    *state.session.timeline.lock().unwrap() = Some(timeline.clone());
    *state.session.history.lock().unwrap() = History::default();

    edit::publish(state, timeline);
    set(state, |project| {
        project.path = Some(path);
        matches_disk(project);
    });
}

/// The document the session starts on. `on_disk` is false for the demo
/// fallback, which no file holds yet — so autosave gives it one.
pub fn opened(state: &State, on_disk: bool) {
    if on_disk {
        set(state, matches_disk);
    } else {
        touch(state);
    }
}

/// Note that the document has moved away from the disk. Every edit says so,
/// which is what the dirty marker and autosave both run on.
pub fn touch(state: &State) {
    set(state, |project| {
        project.saved = false;
        project.last_edited = Some(Instant::now());
    });
}

/// Write whenever the editing pauses, so a crash costs seconds rather than the
/// session. Only ever where the project already lives: autosave should not be
/// the thing putting a picker up.
pub fn spawn_autosave(state: Arc<State>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(AUTOSAVE_POLL);

            if let Some(path) = pending(&state, AUTOSAVE_AFTER)
                && let Err(e) = write(&state, &path)
            {
                eprintln!("autosave failed: {e:#}");
            }
        }
    });
}

/// On the way out, with no lull to wait for and nowhere to report a failure.
pub fn flush(state: &State) {
    if let Some(path) = pending(state, Duration::ZERO)
        && let Err(e) = write(state, &path)
    {
        eprintln!("could not save on the way out: {e:#}");
    }
}

/// The file to write, once the editing has been stopped for `lull`.
fn pending(state: &State, lull: Duration) -> Option<PathBuf> {
    let project = state.session.project.lock().unwrap();
    let settled = project.last_edited.is_some_and(|at| at.elapsed() >= lull);

    project.path.clone().filter(|_| !project.saved && settled)
}

fn matches_disk(project: &mut Project) {
    project.saved = true;
    project.last_edited = None;
}

/// Change what the session knows about the file, and tell the front end.
fn set(state: &State, change: impl FnOnce(&mut Project)) {
    change(&mut state.session.project.lock().unwrap());
    state.emit(|events| events.project.emit_changed(&dto(state)));
}

pub fn dto(state: &State) -> ProjectDto {
    let project = state.session.project.lock().unwrap();

    ProjectDto {
        name: project
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| UNTITLED.into()),
        path: project.path.as_ref().map(|path| path.display().to_string()),
        saved: project.saved,
    }
}

#[events(namespace = "project")]
pub trait ProjectEvents {
    fn changed(payload: ProjectDto);
}

enum Intent {
    Open,
    Save,
}

/// Put a native panel up and wait for it. The answer arrives on a callback, so
/// it comes back through a channel rather than blocking this thread.
async fn pick(state: &State, intent: Intent) -> Option<PathBuf> {
    let handle = state.handle.get()?.clone();
    let (tx, rx) = oneshot::channel();

    let dialog = handle
        .dialog()
        .file()
        .add_filter("cut project", &[EXTENSION]);
    let answer = move |path| {
        tx.send(path).ok();
    };

    match intent {
        Intent::Open => dialog.pick_file(answer),
        Intent::Save => dialog.set_file_name(dto(state).name).save_file(answer),
    }

    // A cancelled panel answers with nothing, as does a dropped sender.
    rx.await.ok().flatten()?.into_path().ok()
}

fn fault(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}
