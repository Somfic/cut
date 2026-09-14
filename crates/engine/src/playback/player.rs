use crate::playback::{Decoder, SeekMode, Sinks, VideoStream};
use crate::project::Clip;
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::Ordering;
use std::time::Duration;

/// Plays a track
pub struct Player {
    sinks: Sinks,
    decoders: HashMap<PathBuf, Decoder>,
    live_source: Option<PathBuf>,
    live_clip: Option<Clip>,
}

impl Player {
    pub fn new(sinks: &Sinks) -> Self {
        Player {
            sinks: sinks.clone(),
            decoders: HashMap::new(),
            live_source: None,
            live_clip: None,
        }
    }

    /// The clip this track is currently playing, if any. Decoded frames carry
    /// source timestamps, so this is what maps them back onto the timeline.
    pub fn live_clip(&self) -> Option<&Clip> {
        self.live_clip.as_ref()
    }

    fn active_decoder(&self) -> Option<&Decoder> {
        self.live_source.as_ref().and_then(|f| self.decoders.get(f))
    }

    pub fn play(&self) {
        if let Some(d) = self.active_decoder() {
            d.play();
        }
    }

    pub fn pause(&self) {
        if let Some(d) = self.active_decoder() {
            d.pause();
        }
    }

    pub fn is_playing(&self) -> bool {
        self.sinks.audio_playing.load(Ordering::Relaxed)
    }

    /// Seek the live decoder, returning where it actually landed. Re-basing the
    /// master clock is the engine's job — one clock, one owner.
    pub fn seek(
        &self,
        frame: usize,
        video_sink: &mut VideoStream,
        mode: SeekMode,
    ) -> Option<Duration> {
        let d = self.active_decoder()?;
        Some(d.seek_to_frame(frame, video_sink, mode))
    }

    /// Play `clip` from timeline frame `tl_frame`. Within one source this is
    /// just a seek; across sources it swaps which decoder is live.
    pub fn cut_to(
        &mut self,
        clip: &Clip,
        tl_frame: usize,
        stream: &mut VideoStream,
        mode: SeekMode,
    ) -> anyhow::Result<Option<Duration>> {
        let source = clip.source.clone();
        let file = source.path.clone();
        self.live_clip = Some(clip.clone());

        // if different source, swap active decoder
        if self.live_source.as_ref() != Some(&file) {
            self.active_decoder().map(|d| d.pause());

            self.live_source = Some(file.clone());

            if !self.decoders.contains_key(&file) {
                let decoder = Decoder::for_source(
                    source,
                    self.sinks.audio_sink.clone(),
                    self.sinks.video_sink.clone(),
                )?;
                self.decoders.insert(file.clone(), decoder);
            }

            let decoder = &self.decoders[&file];
            if self.is_playing() {
                decoder.play();
            } else {
                decoder.pause();
            }
        }

        Ok(self.seek(clip.source_frame(tl_frame), stream, mode))
    }
}
