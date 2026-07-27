/// View state owned by the canvas: viewport and drag tracking. The playhead
/// itself lives in the app, since the video pipeline is the source of truth.
#[derive(Debug)]
pub struct State {
    /// Frame at the left edge of the widget.
    pub(super) scroll: f32,
    /// Pixels per frame.
    pub(super) zoom: f32,
    /// The last frame we asked for while dragging, or `None` when not dragging.
    /// Remembered to suppress repeat seeks for a frame we're already on.
    pub(super) scrubbing: Option<usize>,
    /// Whether the initial fit-to-width has happened. It can't run until we
    /// know both the widget's width and the timeline's length, neither of
    /// which is available when the state is created.
    pub(super) fitted: bool,
}

impl Default for State {
    fn default() -> Self {
        State {
            scroll: 0.0,
            zoom: 2.0,
            scrubbing: None,
            fitted: false,
        }
    }
}

impl State {
    /// Where a timeline frame sits horizontally, in widget pixels.
    pub(super) fn x_of(&self, frame: f32) -> f32 {
        (frame - self.scroll) * self.zoom
    }

    /// The inverse: which timeline frame a pixel column points at.
    pub(super) fn frame_at(&self, x: f32) -> usize {
        (self.scroll + x / self.zoom).max(0.0) as usize
    }
}
