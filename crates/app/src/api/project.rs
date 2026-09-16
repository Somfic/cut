//! Opening and saving the document. The format itself is the engine's —
//! this is the session's side of it: which file is open, whether what is in
//! memory matches it, and the two ways it gets written (⌘S and autosave).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::{Context, anyhow};
use cut_engine::project::{History, Timeline, file};
use draad::{api, events, ty};
use futures::channel::oneshot;
use tauri_plugin_dialog::DialogExt;

use crate::api::edit;
use crate::state::State;

/// The extension a project is saved under, and the only one Open offers.
const EXTENSION: &str = "cut";

/// What the title bar shows: the file, and whether there is work in it that
/// the disk does not have yet.
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

    /// Ask for a place to put it either way, and edit it as that file from
    /// here on.
    async fn save_as(&self) -> Result<(), String>;

    /// Replace the document with one off disk. Cancelling the picker is not
    /// an error — it is an answer of "never mind".
    async fn open(&self) -> Result<(), String>;
}

#[api]
impl ProjectApi for Arc<State> {
    async fn get(&self) -> ProjectDto {
        dto(self)
    }

    async fn save(&self) -> Result<(), String> {
        // Let go of the lock before any `await`: a dialog is up for as long
        // as the user leaves it up, and everything else here wants this lock.
        let path = self.session.project.lock().unwrap().path.clone();

        match path {
            Some(path) => write(self, &path).map_err(fault),
            None => save_somewhere(self).await,
        }
    }

    async fn save_as(&self) -> Result<(), String> {
        save_somewhere(self).await
    }

    async fn open(&self) -> Result<(), String> {
        let Some(path) = pick(self, Intent::Open).await else {
            return Ok(());
        };

        let timeline = file::load(&path).map_err(fault)?;
        adopt(self, Arc::new(timeline), path);
        Ok(())
    }
}

async fn save_somewhere(state: &Arc<State>) -> Result<(), String> {
    let Some(path) = pick(state, Intent::Save).await else {
        return Ok(());
    };

    write(state, &path).map_err(fault)
}

/// Serialise the document to `path` and take that as where it now lives —
/// so a Save As moves the session to the new file rather than leaving the
/// next ⌘S pointed at the old one.
pub fn write(state: &State, path: &Path) -> anyhow::Result<()> {
    // Cloned out of the lock: `file::save` probes nothing, but it does touch
    // the disk, and an edit arriving mid-write should not wait on it.
    let timeline = state
        .session
        .timeline
        .lock()
        .unwrap()
        .clone()
        .ok_or_else(|| anyhow!("no document is open"))?;

    file::save(&timeline, path).with_context(|| format!("could not save {}", path.display()))?;

    let mut project = state.session.project.lock().unwrap();
    project.path = Some(path.to_path_buf());
    project.saved = true;
    project.touched = None;
    drop(project);

    publish(state);
    Ok(())
}

/// Make `timeline` the document, as `path`. The undo stack goes with the
/// document it described — a version of the project you just closed is not
/// somewhere the one you just opened can go back to.
pub fn adopt(state: &State, timeline: Arc<Timeline>, path: PathBuf) {
    *state.session.timeline.lock().unwrap() = Some(timeline.clone());
    *state.session.history.lock().unwrap() = History::default();

    let mut project = state.session.project.lock().unwrap();
    project.path = Some(path);
    project.saved = true;
    project.touched = None;
    drop(project);

    edit::publish(state, timeline);
    publish(state);
}

/// The document the session starts on has arrived. `on_disk` is false for the
/// demo fallback, which is a document no file holds yet.
pub fn opened(state: &State, on_disk: bool) {
    if on_disk {
        let mut project = state.session.project.lock().unwrap();
        project.saved = true;
        project.touched = None;
        drop(project);

        publish(state);
    } else {
        touch(state);
    }
}

/// Note that the document has moved away from what is on disk. Every edit
/// says so, which is what both the dirty marker and autosave run on.
pub fn touch(state: &State) {
    let mut project = state.session.project.lock().unwrap();
    project.saved = false;
    project.touched = Some(Instant::now());
    drop(project);

    publish(state);
}

/// How long the edits have to stop for before autosave writes. Long enough
/// that a held arrow key is one save rather than thirty.
const AUTOSAVE_AFTER: Duration = Duration::from_secs(2);
const AUTOSAVE_POLL: Duration = Duration::from_millis(500);

/// Write the document out whenever the editing pauses, so a crash costs the
/// last couple of seconds rather than the session. Only ever writes where the
/// project already lives: autosave is not the thing that should be putting up
/// a picker.
pub fn spawn_autosave(state: Arc<State>) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(AUTOSAVE_POLL);

            let due = {
                let project = state.session.project.lock().unwrap();
                match (&project.path, project.saved, project.touched) {
                    (Some(path), false, Some(at)) if at.elapsed() >= AUTOSAVE_AFTER => {
                        Some(path.clone())
                    }
                    _ => None,
                }
            };

            if let Some(path) = due
                && let Err(e) = write(&state, &path)
            {
                eprintln!("autosave failed: {e:#}");
            }
        }
    });
}

/// On the way out, with no lull to wait for. Best effort: a failure here has
/// nowhere left to be reported to.
pub fn flush(state: &State) {
    let pending = {
        let project = state.session.project.lock().unwrap();
        project.path.clone().filter(|_| !project.saved)
    };

    if let Some(path) = pending
        && let Err(e) = write(state, &path)
    {
        eprintln!("could not save on the way out: {e:#}");
    }
}

pub fn dto(state: &State) -> ProjectDto {
    let project = state.session.project.lock().unwrap();

    ProjectDto {
        name: project
            .path
            .as_ref()
            .and_then(|path| path.file_name())
            .map(|name| name.to_string_lossy().into_owned())
            .unwrap_or_else(|| "Untitled".into()),
        path: project.path.as_ref().map(|path| path.display().to_string()),
        saved: project.saved,
    }
}

fn publish(state: &State) {
    if let Some(events) = state.events.get() {
        events.project.emit_changed(&dto(state));
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

/// Put a native panel up and wait for it. The dialog's answer arrives on a
/// callback, so it comes back through a channel rather than blocking the
/// thread this command is running on.
async fn pick(state: &State, intent: Intent) -> Option<PathBuf> {
    let handle = state.handle.get()?.clone();
    let (tx, rx) = oneshot::channel();

    let dialog = handle
        .dialog()
        .file()
        .add_filter("cut project", &[EXTENSION]);
    match intent {
        Intent::Open => dialog.pick_file(move |path| {
            tx.send(path).ok();
        }),
        Intent::Save => dialog
            .set_file_name(
                state
                    .session
                    .project
                    .lock()
                    .unwrap()
                    .path
                    .as_ref()
                    .and_then(|path| path.file_name())
                    .map(|name| name.to_string_lossy().into_owned())
                    .unwrap_or_else(|| format!("Untitled.{EXTENSION}")),
            )
            .save_file(move |path| {
                tx.send(path).ok();
            }),
    }

    // A cancelled panel answers with nothing, as does a dropped sender.
    rx.await.ok().flatten()?.into_path().ok()
}

fn fault(e: impl std::fmt::Display) -> String {
    format!("{e:#}")
}
