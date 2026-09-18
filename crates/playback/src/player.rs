use std::{
    sync::{
        Arc,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};

use crate::{Outputs, TrackPlayer, VideoStream};
use cut_audio::Output;
use cut_media::SeekMode;
use cut_timeline::{Clip, Timeline};

/// Plays a timeline: one playhead, however many tracks. It owns the clock and
/// the output device, and each track's decoders belong to a [`TrackPlayer`].
pub struct Player {
    /// The device, the clock it advances, and the flags the callback reads.
    output: Output,
    outputs: Outputs,
    tracks: Vec<TrackPlayer>,
    /// Frames come out of one video sink however many tracks play, so this
    /// is whose clip the last cut left them belonging to.
    leading: usize,
}

impl Player {
    pub fn new() -> (Self, VideoStream) {
        let output = Output::start();
        let (outputs, video) = Outputs::new(output.sink.clone());
        let tracks = vec![TrackPlayer::new(&outputs)];
        let player = Player {
            output,
            outputs,
            tracks,
            leading: 0,
        };

        (player, video)
    }

    pub fn play(&self) {
        self.output.playing.store(true, Ordering::Relaxed);
        for t in &self.tracks {
            t.play();
        }
    }

    pub fn pause(&self) {
        self.output.playing.store(false, Ordering::Relaxed);
        for t in &self.tracks {
            t.pause();
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
        self.output.playing.load(Ordering::Relaxed)
    }

    /// The flag the audio callback reads, handed out so observers share it
    /// rather than keep a copy that can drift.
    pub fn playing_flag(&self) -> Arc<AtomicBool> {
        self.output.playing.clone()
    }

    pub fn position(&self) -> Duration {
        self.output.clock.position()
    }

    /// What a track is playing. Frames carry source time, so this is what
    /// maps one back onto the timeline.
    pub fn live_clip(&self, track: usize) -> Option<&Clip> {
        self.tracks.get(track).and_then(|t| t.live_clip())
    }

    /// Where the frames now arriving came from. Not always track 0, which
    /// may be empty there or switched off.
    pub fn leading_clip(&self) -> Option<&Clip> {
        self.live_clip(self.leading)
    }

    /// Every track cuts to whatever covers `tl_frame`. One playhead, so no
    /// track parameter: keeping them together is this player's job.
    pub fn cut_to(
        &mut self,
        timeline: &Timeline,
        tl_frame: usize,
        stream: &mut VideoStream,
        mode: SeekMode,
    ) -> anyhow::Result<()> {
        while self.tracks.len() < timeline.tracks.len() {
            self.tracks.push(TrackPlayer::new(&self.outputs));
        }

        let playing = self.is_playing();
        let mut landed = None;

        for (index, (track, player)) in timeline.tracks.iter().zip(&mut self.tracks).enumerate() {
            // Off plays nothing, including the clip it is sitting on.
            if !track.enabled {
                player.pause();
                continue;
            }

            // A gap on this track: nothing to cut to, leave its decoder alone.
            let Some(clip) = track.clip_at(tl_frame) else {
                continue;
            };

            let at = player.cut_to(clip, tl_frame, stream, mode, playing)?;

            // `cut_to` only sets decoder state when it swaps source, so a
            // track coming back on would otherwise stay paused.
            if playing {
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
            self.output.clock.seek_to(target);
            self.output.flush.store(true, Ordering::Relaxed);
        }

        Ok(())
    }
}
