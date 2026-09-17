use crate::state::{Rect, State};
use cut_media::{Frame, PixelLayout};
use cut_render::{FrameRenderer, Picture};
use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

pub struct Gpu {
    window: tauri::Window,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    renderer: Arc<Mutex<FrameRenderer>>,
    size: (u32, u32),
    last_shown: u64,
    shown_rect: Option<Rect>,
    checked: bool,
}

pub struct Uploader {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<Mutex<FrameRenderer>>,
    /// Held, not compared by value: while this `Arc` is alive no other frame
    /// can reuse its address, which is what makes `ptr_eq` sound.
    uploaded: Option<Arc<Frame>>,
}

impl Gpu {
    pub fn new(window: tauri::Window) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;

        let adapter =
            futures::executor::block_on(instance.request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            }))?;

        let (device, queue) =
            futures::executor::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
                label: Some("spike device"),
                ..Default::default()
            }))?;

        let inner = window.inner_size()?;
        let size = (inner.width.max(1), inner.height.max(1));

        let mut config = surface
            .get_default_config(&adapter, size.0, size.1)
            .ok_or_else(|| anyhow::anyhow!("surface is not supported by this adapter"))?;

        // `yuv_to_rgb` already returns gamma-encoded BT.709; an `*Srgb`
        // surface would encode it a second time and wash it out.
        let caps = surface.get_capabilities(&adapter);
        if config.format.is_srgb()
            && let Some(linear) = caps.formats.iter().copied().find(|f| {
                !f.is_srgb() && f.remove_srgb_suffix() == config.format.remove_srgb_suffix()
            })
        {
            config.format = linear;
        }

        if caps_supports(&surface, &adapter, wgpu::CompositeAlphaMode::PostMultiplied) {
            config.alpha_mode = wgpu::CompositeAlphaMode::PostMultiplied;
        }

        config.present_mode = wgpu::PresentMode::Fifo;
        surface.configure(&device, &config);

        eprintln!(
            "surface: {:?} · alpha {:?} · present {:?}",
            config.format, config.alpha_mode, config.present_mode
        );

        let renderer = Arc::new(Mutex::new(FrameRenderer::new(&device, config.format)));

        Ok(Gpu {
            window,
            surface,
            device,
            queue,
            config,
            renderer,
            size,
            last_shown: 0,
            shown_rect: None,
            checked: false,
        })
    }

    pub fn uploader(&self) -> Uploader {
        Uploader {
            device: self.device.clone(),
            queue: self.queue.clone(),
            renderer: self.renderer.clone(),
            uploaded: None,
        }
    }

    fn check_coordinate_space(&mut self, shared: &State) {
        if self.checked {
            return;
        }

        let Some((w, h)) = *shared.surface.page.lock().unwrap() else {
            return;
        };

        self.checked = true;

        if (w, h) != (self.size.0 as f32, self.size.1 as f32) {
            eprintln!(
                "page is {w}x{h} but the surface is {}x{} — the video will be offset",
                self.size.0, self.size.1,
            );
        }
    }

    pub fn present(&mut self, shared: &State) -> anyhow::Result<()> {
        let inner = self.window.inner_size()?;
        if (inner.width, inner.height) != self.size && inner.width > 0 && inner.height > 0 {
            self.size = (inner.width, inner.height);
            self.config.width = inner.width;
            self.config.height = inner.height;
            self.surface.configure(&self.device, &self.config);
            self.shown_rect = None;
        }

        let rect = shared.surface.rect.lock().unwrap().unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: self.size.0 as f32,
            height: self.size.1 as f32,
        });
        if rect.width < 1.0 || rect.height < 1.0 {
            return Ok(());
        }

        // nothing new and nothing moved, the screen already shows this
        let uploaded = shared.slot.generation();
        if uploaded == self.last_shown && self.shown_rect == Some(rect) {
            return Ok(());
        }

        let (vw, vh) = self
            .renderer
            .lock()
            .unwrap()
            .uploaded_size()
            .unwrap_or((0, 0));
        self.check_coordinate_space(shared);
        self.last_shown = uploaded;
        self.shown_rect = Some(rect);

        let renderer = self.renderer.lock().unwrap();
        if vh == 0 {
            return Ok(()); // nothing uploaded yet
        }

        renderer.update_uniforms(&self.queue, rect.width, rect.height, vw, vh);

        let surface_texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            Err(wgpu::SurfaceError::Outdated | wgpu::SurfaceError::Lost) => {
                self.surface.configure(&self.device, &self.config);
                return Ok(());
            }
            Err(e) => return Err(e.into()),
        };

        let view = surface_texture
            .texture
            .create_view(&wgpu::TextureViewDescriptor::default());
        let mut encoder = self
            .device
            .create_command_encoder(&wgpu::CommandEncoderDescriptor { label: None });

        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("video pass"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    resolve_target: None,
                    ops: wgpu::Operations {
                        // The page cannot paint this: everything it leaves
                        // transparent is a hole onto this surface.
                        load: wgpu::LoadOp::Clear(
                            shared
                                .surface
                                .chrome
                                .lock()
                                .unwrap()
                                .map(|(r, g, b)| wgpu::Color { r, g, b, a: 1.0 })
                                .unwrap_or(wgpu::Color::BLACK),
                        ),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            pass.set_viewport(rect.x, rect.y, rect.width, rect.height, 0.0, 1.0);
            renderer.draw(&mut pass);
        }

        self.queue.submit([encoder.finish()]);
        surface_texture.present();
        shared.counters.presents.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }
}

impl Uploader {
    pub fn run(mut self, shared: Arc<State>, handle: tauri::AppHandle, gpu: Arc<Mutex<Gpu>>) {
        loop {
            let frame = shared.slot.take();

            // Redraws outnumber frames — display rate against 24-30fps — and
            // re-uploading an unchanged picture costs megabytes each time.
            let repeat = self
                .uploaded
                .as_ref()
                .is_some_and(|current| Arc::ptr_eq(current, &frame));

            if !repeat {
                self.renderer
                    .lock()
                    .unwrap()
                    .upload(&self.device, &self.queue, &picture(&frame));
                self.uploaded = Some(frame);
            }
            shared.slot.uploaded();

            let gpu = gpu.clone();
            let shared = shared.clone();
            handle
                .run_on_main_thread(move || {
                    if let Ok(mut gpu) = gpu.lock()
                        && let Err(e) = gpu.present(&shared)
                    {
                        eprintln!("present failed: {e:#}");
                    }
                })
                .ok();
        }
    }
}

/// A decoded frame as the renderer wants it: the planes where they lie, and
/// the strides to read them by.
fn picture(frame: &Frame) -> Picture<'_> {
    Picture {
        width: frame.width,
        height: frame.height,
        layout: match frame.layout {
            PixelLayout::Nv12 => cut_render::PixelLayout::Nv12,
            PixelLayout::P010 => cut_render::PixelLayout::P010,
        },
        y: frame.y(),
        y_stride: frame.y_stride(),
        uv: frame.uv(),
        uv_stride: frame.uv_stride(),
    }
}

fn caps_supports(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    mode: wgpu::CompositeAlphaMode,
) -> bool {
    surface
        .get_capabilities(adapter)
        .alpha_modes
        .contains(&mode)
}
