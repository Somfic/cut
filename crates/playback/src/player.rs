use crate::{Sinks, VideoStream};
use cut_media::{Decoder, SeekMode};
use cut_timeline::Clip;
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

    /// The clip this track is playing, if any.
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

    /// Seek the live decoder to where it lands. The clock has one owner, and
    /// it is the engine.
    pub fn seek(
        &self,
        frame: usize,
        video_sink: &mut VideoStream,
        mode: SeekMode,
    ) -> Option<Duration> {
        let d = self.active_decoder()?;
        let landed = d.seek_to_frame(frame, mode);
        // Everything already decoded is from before the seek.
        video_sink.drain();

        Some(landed)
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
            if let Some(d) = self.active_decoder() {
                d.pause();
            }

            self.live_source = Some(file.clone());

            if !self.decoders.contains_key(&file) {
                let (audio, video) = self.sinks.outs();
                let decoder = Decoder::for_source(source, audio, video)?;
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
