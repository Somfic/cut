use state::State;
use std::sync::{Arc, Mutex};
use tauri::menu::{Menu, PredefinedMenuItem, Submenu};
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
    cut_media::init()?;

    let project = std::env::args_os()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from(DEFAULT_PROJECT));

    let state = Arc::new(State::default());

    // start frontend dev server
    let frontend = std::sync::Mutex::new(Some(frontend::DevServer::start()));

    // build tauri app
    let app = tauri::Builder::default()
        .plugin(tauri_plugin_dialog::init())
        .menu(menu)
        .manage(state.clone())
        .invoke_handler(invoke_handler())
        .build(tauri::generate_context!())?;

    // the dialogs hang off it, and it exists only once the app is built
    state
        .handle
        .set(app.handle().clone())
        .unwrap_or_else(|_| unreachable!("the app is built once"));

    // where saves go until the user says otherwise
    state.session.project.lock().unwrap().path = Some(project.clone());

    // build events
    let bus = draad::TauriBus::new(app.handle().clone());
    state
        .events
        .set(generated::Events::new(bus))
        .unwrap_or_else(|_| unreachable!("the app is built once"));

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
        let (gpu, shared, watched) = (gpu.clone(), state.clone(), window.clone());
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

                // Going fullscreen is a resize, and it is what decides
                // whether the page leaves room for the traffic lights.
                api::window::publish(
                    &shared,
                    api::window::WindowDto {
                        fullscreen: watched.is_fullscreen().unwrap_or(false),
                    },
                );
            }
        });
    }

    // start decoder
    decode::spawn_decoder(state.clone(), project);

    // start autosave
    api::project::spawn_autosave(state.clone());

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
            // on exit write out anything unsaved, and kill the frontend too
            RunEvent::Exit => {
                api::project::flush(&state);
                drop(frontend.lock().unwrap().take());
            }
            _ => {}
        }
    });

    Ok(())
}

/// The default menu bar, minus the items whose keys the app answers itself:
/// AppKit matches ⌘Z before the webview sees it. The clipboard items stay,
/// since they are what makes ⌘C work in a text field.
fn menu(handle: &tauri::AppHandle) -> tauri::Result<Menu<tauri::Wry>> {
    let app = Submenu::with_items(
        handle,
        "cut",
        true,
        &[
            &PredefinedMenuItem::about(handle, None, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::services(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::hide(handle, None)?,
            &PredefinedMenuItem::hide_others(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::quit(handle, None)?,
        ],
    )?;

    let edit = Submenu::with_items(
        handle,
        "Edit",
        true,
        &[
            &PredefinedMenuItem::cut(handle, None)?,
            &PredefinedMenuItem::copy(handle, None)?,
            &PredefinedMenuItem::paste(handle, None)?,
            &PredefinedMenuItem::select_all(handle, None)?,
        ],
    )?;

    let window = Submenu::with_items(
        handle,
        "Window",
        true,
        &[
            &PredefinedMenuItem::minimize(handle, None)?,
            &PredefinedMenuItem::separator(handle)?,
            &PredefinedMenuItem::close_window(handle, None)?,
        ],
    )?;

    Menu::with_items(handle, &[&app, &edit, &window])
}

// generated stuff
#[allow(non_camel_case_types, dead_code, unused_imports)]
mod generated {
    pub(super) type __DraadState = std::sync::Arc<crate::state::State>;
    pub(super) type __DraadBus = ::draad::TauriBus<::tauri::Wry>;
    include!("generated.rs");
}
use generated::invoke_handler;
