//! A web front end over a native video surface: wgpu draws frames into the
//! window, and a transparent webview sits on top holding the UI.

mod api;
mod decode;
mod gpu;
mod state;

use std::sync::{Arc, Mutex};

use tauri::webview::WebviewBuilder;
use tauri::window::WindowBuilder;
use tauri::{PhysicalPosition, RunEvent, WebviewUrl, WindowEvent};

use state::Shared;

// Scans `src/api`, emits the tauri commands here and the typed client into
// `ui/src/lib/schema/index.ts`.
draad::include_generated!(std::sync::Arc<crate::state::Shared>, client_dir = "ui/src/lib/schema");

/// Where a save goes when the command line does not say.
const DEFAULT_PROJECT: &str = "project.cut";

fn main() -> anyhow::Result<()> {
    cut_engine::init()?;

    let project = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_PROJECT));

    let shared = Arc::new(Shared::default());

    let app = tauri::Builder::default()
        .manage(shared.clone())
        .invoke_handler(invoke_handler())
        .build(tauri::generate_context!())?;

    let window = WindowBuilder::new(&app, "main")
        .title("cut")
        .inner_size(1280.0, 800.0)
        .transparent(true)
        // Keep the native frame: rounded corners, the resize margin and the
        // traffic lights all go with `decorations(false)`. `Overlay` only
        // makes the title bar transparent, so the page still spans the frame.
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .build()?;

    let size = window.inner_size()?;

    // Before the webview: on macOS subview order is creation order. On the
    // main thread because Metal panics if a surface is made anywhere else.
    let gpu = gpu::Gpu::new(window.clone())?;
    let uploader = gpu.uploader();
    let gpu = Arc::new(Mutex::new(gpu));

    // Physical: `inner_size` is device pixels, and `LogicalSize` here would
    // build a webview twice the window's size on a 2x display.
    let webview = window.add_child(
        WebviewBuilder::new("ui", WebviewUrl::App("index.html".into())).transparent(true),
        PhysicalPosition::new(0, 0),
        size,
    )?;

    // Neither follows the window on its own, and a live resize runs in a
    // nested event loop where `MainEventsCleared` stops arriving.
    {
        let (gpu, shared) = (gpu.clone(), shared.clone());
        window.on_window_event(move |event| {
            if let WindowEvent::Resized(size) = event {
                if let Err(e) = webview.set_size(*size) {
                    eprintln!("could not resize the webview: {e:#}");
                }
                if let Ok(mut gpu) = gpu.lock()
                    && let Err(e) = gpu.present(&shared)
                {
                    eprintln!("present during resize failed: {e:#}");
                }
            }
        });
    }

    decode::spawn_decoder(shared.clone(), project);

    {
        let (shared, handle, gpu) = (shared.clone(), app.handle().clone(), gpu.clone());
        std::thread::spawn(move || uploader.run(shared, handle, gpu));
    }

    // Repaints an expose while paused. Not what paces playback: tao idles on
    // `ControlFlow::Wait`, so with no input this fires twice a second.
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

