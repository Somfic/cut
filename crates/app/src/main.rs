use state::State;
use std::sync::{Arc, Mutex};
use tauri::webview::WebviewBuilder;
use tauri::window::WindowBuilder;
use tauri::{PhysicalPosition, RunEvent, TitleBarStyle, WindowEvent};

mod api;
mod decode;
mod frontend;
mod gpu;
mod state;

const DEFAULT_PROJECT: &str = "project.cut";

fn main() -> anyhow::Result<()> {
    cut_engine::init()?;

    let project = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_PROJECT));

    let state = Arc::new(State::default());

    // start frontend dev server
    let frontend = std::sync::Mutex::new(Some(frontend::DevServer::start()));

    // build tauri app
    let app = tauri::Builder::default()
        .manage(state.clone())
        .invoke_handler(invoke_handler())
        .build(tauri::generate_context!())?;

    // create window
    let window = WindowBuilder::new(&app, "main")
        .title("cut")
        .inner_size(1280.0, 800.0)
        .transparent(true)
        .title_bar_style(TitleBarStyle::Overlay)
        .hidden_title(true)
        .build()?;

    let webview = window.add_child(
        WebviewBuilder::new("ui", frontend::url()).transparent(true),
        PhysicalPosition::new(0, 0),
        window.inner_size()?,
    )?;

    // create gpu
    let gpu = gpu::Gpu::new(window.clone())?;
    let uploader = gpu.uploader();
    let gpu = Arc::new(Mutex::new(gpu));

    // repaint on resize
    {
        let (gpu, shared) = (gpu.clone(), state.clone());
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

    // start decoder
    decode::spawn_decoder(state.clone(), project);

    // start uploader
    {
        let (shared, handle, gpu) = (state.clone(), app.handle().clone(), gpu.clone());
        std::thread::spawn(move || uploader.run(shared, handle, gpu));
    }

    // start app
    app.run(move |_handle, event| {
        match event {
            RunEvent::MainEventsCleared => {
                if let Ok(mut gpu) = gpu.lock()
                    && let Err(e) = gpu.present(&state)
                {
                    eprintln!("present failed: {e:#}");
                }
            }
            // on exit kill frontend too
            RunEvent::Exit => drop(frontend.lock().unwrap().take()),
            _ => {}
        }
    });

    Ok(())
}

// generated stuff
#[allow(non_camel_case_types, dead_code, unused_imports)]
mod generated {
    pub(super) type __DraadState = std::sync::Arc<crate::state::State>;
    pub(super) type __DraadBus = ();
    include!("generated.rs");
}
use generated::invoke_handler;
