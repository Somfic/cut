//! Getting a decoded frame onto the window.

use std::sync::atomic::Ordering;
use std::sync::{Arc, Mutex};

use cut_render::FrameRenderer;

use crate::state::{Rect, Shared};

/// Only seen before the page reports its own resolved background.
const CHROME: wgpu::Color = wgpu::Color {
    r: 0.086,
    g: 0.094,
    b: 0.114,
    a: 1.0,
};

/// The video surface. Lives on the main thread: Metal will not hand out its
/// layer anywhere else.
pub struct Gpu {
    window: tauri::Window,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// The uploader holds this for a write; presenting only to record a draw.
    renderer: Arc<Mutex<FrameRenderer>>,
    size: (u32, u32),
    /// Last presented, so an unchanged turn can be skipped.
    shown: u64,
    shown_rect: Option<Rect>,
    checked: bool,
}

/// The expensive half of the frame path: a full-plane copy per frame, which
/// on the event-loop thread would compete with the webview.
pub struct Uploader {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<Mutex<FrameRenderer>>,
}

impl Gpu {
    pub fn new(window: tauri::Window) -> anyhow::Result<Self> {
        let instance = wgpu::Instance::default();
        let surface = instance.create_surface(window.clone())?;

        let adapter = futures::executor::block_on(instance.request_adapter(
            &wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                compatible_surface: Some(&surface),
                force_fallback_adapter: false,
            },
        ))?;

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
            && let Some(linear) = caps
                .formats
                .iter()
                .copied()
                .find(|f| !f.is_srgb() && f.remove_srgb_suffix() == config.format.remove_srgb_suffix())
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
            shown: 0,
            shown_rect: None,
            checked: false,
        })
    }

    /// `Device` and `Queue` are refcounted and `Send + Sync`, so this is cheap.
    pub fn uploader(&self) -> Uploader {
        Uploader {
            device: self.device.clone(),
            queue: self.queue.clone(),
            renderer: self.renderer.clone(),
        }
    }

    /// The page and the surface must agree on a coordinate space, or every
    /// rect the page reports lands somewhere else. Warned once.
    fn check_coordinate_space(&mut self, shared: &Shared) {
        if self.checked {
            return;
        }
        let Some((w, h)) = *shared.page.lock().unwrap() else {
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

    /// Put whatever the uploader last wrote on screen. No frame data is
    /// touched here — a resize check, a uniform write, a draw and a present.
    pub fn present(&mut self, shared: &Shared) -> anyhow::Result<()> {
        // Cheaper than reacting to resize events, which race the surface.
        let inner = self.window.inner_size()?;
        if (inner.width, inner.height) != self.size && inner.width > 0 && inner.height > 0 {
            self.size = (inner.width, inner.height);
            self.config.width = inner.width;
            self.config.height = inner.height;
            self.surface.configure(&self.device, &self.config);
            // A reconfigured swapchain has undefined contents.
            self.shown_rect = None;
        }

        // The whole window until the page says otherwise.
        let rect = shared.rect.lock().unwrap().unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: self.size.0 as f32,
            height: self.size.1 as f32,
        });
        if rect.width < 1.0 || rect.height < 1.0 {
            return Ok(());
        }

        // Nothing new and nothing moved: the screen already shows this.
        let uploaded = shared.uploaded.load(Ordering::Relaxed);
        if uploaded == self.shown && self.shown_rect == Some(rect) {
            return Ok(());
        }
        let (vw, vh) = self
            .renderer
            .lock()
            .unwrap()
            .uploaded_size()
            .unwrap_or((0, 0));
        self.check_coordinate_space(shared);
        self.shown = uploaded;
        self.shown_rect = Some(rect);

        let renderer = self.renderer.lock().unwrap();
        if vh == 0 {
            return Ok(()); // nothing uploaded yet
        }
        renderer.update_uniforms(&self.queue, rect.width, rect.height, vw, vh);

        let surface_texture = match self.surface.get_current_texture() {
            Ok(texture) => texture,
            // Resized out from under us between the check and here.
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
                                .chrome
                                .lock()
                                .unwrap()
                                .map(|(r, g, b)| wgpu::Color { r, g, b, a: 1.0 })
                                .unwrap_or(CHROME),
                        ),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // No offset: `TitleBarStyle::Overlay` runs the page across the
            // whole frame, so its coordinates are the surface's.
            pass.set_viewport(rect.x, rect.y, rect.width, rect.height, 0.0, 1.0);
            renderer.draw(&mut pass);
        }

        self.queue.submit([encoder.finish()]);
        surface_texture.present();
        shared.presents.fetch_add(1, Ordering::Relaxed);

        Ok(())
    }
}

impl Uploader {
    /// Write arriving frames to the GPU, asking the main thread to present.
    pub fn run(self, shared: Arc<Shared>, handle: tauri::AppHandle, gpu: Arc<Mutex<Gpu>>) {
        while shared.running.load(Ordering::Relaxed) {
            let frame = {
                let mut slot = shared.frame.lock().unwrap();

                while slot.is_none() {
                    if !shared.running.load(Ordering::Relaxed) {
                        return;
                    }
                    slot = shared.arrived.wait(slot).unwrap();
                }

                slot.take().expect("waited until occupied")
            };

            self.renderer
                .lock()
                .unwrap()
                .upload(&self.device, &self.queue, &frame);
            shared.uploaded.fetch_add(1, Ordering::Relaxed);

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

fn caps_supports(
    surface: &wgpu::Surface<'static>,
    adapter: &wgpu::Adapter,
    mode: wgpu::CompositeAlphaMode,
) -> bool {
    surface.get_capabilities(adapter).alpha_modes.contains(&mode)
}
