//! Where decoded media lands on its way out of a decoder.
//!
//! Audio goes straight to the device buffer in `cut-audio`; video comes back
//! here as a stream the transport polls, so it can be scheduled against the
//! clock rather than shown the moment it is decoded.

use cut_audio::AudioSink;
use cut_media::{AudioOut, Frame, VideoOut};
use futures::Stream;
use futures::channel::mpsc::{self, Receiver, Sender};
use futures::stream::FusedStream;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

/// How many decoded frames may wait before a decoder is made to stall.
const VIDEO_QUEUE: usize = 4;

#[derive(Clone)]
pub struct VideoSink(Sender<Arc<Frame>>);

impl VideoSink {
    /// False once the far end is gone, which stops the pipeline feeding it.
    pub fn write(&self, frame: Frame) -> bool {
        match self.0.clone().try_send(Arc::new(frame)) {
            Ok(()) => true,
            Err(e) => e.is_full(),
        }
    }
}

pub struct VideoStream(Receiver<Arc<Frame>>);

impl VideoStream {
    /// Throw away what is already decoded — after a seek it is all from
    /// where the playhead no longer is.
    pub fn drain(&mut self) {
        while self.0.try_recv().is_ok() {}
    }
}

impl Stream for VideoStream {
    type Item = Arc<Frame>;
    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Pin::new(&mut self.0).poll_next(cx)
    }
}

impl FusedStream for VideoStream {
    fn is_terminated(&self) -> bool {
        self.0.is_terminated()
    }
}

/// Where one track's decoders write. Deliberately only the two ends: a track
/// has no business reaching the clock or the play flag, which belong to the
/// [`Player`](crate::Player) that owns the playhead.
#[derive(Clone)]
pub struct Outputs {
    pub audio: AudioSink,
    pub video: VideoSink,
}

impl Outputs {
    pub fn new(audio: AudioSink) -> (Self, VideoStream) {
        let (sender, receiver) = mpsc::channel::<Arc<Frame>>(VIDEO_QUEUE);

        (
            Outputs {
                audio,
                video: VideoSink(sender),
            },
            VideoStream(receiver),
        )
    }

    /// The two callbacks a decoder is built with.
    pub fn outs(&self) -> (AudioOut, VideoOut) {
        let audio = self.audio.clone();
        let video = self.video.clone();

        (
            Arc::new(move |samples: &[f32]| audio.write(samples)),
            Arc::new(move |frame: Frame| video.write(frame)),
        )
    }
}
