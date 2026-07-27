mod app;
mod demo;
mod input;
mod media;
mod playback;
mod project;
mod ui;

use app::App;

fn main() -> anyhow::Result<()> {
    gstreamer::init()?;

    iced::application(
        move || (App::default(), iced::Task::none()),
        App::update,
        App::view,
    )
    .subscription(App::subscription)
    .run()?;

    Ok(())
}
