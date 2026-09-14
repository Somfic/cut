//! The preview, as an iced widget.
//!
//! Only the adapter lives here: the drawing itself is `cut_render`, so the
//! same frame path serves any other front end.

use cut_engine::media::Frame;
use cut_render::FrameRenderer;
use iced::{Rectangle, mouse, wgpu, widget::shader::Viewport};
use std::sync::Arc;

pub struct VideoView {
    pub frame: Option<Arc<Frame>>,
}

/// `FrameRenderer` under iced's pipeline trait, which it cannot implement
/// itself from another crate.
pub struct VideoPipeline(FrameRenderer);

impl iced::widget::shader::Pipeline for VideoPipeline {
    fn new(device: &wgpu::Device, _queue: &wgpu::Queue, format: wgpu::TextureFormat) -> Self {
        VideoPipeline(FrameRenderer::new(device, format))
    }
}

#[derive(Debug)]
pub struct FramePrimitive {
    frame: Option<Arc<Frame>>,
}

impl iced::widget::shader::Primitive for FramePrimitive {
    type Pipeline = VideoPipeline;

    fn prepare(
        &self,
        renderer: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        if let Some(frame) = &self.frame {
            renderer.0.upload(device, queue, frame);
            renderer
                .0
                .update_uniforms(queue, bounds.width, bounds.height, frame.width, frame.height);
        }
    }

    fn draw(&self, renderer: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        renderer.0.draw(render_pass)
    }
}

impl<Message> iced::widget::shader::Program<Message> for VideoView {
    type State = ();
    type Primitive = FramePrimitive;

    fn draw(
        &self,
        _state: &Self::State,
        _cursor: mouse::Cursor,
        _bounds: Rectangle,
    ) -> Self::Primitive {
        FramePrimitive {
            frame: self.frame.clone(),
        }
    }
}
