//! What the page can ask for. Nothing here is on the frame path.

use std::sync::Arc;
use std::sync::atomic::Ordering;

use cut_engine::playback::{Controls, Request, SeekMode};

use crate::state::{Rect, Shared};

/// The document, flattened for the canvas. Sent per edit, not per frame.
#[derive(serde::Serialize)]
pub struct TimelineDto {
    pub tracks: Vec<TrackDto>,
    pub length: usize,
}

#[derive(serde::Serialize)]
pub struct TrackDto {
    pub clips: Vec<ClipDto>,
}

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipDto {
    pub id: u64,
    pub position: usize,
    pub length: usize,
    pub source_start: usize,
    pub source_length: usize,
    pub name: String,
}

#[tauri::command]
pub fn timeline(shared: tauri::State<'_, Arc<Shared>>) -> Option<TimelineDto> {
    let timeline = shared.timeline.lock().unwrap().clone()?;

    Some(TimelineDto {
        length: timeline.length(),
        tracks: timeline
            .tracks
            .iter()
            .map(|track| TrackDto {
                clips: track
                    .clips
                    .iter()
                    .map(|clip| ClipDto {
                        id: clip.id.raw(),
                        position: clip.position,
                        length: clip.length,
                        source_start: clip.source_start,
                        source_length: clip.source.frame_count(),
                        name: clip
                            .source
                            .path
                            .file_name()
                            .map(|name| name.to_string_lossy().into_owned())
                            .unwrap_or_default(),
                    })
                    .collect(),
            })
            .collect(),
    })
}

/// A correction, not a feed: the page runs its own clock between polls.
#[derive(serde::Serialize)]
pub struct TransportDto {
    pub playhead: usize,
    pub playing: bool,
    pub fps: f64,
}

#[tauri::command]
pub fn transport(shared: tauri::State<'_, Arc<Shared>>) -> TransportDto {
    let controls = shared.controls.lock().unwrap();

    TransportDto {
        playhead: controls.as_ref().map_or(0, Controls::playhead),
        playing: controls.as_ref().is_some_and(Controls::is_playing),
        // Never zero: the page divides by it, and the first poll can land
        // before any frame has.
        fps: shared.fps.lock().unwrap().max(24.0),
    }
}

#[tauri::command]
pub fn seek(shared: tauri::State<'_, Arc<Shared>>, frame: usize) {
    if let Some(controls) = shared.controls.lock().unwrap().as_mut() {
        controls.send(Request::Seek((frame, SeekMode::Accurate)));
    }
}

#[derive(serde::Serialize)]
pub struct Stats {
    pub frames: u64,
    pub presents: u64,
    pub dropped: u64,
}

#[tauri::command]
pub fn stats(shared: tauri::State<'_, Arc<Shared>>) -> Stats {
    Stats {
        frames: shared.frames.load(Ordering::Relaxed),
        presents: shared.presents.load(Ordering::Relaxed),
        dropped: shared.dropped.load(Ordering::Relaxed),
    }
}

#[tauri::command]
pub fn toggle_playback(shared: tauri::State<'_, Arc<Shared>>) {
    toggle(&shared);
}

pub fn toggle(shared: &Shared) {
    match shared.controls.lock().unwrap().as_mut() {
        Some(controls) => controls.send(Request::TogglePlayback),
        None => eprintln!("toggle: no controls yet"),
    }
}

#[tauri::command]
pub fn set_chrome(shared: tauri::State<'_, Arc<Shared>>, r: f64, g: f64, b: f64) {
    *shared.chrome.lock().unwrap() = Some((r, g, b));
}

#[tauri::command]
#[allow(clippy::too_many_arguments)]
pub fn set_video_rect(
    shared: tauri::State<'_, Arc<Shared>>,
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    page_width: f32,
    page_height: f32,
) {
    *shared.rect.lock().unwrap() = Some(Rect {
        x,
        y,
        width,
        height,
    });
    *shared.page.lock().unwrap() = Some((page_width, page_height));
}
