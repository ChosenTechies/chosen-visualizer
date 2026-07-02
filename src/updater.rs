use eframe::{
    egui,
    egui::{Color32, RichText},
};
use serde::Deserialize;
use std::{
    env,
    path::PathBuf,
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

pub const APP_VERSION_LABEL: &str = "1.0.2 Early access";
const DEFAULT_REPOSITORY: &str = "urmot/chosen-visualizer";

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub notes: String,
    pub page_url: String,
    pub asset_url: Option<String>,
}

#[derive(Debug)]
pub enum UpdateCheckResult {
    Available(UpdateInfo),
    UpToDate,
    Failed(String),
}

#[derive(Deserialize)]
struct GithubRelease {
    tag_name: String,
    name: Option<String>,
    body: Option<String>,
    html_url: String,
    assets: Vec<GithubAsset>,
}

#[derive(Deserialize)]
struct GithubAsset {
    name: String,
    browser_download_url: String,
}

pub fn repository() -> String {
    option_env!("CHOSEN_VISUALIZER_GITHUB_REPO")
        .unwrap_or(DEFAULT_REPOSITORY)
        .to_owned()
}

pub fn start_update_check() -> Receiver<UpdateCheckResult> {
    let (tx, rx) = mpsc::channel();
    thread::spawn(move || {
        let result = check_latest_release();
        let _ = tx.send(result);
    });
    rx
}

fn check_latest_release() -> UpdateCheckResult {
    let url = format!(
        "https://api.github.com/repos/{}/releases/latest",
        repository()
    );
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(8))
        .user_agent(format!("Chosen Visualizer/{}", APP_VERSION_LABEL))
        .build()
    {
        Ok(client) => client,
        Err(error) => return UpdateCheckResult::Failed(error.to_string()),
    };

    let response = match client.get(url).send() {
        Ok(response) => response,
        Err(error) => return UpdateCheckResult::Failed(error.to_string()),
    };

    if !response.status().is_success() {
        return UpdateCheckResult::Failed(format!("GitHub returned {}", response.status()));
    }

    let body = match response.text() {
        Ok(body) => body,
        Err(error) => return UpdateCheckResult::Failed(error.to_string()),
    };

    let release = match serde_json::from_str::<GithubRelease>(&body) {
        Ok(release) => release,
        Err(error) => return UpdateCheckResult::Failed(error.to_string()),
    };

    if !is_newer_release(&release.tag_name) {
        return UpdateCheckResult::UpToDate;
    }

    let asset_url = release
        .assets
        .iter()
        .find(|asset| asset.name.ends_with(".exe") || asset.name.ends_with(".msi"))
        .or_else(|| release.assets.first())
        .map(|asset| asset.browser_download_url.clone());

    UpdateCheckResult::Available(UpdateInfo {
        version: release.name.unwrap_or(release.tag_name),
        notes: release.body.unwrap_or_default(),
        page_url: release.html_url,
        asset_url,
    })
}

fn is_newer_release(tag: &str) -> bool {
    let latest = version_numbers(tag);
    let current = version_numbers(APP_VERSION_LABEL);
    latest > current
}

fn version_numbers(value: &str) -> Vec<u32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

pub fn launch_update_ui(info: &UpdateInfo) -> Result<(), String> {
    let exe = env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(exe);
    command.arg("--update-ui");
    command.arg(info.asset_url.as_deref().unwrap_or(&info.page_url));
    command.arg(&info.page_url);
    command.spawn().map_err(|error| error.to_string())?;
    Ok(())
}

pub struct UpdatingApp {
    download_url: String,
    release_url: String,
    status: String,
    started: bool,
    result_rx: Option<Receiver<Result<PathBuf, String>>>,
}

impl UpdatingApp {
    pub fn new(download_url: String, release_url: String) -> Self {
        Self {
            download_url,
            release_url,
            status: "Ready to install the latest GitHub release.".to_owned(),
            started: false,
            result_rx: None,
        }
    }

    fn start(&mut self) {
        if self.started {
            return;
        }
        self.started = true;
        self.status = "Downloading update from GitHub...".to_owned();
        let download_url = self.download_url.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = download_update(&download_url);
            let _ = tx.send(result);
        });
        self.result_rx = Some(rx);
    }
}

impl eframe::App for UpdatingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.result_rx {
            if let Ok(result) = rx.try_recv() {
                self.result_rx = None;
                self.status = match result {
                    Ok(path) => {
                        open_installer(&path);
                        format!(
                            "Downloaded update to {}. The installer was opened.",
                            path.display()
                        )
                    }
                    Err(error) => error,
                };
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(18, 20, 23)))
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(28.0);
                    ui.heading(RichText::new("Chosen Visualizer Updater").size(22.0));
                    ui.label(
                        RichText::new(APP_VERSION_LABEL).color(Color32::from_rgb(170, 174, 178)),
                    );
                    ui.add_space(16.0);
                    ui.label(&self.status);
                    ui.add_space(12.0);
                    if ui
                        .add_enabled(
                            !self.started,
                            egui::Button::new("Install update from GitHub"),
                        )
                        .clicked()
                    {
                        self.start();
                    }
                    if ui.button("Open release page").clicked() {
                        open_url(&self.release_url);
                    }
                });
            });

        ctx.request_repaint_after(Duration::from_millis(150));
    }
}

fn download_update(download_url: &str) -> Result<PathBuf, String> {
    if !download_url.starts_with("http") {
        return Err("The release does not include a direct downloadable asset.".to_owned());
    }

    let response = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(120))
        .user_agent(format!("Chosen Visualizer Updater/{}", APP_VERSION_LABEL))
        .build()
        .map_err(|error| error.to_string())?
        .get(download_url)
        .send()
        .map_err(|error| error.to_string())?;

    if !response.status().is_success() {
        return Err(format!("GitHub download failed with {}", response.status()));
    }

    let file_name = download_url
        .rsplit('/')
        .next()
        .filter(|name| !name.is_empty())
        .unwrap_or("chosen-visualizer-update.exe");
    let path = env::temp_dir().join(file_name);
    let bytes = response.bytes().map_err(|error| error.to_string())?;
    std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    Ok(path)
}

fn open_installer(path: &PathBuf) {
    let _ = Command::new(path).spawn();
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();
    }

    #[cfg(not(windows))]
    {
        let _ = Command::new("xdg-open").arg(url).spawn();
    }
}
