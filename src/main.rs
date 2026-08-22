mod app;
mod demo;
mod input;
mod media;
mod playback;
mod project;
mod rate;
mod ui;

use std::path::PathBuf;

use app::App;

/// Where a save goes when the command line does not say.
const DEFAULT_PROJECT: &str = "project.cut";

fn main() -> anyhow::Result<()> {
    gstreamer::init()?;

    // `cut [path]`. The file does not have to exist yet: without one the demo
    // timeline comes up, and saving writes the first project there.
    let project = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PROJECT));

    iced::application(
        move || (App::new(project.clone()), iced::Task::none()),
        App::update,
        App::view,
    )
    .subscription(App::subscription)
    .run()?;

    Ok(())
}
