//! Keeping the view on the playhead.
//!
//! While playback runs, the playhead holds still on screen and the timeline
//! slides under it. Where it holds is wherever it happened to be when playback
//! started, clamped into `BAND` — so starting from the middle keeps the middle,
//! and starting from off the edge brings it just inside first.
//!
//! Three states: `Manual` after the user takes the view by hand, `Chasing`
//! while a spring brings the playhead to the position it will hold, and
//! `Pinned` once it's there.

use std::time::Instant;

/// The playhead can hold anywhere between these fractions of the width. Outside
/// them there's too little timeline on one side to be worth looking at.
const BAND: (f32, f32) = (0.2, 0.8);
/// Spring constant pulling the view into position, in 1/s². Higher snaps into
/// the pin faster; lower eases in.
const STIFFNESS: f32 = 30.0;
/// Damping relative to critical: 1.0 glides to a stop dead on target, lower
/// coasts past and drifts back. Near critical so the approach doesn't ring.
const DAMPING: f32 = 0.95;
/// Close enough to pin, in *pixels* — the unit the eye judges. In frames it
/// would mean a tighter tolerance zoomed out than zoomed in, which is backwards.
const SETTLE: f32 = 1.0;
/// And how closely the view has to match the playhead's own speed, in pixels
/// per second, before it can stop springing and start tracking exactly.
const SETTLE_SPEED: f32 = 40.0;
/// Longest step the integrator will take. A stalled frame shouldn't launch the
/// viewport across the timeline.
const MAX_STEP: f32 = 1.0 / 15.0;
/// A playhead move larger than this many frames is a seek, not playback.
const JUMP: usize = 8;
/// Smoothing on the playhead's measured speed, applied once per frame arrival.
const SMOOTHING: f32 = 0.2;
/// How long the playhead can sit still before playback counts as stopped.
const STOPPED_AFTER: f32 = 0.2;

#[derive(Clone, Copy)]
enum Mode {
    /// The user has taken the view; leave it alone until the playhead moves.
    Manual,
    /// Springing until the playhead reaches `fraction`. Velocity only exists
    /// here — there is no such thing as the view's momentum while its position
    /// is a function of the playhead's.
    Chasing { velocity: f32, fraction: f32 },
    /// Holding the playhead at `fraction` of the width: scroll is derived from
    /// it exactly, so the playhead is pixel-static and only the timeline moves.
    Pinned { fraction: f32 },
}

pub struct Follow {
    mode: Mode,
    speed: Speed,
    last_redraw: Option<Instant>,
}

impl Default for Follow {
    fn default() -> Self {
        Follow {
            mode: Mode::Manual,
            speed: Speed::default(),
            last_redraw: None,
        }
    }
}

impl Follow {
    /// Advance one redraw, moving `scroll`. Returns whether the view is still
    /// animating — which is also whether another redraw is needed.
    pub fn advance(
        &mut self,
        playhead: usize,
        scroll: &mut f32,
        zoom: f32,
        width: f32,
        now: Instant,
    ) -> bool {
        let dt = self
            .last_redraw
            .map_or(0.0, |previous| (now - previous).as_secs_f32())
            .min(MAX_STEP);
        self.last_redraw = Some(now);

        let movement = self.speed.observe(playhead, now);

        // Taking over from the user: hold the playhead where it already is,
        // pulled inside the band if it's sitting out at an edge.
        if let Mode::Manual = self.mode
            && !matches!(movement, Movement::Still)
            && width > 0.0
        {
            let fraction = ((playhead as f32 - *scroll) * zoom / width).clamp(BAND.0, BAND.1);
            self.mode = Mode::Chasing {
                velocity: 0.0,
                fraction,
            };
        }

        let (velocity, fraction) = match self.mode {
            Mode::Manual => return false,
            Mode::Pinned { fraction } => {
                *scroll = target(playhead, fraction, zoom, width);
                return false;
            }
            Mode::Chasing { velocity, fraction } => (velocity, fraction),
        };

        let target = target(playhead, fraction, zoom, width);
        let offset = target - *scroll;

        // Damping resists motion *relative to the playhead*, not motion itself.
        // Against zero, a chase can only balance pull against drag at a standing
        // lag of `speed · c / k` frames — a constant in frames, so it grows with
        // zoom in pixels.
        //
        // Except when the target is pinned at the start of the timeline: there
        // the view genuinely isn't meant to be moving, and leading the playhead
        // would push it off the left edge.
        let leading = if target > 0.0 {
            self.speed.per_second()
        } else {
            0.0
        };
        let relative = velocity - leading;

        // Both tolerances in pixels, so arriving doesn't get harder the further
        // you zoom in.
        if offset.abs() * zoom < SETTLE && relative.abs() * zoom < SETTLE_SPEED {
            *scroll = target;
            self.mode = Mode::Pinned { fraction };
            return false;
        }

        // Critical damping is 2·√k; scaling it by DAMPING decides whether the
        // view settles dead on target or coasts past it.
        let damping = 2.0 * STIFFNESS.sqrt() * DAMPING;
        let velocity = velocity + (offset * STIFFNESS - relative * damping) * dt;

        *scroll = (*scroll + velocity * dt).max(0.0);
        self.mode = Mode::Chasing { velocity, fraction };

        true
    }

    /// Hand the view back to the user: stop following until the playhead moves
    /// again, at which point it holds wherever it is by then. Dropping the
    /// velocity with it is the point — a glide left running would fight the
    /// gesture that interrupted it.
    pub fn release(&mut self) {
        self.mode = Mode::Manual;
        // The speed estimate is about to go stale: whatever moves the playhead
        // next may be a scrub or a resume after a long pause, and measuring
        // across that gap would say playback is crawling.
        self.speed.forget();
    }
}

/// The scroll that puts the playhead at `fraction` of the width. Clamped at
/// zero: the timeline doesn't scroll into empty space before frame 0, so near
/// the start the playhead sits left of where it will hold.
fn target(playhead: usize, fraction: f32, zoom: f32, width: f32) -> f32 {
    (playhead as f32 - width * fraction / zoom).max(0.0)
}

enum Movement {
    Still,
    Stepped,
    Jumped,
}

/// How fast the playhead is advancing, in frames per second.
///
/// Sampled between *changes* rather than between redraws: the playhead only
/// moves when a frame arrives, so per-redraw sampling would read zero, zero,
/// zero, then a frame across a millisecond — noise that lands straight in the
/// damping term.
#[derive(Default)]
struct Speed {
    last: Option<(usize, Instant)>,
    per_second: f32,
}

impl Speed {
    fn observe(&mut self, playhead: usize, now: Instant) -> Movement {
        let Some((previous, at)) = self.last else {
            self.last = Some((playhead, now));
            return Movement::Still;
        };

        if playhead == previous {
            // Standing still for longer than a few frames: playback has
            // stopped, so bleed the estimate away rather than letting a stale
            // speed keep pushing the view.
            if (now - at).as_secs_f32() > STOPPED_AFTER {
                self.per_second = 0.0;
            }
            return Movement::Still;
        }

        let jumped = playhead.abs_diff(previous) > JUMP;
        let elapsed = (now - at).as_secs_f32();

        // A seek isn't travel — it teleports — so it says nothing about how
        // fast playback is running.
        let sample = if jumped || elapsed <= 0.0 {
            0.0
        } else {
            (playhead as f32 - previous as f32) / elapsed
        };

        self.per_second += (sample - self.per_second) * SMOOTHING;
        self.last = Some((playhead, now));

        if jumped {
            Movement::Jumped
        } else {
            Movement::Stepped
        }
    }

    fn per_second(&self) -> f32 {
        self.per_second
    }

    /// Drop the baseline, so the next observation starts a fresh measurement
    /// instead of averaging across a gap.
    fn forget(&mut self) {
        self.last = None;
        self.per_second = 0.0;
    }
}
