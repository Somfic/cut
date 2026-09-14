use std::cell::Cell;
use std::time::Instant;

/// Smoothing applied to each new sample. Low enough that the number is readable
/// rather than flickering, high enough to react to a stall within a few frames.
const SMOOTHING: f32 = 0.1;

/// How often something happens, in hertz, measured from the gaps between calls
/// to `tick`. Uses `Cell` so it can be sampled from `&self` — the UI rate has to
/// be counted in `view`, which never gets `&mut`.
#[derive(Default)]
pub struct Rate {
    last: Cell<Option<Instant>>,
    hz: Cell<f32>,
}

impl Rate {
    /// Record an occurrence, returning the current rate.
    pub fn tick(&self) -> f32 {
        let now = Instant::now();

        if let Some(previous) = self.last.replace(Some(now)) {
            let dt = (now - previous).as_secs_f32();

            if dt > 0.0 {
                let sample = 1.0 / dt;
                // Seed from the first real sample; averaging against the initial
                // zero would take a second to climb out of.
                let smoothed = match self.hz.get() {
                    0.0 => sample,
                    current => current + (sample - current) * SMOOTHING,
                };
                self.hz.set(smoothed);
            }
        }

        self.hz.get()
    }

    pub fn hz(&self) -> f32 {
        self.hz.get()
    }
}
