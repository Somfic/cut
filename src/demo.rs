use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::anyhow;

use crate::media::Source;
use crate::project::{Clip, Timeline};

pub const SOURCES: &[&str] = &[];
const N_CLIPS: usize = 400;
const CLIP_SECS: f64 = 2.0;

pub fn timeline() -> anyhow::Result<Timeline> {
    let mut sources = Vec::new();
    for path in SOURCES {
        match Source::new(*path) {
            Ok(source) => sources.push(Arc::new(source)),
            Err(e) => eprintln!("could not open {path}: {e}"),
        }
    }
    if sources.is_empty() {
        return Err(anyhow!("no sources could be opened"));
    }

    let mut rng = Rng::seeded();
    let mut clips = Vec::with_capacity(N_CLIPS);
    let mut position = 0;

    for i in 0..N_CLIPS {
        let source = sources[i % sources.len()].clone();
        let length = ((CLIP_SECS * source.fps).round() as usize).max(1);
        let max_start = source.frame_count().saturating_sub(length);

        clips.push(Clip {
            source,
            position,
            source_start: rng.below(max_start),
            length,
        });
        position += length;
    }

    Ok(Timeline::single_track(clips))
}

pub struct Rng(u64);

impl Rng {
    pub fn seeded() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Rng(seed | 1)
    }

    fn next_u64(&mut self) -> u64 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        x
    }

    pub fn below(&mut self, n: usize) -> usize {
        if n == 0 {
            0
        } else {
            (self.next_u64() % n as u64) as usize
        }
    }
}
