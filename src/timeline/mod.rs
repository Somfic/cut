use std::sync::Arc;

use crate::media::Source;

#[derive(Default)]
pub struct Timeline {
    pub tracks: Vec<Track>,
}

pub struct Track {
    pub clips: Vec<Clip>,
}

pub struct Clip {
    pub source: Arc<Source>,
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

    pub fn active_clips(&self, frame: usize) -> Vec<&Clip> {
        self.tracks
            .iter()
            .flat_map(|t| &t.clips)
            .filter(|c| c.position <= frame && frame < (c.position + c.length))
            .collect()
    }
}
