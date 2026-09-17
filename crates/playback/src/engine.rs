use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::{Player, Sinks, VideoStream};
use cut_media::SeekMode;
use cut_timeline::{Clip, Timeline};

/// Plays a timeline
pub struct Engine {
    sinks: Sinks,
    players: Vec<Player>,
    /// Frames come out of one video sink however many tracks play, so this
    /// is whose clip the last cut left them belonging to.
    leading: usize,
}

impl Engine {
    pub fn new() -> (Self, VideoStream) {
        let (sinks, video) = Sinks::new();
        let players = vec![Player::new(&sinks)];
        let engine = Engine {
            sinks,
            players,
            leading: 0,
        };
        (engine, video)
    }

    pub fn play(&self) {
        self.sinks.audio_playing.store(true, Ordering::Relaxed);
        for p in &self.players {
            p.play();
        }
    }

    pub fn pause(&self) {
        self.sinks.audio_playing.store(false, Ordering::Relaxed);
        for p in &self.players {
            p.pause();
        }
    }

    pub fn toggle(&self) {
        if self.is_playing() {
            self.pause()
        } else {
            self.play()
        }
    }
    pub fn is_playing(&self) -> bool {
        self.sinks.audio_playing.load(Ordering::Relaxed)
    }

    /// The flag the audio callback reads, handed out so observers share it
    /// rather than keep a copy that can drift.
    pub fn playing_flag(&self) -> Arc<AtomicBool> {
        self.sinks.audio_playing.clone()
    }

    pub fn position(&self) -> Duration {
        self.sinks.clock.position()
    }

    /// What a track is playing. Frames carry source time, so this is what
    /// maps one back onto the timeline.
    pub fn live_clip(&self, track: usize) -> Option<&Clip> {
        self.players.get(track).and_then(|p| p.live_clip())
    }

    /// Where the frames now arriving came from. Not always track 0, which
    /// may be empty there or switched off.
    pub fn leading_clip(&self) -> Option<&Clip> {
        self.live_clip(self.leading)
    }

    /// Every track cuts to whatever covers `tl_frame`. One playhead, so no
    /// track parameter: keeping them together is the engine's job.
    pub fn cut_to(
        &mut self,
        timeline: &Timeline,
        tl_frame: usize,
        stream: &mut VideoStream,
        mode: SeekMode,
    ) -> anyhow::Result<()> {
        while self.players.len() < timeline.tracks.len() {
            self.players.push(Player::new(&self.sinks));
        }

        let mut landed = None;
        for (index, (track, player)) in timeline.tracks.iter().zip(&mut self.players).enumerate() {
            // Off plays nothing, including the clip it is sitting on.
            if !track.enabled {
                player.pause();
                continue;
            }

            // A gap on this track: nothing to cut to, leave its decoder alone.
            let Some(clip) = track.clip_at(tl_frame) else {
                continue;
            };

            let at = player.cut_to(clip, tl_frame, stream, mode)?;

            // `cut_to` only sets decoder state when it swaps source, so a
            // track coming back on would otherwise stay paused.
            if self.sinks.audio_playing.load(Ordering::Relaxed) {
                player.play();
            }

            if landed.is_none() && at.is_some() {
                self.leading = index;
            }
            landed = landed.or(at);
        }

        // Re-base the clock and drop stale audio once, however many tracks
        // moved. It follows the first that landed until compositing gives the
        // timeline its own time base.
        if let Some(target) = landed {
            self.sinks.clock.seek_to(target);
            self.sinks.flush_audio.store(true, Ordering::Relaxed);
        }

        Ok(())
    }
}
