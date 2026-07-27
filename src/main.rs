mod app;
mod demo;
mod media;
mod playback;
mod timeline;
mod ui;

use app::Screen;

fn main() -> anyhow::Result<()> {
    gstreamer::init()?;

    iced::application(
        move || (Screen::default(), iced::Task::none()),
        Screen::update,
        Screen::view,
    )
    .subscription(Screen::subscription)
    .run()?;

    Ok(())
}
