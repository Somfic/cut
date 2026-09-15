use std::sync::Arc;

use draad::{api, ty};

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
}
