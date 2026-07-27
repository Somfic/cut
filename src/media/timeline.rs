use std::sync::Arc;

use crate::media::{Clip, Source, Track};

#[derive(Default)]
pub struct Timeline {
    pub tracks: Vec<Track>,
}

impl Timeline {
    pub fn single_track(clips: Vec<Clip>) -> Self {
        Timeline {
            tracks: vec![Track { clips }],
        }
    }

    pub fn sequence(sources: impl IntoIterator<Item = Arc<Source>>) -> Self {
        let mut position = 0;

        let clips = sources
            .into_iter()
            .map(|source| {
                let length = source.frame_count();
                let clip = Clip {
                    source,
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

    pub fn length(&self) -> usize {
        self.tracks
            .iter()
            .flat_map(|track| &track.clips)
            .map(|clip| clip.position + clip.length)
            .max()
            .unwrap_or(0)
    }

    pub fn clips_at(&self, frame: usize) -> Vec<&Clip> {
        self.tracks.iter().flat_map(|t| t.clip_at(frame)).collect()
    }
}
