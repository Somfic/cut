use cut_timeline::{Clip, Timeline};
use draad::{api, events, ty};
use std::sync::Arc;

use crate::state::State;

/// The document, flattened for the canvas. Sent per edit, not per frame.
#[ty]
pub struct TimelineDto {
    pub tracks: Vec<TrackDto>,
    pub length: usize,
}

#[ty]
pub struct TrackDto {
    pub clips: Vec<ClipDto>,
    /// Off plays nothing; the clips are still there and still editable.
    pub enabled: bool,
}

#[ty]
pub struct ClipDto {
    pub id: u64,
    pub position: usize,
    pub length: usize,
    pub source_start: usize,
    pub source_length: usize,
    pub name: String,
}

#[api(namespace = "timeline")]
pub trait TimelineApi {
    /// The document as it currently stands, or nothing before one opens.
    async fn get(&self) -> Option<TimelineDto>;
}

#[api]
impl TimelineApi for Arc<State> {
    async fn get(&self) -> Option<TimelineDto> {
        let timeline = self.session.timeline.lock().unwrap().clone()?;
        Some(dto(&timeline))
    }
}

pub fn dto(timeline: &Timeline) -> TimelineDto {
    TimelineDto {
        length: timeline.length(),
        tracks: timeline
            .tracks
            .iter()
            .map(|track| TrackDto {
                enabled: track.enabled,
                clips: track.clips.iter().map(clip_dto).collect(),
            })
            .collect(),
    }
}

fn clip_dto(clip: &Clip) -> ClipDto {
    ClipDto {
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
    }
}

/// Tell the canvas the document changed.
pub fn publish(state: &State, timeline: &Timeline) {
    state.emit(|events| events.timeline.emit_changed(&dto(timeline)));
}

/// Pushed when the document changes, so the canvas never has to poll for it.
#[events(namespace = "timeline")]
pub trait TimelineEvents {
    fn changed(payload: TimelineDto);
}
