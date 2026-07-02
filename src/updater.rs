use eframe::{
    egui,
    egui::{Color32, RichText, Stroke, Vec2},
};
use serde::Deserialize;
use std::{
    cmp::Ordering,
    env,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_VERSION_LABEL: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const DEFAULT_REPOSITORY: &str = "ChosenTechies/chosen-visualizer";

#[derive(Clone, Debug)]
pub struct UpdateAsset {
    pub name: String,
    pub download_url: String,
}

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub tag: String,
    pub notes: String,
    pub page_url: String,
    pub prerelease: bool,
    pub asset: Option<UpdateAsset>,
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
    draft: bool,
    prerelease: bool,
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
        let result = check_releases();
        let _ = tx.send(result);
    });
    rx
}

fn check_releases() -> UpdateCheckResult {
    let repo = repository();
    let url = format!("https://api.github.com/repos/{repo}/releases?per_page=20");
    let client = match reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(10))
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

    if response.status() == reqwest::StatusCode::NOT_FOUND {
        return UpdateCheckResult::Failed(format!(
            "GitHub repository {repo} was not found. Confirm the repository and releases are public."
        ));
    }

    if !response.status().is_success() {
        return UpdateCheckResult::Failed(format!("GitHub returned {}", response.status()));
    }

    let body = match response.text() {
        Ok(body) => body,
        Err(error) => return UpdateCheckResult::Failed(error.to_string()),
    };

    let releases = match serde_json::from_str::<Vec<GithubRelease>>(&body) {
        Ok(releases) => releases,
        Err(error) => return UpdateCheckResult::Failed(error.to_string()),
    };

    let release = releases
        .into_iter()
        .filter(|release| !release.draft && is_newer_release(&release.tag_name))
        .max_by(|left, right| compare_versions(&left.tag_name, &right.tag_name));

    let Some(release) = release else {
        return UpdateCheckResult::UpToDate;
    };

    let asset = release.assets.iter().find_map(update_asset);
    let tag = release.tag_name.clone();
    UpdateCheckResult::Available(UpdateInfo {
        version: release.name.unwrap_or_else(|| tag.clone()),
        tag,
        notes: release.body.unwrap_or_default(),
        page_url: release.html_url,
        prerelease: release.prerelease,
        asset,
    })
}

fn update_asset(asset: &GithubAsset) -> Option<UpdateAsset> {
    if !is_installable_asset_name(&asset.name) {
        return None;
    }

    Some(UpdateAsset {
        name: asset.name.clone(),
        download_url: asset.browser_download_url.clone(),
    })
}

fn is_installable_asset_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    let is_windows_installer = lower.ends_with(".msi")
        || (lower.ends_with(".exe")
            && (lower.contains("setup")
                || lower.contains("install")
                || lower.contains("chosen-visualizer")));
    is_windows_installer
        && !lower.contains("debug")
        && !lower.ends_with(".pdb")
        && !lower.ends_with(".zip")
}

fn is_newer_release(tag: &str) -> bool {
    compare_versions(tag, APP_VERSION) == Ordering::Greater
}

fn compare_versions(left: &str, right: &str) -> Ordering {
    let mut left_numbers = version_numbers(left);
    let mut right_numbers = version_numbers(right);
    let len = left_numbers.len().max(right_numbers.len()).max(3);
    left_numbers.resize(len, 0);
    right_numbers.resize(len, 0);
    left_numbers.cmp(&right_numbers)
}

fn version_numbers(value: &str) -> Vec<u32> {
    value
        .split(|character: char| !character.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .map(|part| part.parse::<u32>().unwrap_or(0))
        .collect()
}

pub fn launch_update_ui(info: &UpdateInfo) -> Result<(), String> {
    let Some(asset) = &info.asset else {
        return Err("This GitHub release does not include a Windows installer asset.".to_owned());
    };

    let exe = env::current_exe().map_err(|error| error.to_string())?;
    let mut command = Command::new(exe);
    command.arg("--update-ui");
    command.arg(&asset.download_url);
    command.arg(&info.page_url);
    command.arg(&asset.name);
    command.spawn().map_err(|error| error.to_string())?;
    Ok(())
}

pub struct UpdatingApp {
    download_url: String,
    release_url: String,
    asset_name: String,
    status: String,
    started: bool,
    failed: bool,
    result_rx: Option<Receiver<Result<PathBuf, String>>>,
}

impl UpdatingApp {
    pub fn new(download_url: String, release_url: String, asset_name: String) -> Self {
        Self {
            download_url,
            release_url,
            asset_name,
            status: "Preparing the GitHub release download...".to_owned(),
            started: false,
            failed: false,
            result_rx: None,
        }
    }

    fn start(&mut self) {
        if self.started || self.download_url.trim().is_empty() {
            return;
        }
        self.started = true;
        self.failed = false;
        self.status = format!("Downloading {} from GitHub...", self.asset_name);
        let download_url = self.download_url.clone();
        let asset_name = self.asset_name.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = download_update(&download_url, &asset_name);
            let _ = tx.send(result);
        });
        self.result_rx = Some(rx);
    }

    fn retry(&mut self) {
        self.started = false;
        self.failed = false;
        self.start();
    }
}

impl eframe::App for UpdatingApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if !self.started {
            self.start();
        }

        if let Some(rx) = &self.result_rx {
            if let Ok(result) = rx.try_recv() {
                self.result_rx = None;
                self.status = match result {
                    Ok(path) => match open_installer(&path) {
                        Ok(()) => {
                            format!("Downloaded {}. The installer was opened.", path.display())
                        }
                        Err(error) => {
                            self.failed = true;
                            format!(
                                "Downloaded {}, but could not open it: {error}",
                                path.display()
                            )
                        }
                    },
                    Err(error) => {
                        self.failed = true;
                        error
                    }
                };
            }
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(Color32::from_rgb(15, 17, 20)))
            .show(ctx, |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(10.0, 10.0);
                ui.add_space(18.0);
                ui.vertical_centered(|ui| {
                    ui.label(
                        RichText::new("Chosen Visualizer")
                            .size(13.0)
                            .color(Color32::from_rgb(151, 162, 176)),
                    );
                    ui.heading(
                        RichText::new("Installing Update")
                            .size(24.0)
                            .color(Color32::from_rgb(238, 240, 243)),
                    );
                    ui.label(
                        RichText::new(format!("Installed build: {APP_VERSION_LABEL}"))
                            .color(Color32::from_rgb(151, 162, 176)),
                    );
                });

                ui.add_space(12.0);
                egui::Frame::none()
                    .fill(Color32::from_rgb(24, 27, 31))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(46, 52, 60)))
                    .inner_margin(egui::Margin::symmetric(16.0, 14.0))
                    .show(ui, |ui| {
                        ui.label(
                            RichText::new(&self.asset_name)
                                .strong()
                                .color(Color32::from_rgb(226, 231, 236)),
                        );
                        ui.label(
                            RichText::new(&self.status).color(Color32::from_rgb(178, 185, 194)),
                        );
                        if self.result_rx.is_some() {
                            ui.add(egui::ProgressBar::new(0.5).animate(true).show_percentage());
                        }
                    });

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Open release page").clicked() {
                        open_url(&self.release_url);
                    }
                    if self.failed && ui.button("Retry").clicked() {
                        self.retry();
                    }
                });
            });

        ctx.request_repaint_after(Duration::from_millis(150));
    }
}

fn download_update(download_url: &str, asset_name: &str) -> Result<PathBuf, String> {
    if !download_url.starts_with("https://") {
        return Err("The release asset URL is not a secure GitHub download URL.".to_owned());
    }
    if !is_installable_asset_name(asset_name) {
        return Err(format!(
            "{asset_name} is not a supported Windows installer asset."
        ));
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

    let path = env::temp_dir().join(safe_asset_file_name(asset_name));
    let bytes = response.bytes().map_err(|error| error.to_string())?;
    std::fs::write(&path, bytes).map_err(|error| error.to_string())?;
    Ok(path)
}

fn safe_asset_file_name(name: &str) -> String {
    let file_name: String = name
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, '.' | '-' | '_') {
                character
            } else {
                '_'
            }
        })
        .collect();

    if file_name.trim_matches('_').is_empty() {
        "chosen-visualizer-update.exe".to_owned()
    } else {
        file_name
    }
}

fn open_installer(path: &Path) -> Result<(), String> {
    let extension = path
        .extension()
        .and_then(|extension| extension.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();

    let mut command = if extension == "msi" {
        let mut command = Command::new("msiexec");
        command.arg("/i").arg(path);
        command
    } else {
        Command::new(path)
    };

    command
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
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

#[cfg(test)]
mod tests {
    use super::{compare_versions, is_installable_asset_name};
    use std::cmp::Ordering;

    #[test]
    fn compares_version_tags() {
        assert_eq!(compare_versions("v1.0.3", "1.0.2"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.2", "v1.0.2"), Ordering::Equal);
        assert_eq!(compare_versions("v1.0.1", "1.0.2"), Ordering::Less);
    }

    #[test]
    fn only_installs_windows_assets() {
        assert!(is_installable_asset_name("chosen-visualizer-installer.exe"));
        assert!(is_installable_asset_name("ChosenVisualizerSetup.msi"));
        assert!(!is_installable_asset_name("source.zip"));
        assert!(!is_installable_asset_name("chosen-visualizer.pdb"));
    }
}
