//! Scaffolding for the demo project the app opens on launch. None of this is
//! real editor behaviour — delete this module once projects can be loaded.

use std::time::{SystemTime, UNIX_EPOCH};

pub const SOURCES: &[&str] = &["/Users/lucas/Downloads/pizza.mp4"];

/// Lay this many clips of the one source, each this long, from random moments
/// in the file.
pub const N_CLIPS: usize = 400;
pub const CLIP_SECS: f64 = 2.0;

/// Tiny xorshift PRNG — enough to scatter clip start points, no dependency.
pub struct Rng(u64);

impl Rng {
    pub fn seeded() -> Self {
        let seed = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Rng(seed | 1) // xorshift needs nonzero state
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
