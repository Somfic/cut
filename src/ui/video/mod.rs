use crate::media::Frame;
use iced::{Rectangle, mouse, wgpu, widget::shader::Viewport};
use std::sync::Arc;

mod renderer;

pub use renderer::FrameRenderer;

pub struct VideoView {
    pub frame: Option<Arc<Frame>>,
}

#[derive(Debug)]
pub struct FramePrimitive {
    frame: Option<Arc<Frame>>,
}

impl iced::widget::shader::Primitive for FramePrimitive {
    type Pipeline = FrameRenderer;

    fn prepare(
        &self,
        renderer: &mut Self::Pipeline,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: &Rectangle,
        _viewport: &Viewport,
    ) {
        if let Some(frame) = &self.frame {
            renderer.upload(device, queue, frame);
            renderer.update_uniforms(queue, bounds, frame.width, frame.height);
        }
    }

    fn draw(&self, renderer: &Self::Pipeline, render_pass: &mut wgpu::RenderPass<'_>) -> bool {
        renderer.draw(render_pass)
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
