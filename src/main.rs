#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod audio;
mod settings;
mod tray;
mod updater;
mod visualizer;
mod window_control;

use app::ChosenVisualizerApp;
use updater::UpdatingApp;

const APP_TITLE: &str = "Chosen Visualizer";

fn main() -> eframe::Result<()> {
    let mut args = std::env::args().skip(1);
    if args.next().as_deref() == Some("--update-ui") {
        let download_url = args.next().unwrap_or_default();
        let release_url = args.next().unwrap_or_else(|| download_url.clone());
        let asset_name = args
            .next()
            .unwrap_or_else(|| "chosen-visualizer-update.exe".to_owned());
        return run_updater(download_url, release_url, asset_name);
    }

    if updater::install_launched_update_asset().unwrap_or(false) {
        return Ok(());
    }

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

fn run_updater(
    download_url: String,
    release_url: String,
    asset_name: String,
) -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: eframe::egui::ViewportBuilder::default()
            .with_title("Chosen Visualizer Updater")
            .with_inner_size([520.0, 260.0])
            .with_min_inner_size([420.0, 220.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Chosen Visualizer Updater",
        options,
        Box::new(|_cc| {
            Ok(Box::new(UpdatingApp::new(
                download_url,
                release_url,
                asset_name,
            )))
        }),
    )
}
