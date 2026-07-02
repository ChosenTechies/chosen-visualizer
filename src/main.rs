#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod settings;
mod tray;
mod visualizer;
mod window_control;

use app::ChosenVisualizerApp;

const APP_TITLE: &str = "Chosen Visualizer";

fn main() -> eframe::Result<()> {
    let app_icon =
        eframe::icon_data::from_png_bytes(include_bytes!("../chosen-visualizer.png")).ok();

    let mut viewport = eframe::egui::ViewportBuilder::default()
        .with_title(APP_TITLE)
        .with_inner_size([1180.0, 720.0])
        .with_min_inner_size([240.0, 80.0])
        .with_transparent(true);

    if let Some(icon) = app_icon {
        viewport = viewport.with_icon(icon);
    }

    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };

    eframe::run_native(
        APP_TITLE,
        options,
        Box::new(|cc| Ok(Box::new(ChosenVisualizerApp::new(cc, APP_TITLE)))),
    )
}
