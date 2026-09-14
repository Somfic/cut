//! Does a web front end over the native video surface actually work?
//!
//! The window has no webview of its own. wgpu draws decoded frames straight
//! into it — the same `cut_render::FrameRenderer` the iced front end uses, so
//! what this measures is the real frame path, not a mock of it. A transparent
//! webview is then added as a child, on top, holding the HTML.
//!
//! Nothing here is meant to survive. See README.md for what it has to prove.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::Instant;

use cut_engine::media::Frame;
use cut_engine::playback::{self, Controls, Request};
use cut_render::FrameRenderer;
use futures::StreamExt;
use tauri::webview::WebviewBuilder;
use tauri::window::WindowBuilder;
use tauri::{LogicalPosition, LogicalSize, RunEvent, WebviewUrl};

/// Where a save goes when the command line does not say. Matches the app, so
/// the spike opens whatever project you were last editing.
const DEFAULT_PROJECT: &str = "project.cut";

/// The area of the window the video should letterbox into, in physical pixels.
/// The page owns this: it reports the rect of the element standing in for the
/// preview, which is how question 4 (resize sync) gets tested.
#[derive(Default, Clone, Copy, PartialEq)]
struct Rect {
    x: f32,
    y: f32,
    width: f32,
    height: f32,
}

#[derive(Default)]
struct Shared {
    /// The frame waiting to be uploaded. Latest-wins: a preview that falls
    /// behind should skip rather than build up a backlog.
    frame: Mutex<Option<Arc<Frame>>>,
    /// Signalled when `frame` is replaced, so the uploader can sleep rather
    /// than poll.
    arrived: Condvar,
    rect: Mutex<Option<Rect>>,
    controls: Mutex<Option<Controls>>,
    /// Bumped once per completed upload, so the presenter can tell new
    /// content from an event-loop turn with nothing behind it.
    uploaded: AtomicU64,
    /// Running totals. Rates are differences over a known interval rather than
    /// a smoothed instantaneous figure: a smoothed one is dominated by bursts
    /// and reports numbers that never happened.
    frames: AtomicU64,
    presents: AtomicU64,
    /// Frames replaced in the slot before the draw loop ever took them.
    dropped: AtomicU64,
    running: AtomicBool,
}

#[derive(serde::Serialize)]
struct Stats {
    frames: u64,
    presents: u64,
    dropped: u64,
    playing: bool,
}

#[tauri::command]
fn stats(shared: tauri::State<'_, Arc<Shared>>) -> Stats {
    Stats {
        frames: shared.frames.load(Ordering::Relaxed),
        presents: shared.presents.load(Ordering::Relaxed),
        dropped: shared.dropped.load(Ordering::Relaxed),
        playing: shared
            .controls
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(Controls::is_playing),
    }
}

#[tauri::command]
fn toggle_playback(shared: tauri::State<'_, Arc<Shared>>) {
    if let Some(controls) = shared.controls.lock().unwrap().as_mut() {
        controls.send(Request::TogglePlayback);
    }
}

/// The page telling us where it has left room for the video.
#[tauri::command]
fn set_video_rect(shared: tauri::State<'_, Arc<Shared>>, x: f32, y: f32, width: f32, height: f32) {
    *shared.rect.lock().unwrap() = Some(Rect { x, y, width, height });
}

fn main() -> anyhow::Result<()> {
    cut_engine::init()?;

    let project = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_PROJECT));

    let shared = Arc::new(Shared::default());

    let app = tauri::Builder::default()
        .manage(shared.clone())
        .invoke_handler(tauri::generate_handler![stats, toggle_playback, set_video_rect])
        .build(tauri::generate_context!())?;

    // A bare window: no webview of its own, so wgpu owns the surface.
    let window = WindowBuilder::new(&app, "main")
        .title("cut — overlay spike")
        .inner_size(1280.0, 800.0)
        .transparent(true)
        .build()?;

    let size = window.inner_size()?;

    // Before the webview: the surface has to exist under it, and on macOS
    // subview order is creation order.
    //
    // On the main thread, and not by choice — wgpu's Metal backend panics with
    // "get_metal_layer cannot be called in non-ui thread" if the surface is
    // created anywhere else. Presenting stays here for the same reason; only
    // the upload gets a thread of its own.
    let gpu = Gpu::new(window.clone())?;
    let uploader = gpu.uploader();
    let gpu = Arc::new(Mutex::new(gpu));

    window.add_child(
        WebviewBuilder::new("ui", WebviewUrl::App("index.html".into())).transparent(true),
        LogicalPosition::new(0.0, 0.0),
        LogicalSize::new(size.width as f64, size.height as f64),
    )?;

    spawn_decoder(shared.clone(), project);

    {
        let (shared, handle, gpu) = (shared.clone(), app.handle().clone(), gpu.clone());
        std::thread::spawn(move || uploader.run(shared, handle, gpu));
    }

    // Still presented here as well, so a resize or an expose repaints even
    // while paused — but this is no longer what paces playback. tao idles on
    // `ControlFlow::Wait`, so with no input this fires a couple of times a
    // second, which was the whole of the stutter.
    app.run(move |_handle, event| {
        if let RunEvent::MainEventsCleared = event
            && let Ok(mut gpu) = gpu.lock()
            && let Err(e) = gpu.present(&shared)
        {
            eprintln!("present failed: {e:#}");
        }
    });

    Ok(())
}

/// Start the transport.
///
/// It hands frames to the draw loop through `Shared::frame`: deliberately a
/// latest-wins slot rather than a queue, because a preview that falls behind
/// should skip rather than build up a backlog — and the drop count is one of
/// the numbers worth reading.
fn spawn_decoder(shared: Arc<Shared>, project: std::path::PathBuf) {
    shared.running.store(true, Ordering::Relaxed);

    // Question 2, on stderr as well as in the page: the readout in the webview
    // proves the IPC works, but only a number that arrives without the webview
    // being involved proves the draw loop is still ticking.
    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            let (mut frames, mut presents, mut dropped) = (0, 0, 0);
            let mut since = Instant::now();

            while shared.running.load(Ordering::Relaxed) {
                std::thread::sleep(std::time::Duration::from_secs(2));

                let now = Instant::now();
                let secs = (now - since).as_secs_f64();
                since = now;

                let (f, p, d) = (
                    shared.frames.load(Ordering::Relaxed),
                    shared.presents.load(Ordering::Relaxed),
                    shared.dropped.load(Ordering::Relaxed),
                );
                eprintln!(
                    "video {:.1} fps · present {:.1} fps · dropped {:.1}/s",
                    (f - frames) as f64 / secs,
                    (p - presents) as f64 / secs,
                    (d - dropped) as f64 / secs,
                );
                (frames, presents, dropped) = (f, p, d);
            }
        });
    }

    {
        let shared = shared.clone();
        std::thread::spawn(move || {
            futures::executor::block_on(async move {
                let mut events = Box::pin(playback::transport(&project));

                while let Some(event) = events.next().await {
                    match event {
                        playback::Event::Ready(controls) => {
                            *shared.controls.lock().unwrap() = Some(controls);
                        }
                        playback::Event::Opened { .. } => {}
                        playback::Event::Frame(frame) => {
                            shared.frames.fetch_add(1, Ordering::Relaxed);

                            // Replacing a frame the uploader never took is a
                            // dropped one.
                            let previous = shared.frame.lock().unwrap().replace(frame);
                            if previous.is_some() {
                                shared.dropped.fetch_add(1, Ordering::Relaxed);
                            }
                            shared.arrived.notify_one();
                        }
                    }
                }
            })
        });
    }

}

/// The video surface, and everything needed to put a frame on it.
///
/// Lives on the main thread and is stepped once per event-loop turn, because
/// Metal will not hand out its layer anywhere else.
struct Gpu {
    window: tauri::Window,
    surface: wgpu::Surface<'static>,
    device: wgpu::Device,
    queue: wgpu::Queue,
    config: wgpu::SurfaceConfiguration,
    /// Shared with the uploader, which holds it for the length of a texture
    /// write. Presenting only locks it long enough to record a draw call.
    renderer: Arc<Mutex<FrameRenderer>>,
    size: (u32, u32),
    /// The upload count and target rect last presented. Presenting again with
    /// all three unchanged would put an identical picture on the screen.
    shown: u64,
    shown_rect: Option<Rect>,
}

/// The expensive half of the frame path, on a thread of its own.
///
/// Uploading a frame is two full-plane copies — `pack_plane` into a `Vec` on
/// the decoder side, then `write_texture` here — which is 6 MB per frame at
/// 1080p NV12 and 50 MB at 4K P010. Doing that on the event-loop thread makes
/// it compete with the webview for the one thread macOS will let either of
/// them run on. `wgpu::Queue` is `Send + Sync`, so it does not have to.
struct Uploader {
    device: wgpu::Device,
    queue: wgpu::Queue,
    renderer: Arc<Mutex<FrameRenderer>>,
}

impl Gpu {
    fn new(window: tauri::Window) -> anyhow::Result<Self> {
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

        // `yuv_to_rgb` returns BT.709 values that are already gamma-encoded.
        // An `*Srgb` surface would encode them a second time — the washed-out,
        // wrong colours. Ask for the linear-writing twin of whatever the
        // default is.
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
        // Alpha is the whole question: without a surface that can be composited
        // over, there is nothing for the page to show through.
        if caps_supports(&surface, &adapter, wgpu::CompositeAlphaMode::PostMultiplied) {
            config.alpha_mode = wgpu::CompositeAlphaMode::PostMultiplied;
        }
        // Vsync, so the present rate the readout shows means something.
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
        })
    }

    /// A handle for the upload thread. Cloning a `Device` and `Queue` is cheap
    /// — they are reference-counted — and both are `Send + Sync`.
    fn uploader(&self) -> Uploader {
        Uploader {
            device: self.device.clone(),
            queue: self.queue.clone(),
            renderer: self.renderer.clone(),
        }
    }

    /// Put whatever the uploader last wrote on the screen.
    ///
    /// All that is left on the main thread: a resize check, an 8-byte uniform
    /// write, one draw call and the present. No frame data is touched.
    fn present(&mut self, shared: &Shared) -> anyhow::Result<()> {
        // Follow the window. Cheap enough to check every turn, and the
        // alternative — reacting to resize events — races the surface.
        let inner = self.window.inner_size()?;
        if (inner.width, inner.height) != self.size && inner.width > 0 && inner.height > 0 {
            self.size = (inner.width, inner.height);
            self.config.width = inner.width;
            self.config.height = inner.height;
            self.surface.configure(&self.device, &self.config);
        }

        // Whatever the page says, falling back to the whole window until it
        // has told us.
        let rect = shared.rect.lock().unwrap().unwrap_or(Rect {
            x: 0.0,
            y: 0.0,
            width: self.size.0 as f32,
            height: self.size.1 as f32,
        });
        if rect.width < 1.0 || rect.height < 1.0 {
            return Ok(());
        }

        // Nothing new uploaded and nothing moved: the screen already shows
        // this. `MainEventsCleared` fires far more often than either changes.
        let uploaded = shared.uploaded.load(Ordering::Relaxed);
        if uploaded == self.shown && self.shown_rect == Some(rect) {
            return Ok(());
        }
        self.shown = uploaded;
        self.shown_rect = Some(rect);

        let renderer = self.renderer.lock().unwrap();
        let Some((vw, vh)) = renderer.uploaded_size() else {
            return Ok(()); // nothing uploaded yet
        };
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
                        // Transparent, not black: everywhere the video is not
                        // is where the page has to show through. If this comes
                        // out opaque, question 1 has failed.
                        load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                        store: wgpu::StoreOp::Store,
                    },
                    depth_slice: None,
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
            });

            // Confine the video to the rect the page reserved.
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
    /// Wait for frames and write them to the GPU, asking the main thread to
    /// present each one. Returns when the transport stops.
    fn run(self, shared: Arc<Shared>, handle: tauri::AppHandle, gpu: Arc<Mutex<Gpu>>) {
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

            // The copy. Off the event-loop thread, which is the entire point.
            self.renderer
                .lock()
                .unwrap()
                .upload(&self.device, &self.queue, &frame);
            shared.uploaded.fetch_add(1, Ordering::Relaxed);

            // Metal keeps the surface on the main thread, so the finished
            // frame has to be handed there to be shown.
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
