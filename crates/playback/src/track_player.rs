use crate::{Outputs, VideoStream};
use cut_media::{Decoder, SeekMode};
use cut_timeline::Clip;
use std::collections::HashMap;
use std::path::PathBuf;
use std::time::Duration;

/// Plays one track of a timeline: the decoders for every source it has
/// touched, and which of them is live. The playhead belongs to the
/// [`Player`](crate::Player) above it — a track never seeks itself.
pub struct TrackPlayer {
    outputs: Outputs,
    decoders: HashMap<PathBuf, Decoder>,
    live_source: Option<PathBuf>,
    live_clip: Option<Clip>,
}

impl TrackPlayer {
    pub fn new(outputs: &Outputs) -> Self {
        TrackPlayer {
            outputs: outputs.clone(),
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

    /// Seek the live decoder to where it lands. The clock has one owner, and
    /// it is the [`Player`](crate::Player).
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
    /// just a seek; across sources it swaps which decoder is live. `playing`
    /// is passed in rather than read: whether the timeline is rolling is not
    /// a track's to know.
    pub fn cut_to(
        &mut self,
        clip: &Clip,
        tl_frame: usize,
        stream: &mut VideoStream,
        mode: SeekMode,
        playing: bool,
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
                let (audio, video) = self.outputs.outs();
                let decoder = Decoder::for_source(source, audio, video)?;
                self.decoders.insert(file.clone(), decoder);
            }

            let decoder = &self.decoders[&file];
            if playing {
                decoder.play();
            } else {
                decoder.pause();
            }
        }

        Ok(self.seek(clip.source_frame(tl_frame), stream, mode))
    }
}
