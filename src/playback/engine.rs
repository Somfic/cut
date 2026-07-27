use std::{sync::atomic::Ordering, time::Duration};

use crate::{
    playback::{Player, SeekMode, Sinks, VideoStream},
    media::{Clip, Timeline},
};

/// Plays a timeline
pub struct Engine {
    sinks: Sinks,
    players: Vec<Player>,
}

impl Engine {
    pub fn new() -> (Self, VideoStream) {
        let (sinks, video) = Sinks::new();
        let players = vec![Player::new(&sinks)];
        let engine = Engine { sinks, players };
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

    pub fn position(&self) -> Duration {
        self.sinks.clock.position()
    }

    /// What a given track is currently playing. Frames arrive stamped with
    /// source time, so the caller needs this to map one back onto the timeline.
    pub fn live_clip(&self, track: usize) -> Option<&Clip> {
        self.players.get(track).and_then(|p| p.live_clip())
    }

    /// Move the whole timeline to `tl_frame`: every track cuts to whichever clip
    /// covers that position. There's one playhead, so there's no track
    /// parameter — keeping the tracks together is the engine's job.
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
        for (track, player) in timeline.tracks.iter().zip(&mut self.players) {
            // A gap on this track: nothing to cut to, leave its decoder alone.
            let Some(clip) = track.clip_at(tl_frame) else {
                continue;
            };
            landed = landed.or(player.cut_to(clip, tl_frame, stream, mode)?);
        }

        // Re-base the master clock and drop stale audio exactly once, however
        // many tracks moved. Frame times are still source-relative, so the clock
        // follows the first track that landed — revisit when compositing gives
        // the timeline its own time base.
        if let Some(target) = landed {
            self.sinks.clock.seek_to(target);
            self.sinks.flush_audio.store(true, Ordering::Relaxed);
        }

        Ok(())
    }
}
