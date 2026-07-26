use std::sync::Arc;

use crate::video::Video;

mod widget;

pub use widget::timeline;

#[derive(Default)]
pub struct Timeline {
    pub tracks: Vec<Track>,
}

pub struct Track {
    pub clips: Vec<Clip>,
}

pub struct Clip {
    pub source: Arc<Video>,
    pub position: usize,
    pub source_start: usize,
    pub length: usize,
}

impl Clip {
    pub fn source_frame(&self, frame: usize) -> usize {
        self.source_start + (frame - self.position)
    }
}

impl Timeline {
    /// Each source in full, laid end to end on a single track.
    pub fn sequence(sources: impl IntoIterator<Item = Arc<Video>>) -> Self {
        let mut position = 0;

        let clips = sources
            .into_iter()
            .map(|video| {
                let length = video.frame_count().unwrap_or(0);
                let clip = Clip {
                    source: video,
                    position,
                    source_start: 0,
                    length,
                };

                position += length;
                clip
            })
            .collect();

        Timeline {
            tracks: vec![Track { clips }],
        }
    }

    /// The source currently driving playback — source zero, the first clip
    /// (see `video_worker`, which only plays that one for now).
    pub fn playing_video(&self) -> Option<&Arc<Video>> {
        self.tracks.first()?.clips.first().map(|clip| &clip.source)
    }

    /// Length in frames, i.e. where the last clip ends.
    pub fn length(&self) -> usize {
        self.tracks
            .iter()
            .flat_map(|track| &track.clips)
            .map(|clip| clip.position + clip.length)
            .max()
            .unwrap_or(0)
    }

    pub fn active_clips(&self, frame: usize) -> Vec<&Clip> {
        self.tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| c.position <= frame && frame < (c.position + c.length))
            .collect()
    }
}
