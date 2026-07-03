use crate::settings::settings_path;
use eframe::{
    egui,
    egui::{Color32, RichText, Stroke, Vec2},
};
use serde::Deserialize;
use std::{
    cmp::Ordering,
    env, fs, io,
    path::{Path, PathBuf},
    process::Command,
    sync::mpsc::{self, Receiver},
    thread,
    time::Duration,
};

pub const APP_VERSION: &str = env!("CARGO_PKG_VERSION");
pub const APP_VERSION_LABEL: &str = concat!("v", env!("CARGO_PKG_VERSION"));
const DEFAULT_REPOSITORY: &str = "ChosenTechies/chosen-visualizer";
const INSTALLED_EXE_NAME: &str = "chosen-visualizer.exe";
const UPDATER_EXE_NAME: &str = "chosen-visualizer-updater.exe";
const PARTIAL_EXE_NAME: &str = "chosen-visualizer.exe.download";
const DESKTOP_SHORTCUT_NAME: &str = "Chosen Visualizer.lnk";

#[derive(Clone, Debug)]
pub struct UpdateAsset {
    pub name: String,
    pub download_url: String,
}

#[derive(Clone, Debug)]
pub struct UpdateInfo {
    pub version: String,
    pub tag: String,
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
        .max_by(compare_release_candidates);

    let Some(release) = release else {
        return UpdateCheckResult::UpToDate;
    };

    let asset = release.assets.iter().find_map(update_asset);
    let tag = release.tag_name.clone();
    UpdateCheckResult::Available(UpdateInfo {
        version: release.name.unwrap_or_else(|| tag.clone()),
        tag,
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
    let is_windows_exe = lower.ends_with(".exe")
        && (lower.contains("setup")
            || lower.contains("install")
            || lower.contains("chosen-visualizer"));
    is_windows_exe
        && !lower.contains("debug")
        && !lower.ends_with(".pdb")
        && !lower.ends_with(".zip")
}

fn is_versioned_update_asset_name(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.starts_with("chosen-visualizer-v") && lower.ends_with(".exe")
}

fn is_newer_release(tag: &str) -> bool {
    compare_versions(tag, APP_VERSION) == Ordering::Greater
}

fn compare_release_candidates(left: &GithubRelease, right: &GithubRelease) -> Ordering {
    let version_order = compare_versions(&left.tag_name, &right.tag_name);
    if version_order != Ordering::Equal {
        return version_order;
    }

    match (left.prerelease, right.prerelease) {
        (true, false) => Ordering::Less,
        (false, true) => Ordering::Greater,
        _ => Ordering::Equal,
    }
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
        return Err("This GitHub release does not include a Windows executable asset.".to_owned());
    };

    let helper = prepare_update_helper()?;
    let mut command = Command::new(helper);
    command.arg("--update-ui");
    command.arg(&asset.download_url);
    command.arg(&info.page_url);
    command.arg(&asset.name);
    command.spawn().map_err(|error| error.to_string())?;
    Ok(())
}

fn prepare_update_helper() -> Result<PathBuf, String> {
    let install_dir = app_install_dir()?;
    fs::create_dir_all(&install_dir).map_err(|error| error.to_string())?;

    let current_exe = env::current_exe().map_err(|error| error.to_string())?;
    let helper = install_dir.join(UPDATER_EXE_NAME);
    if !same_path(&current_exe, &helper) {
        fs::copy(&current_exe, &helper).map_err(|error| {
            format!(
                "Could not prepare the update helper at {}: {error}",
                helper.display()
            )
        })?;
    }

    Ok(helper)
}

pub fn install_launched_update_asset() -> Result<bool, String> {
    let current_exe = env::current_exe().map_err(|error| error.to_string())?;
    let Some(file_name) = current_exe.file_name().and_then(|name| name.to_str()) else {
        return Ok(false);
    };
    if !is_versioned_update_asset_name(file_name) {
        return Ok(false);
    }

    let install_dir = app_install_dir()?;
    fs::create_dir_all(&install_dir).map_err(|error| error.to_string())?;
    let final_path = install_dir.join(INSTALLED_EXE_NAME);
    if same_path(&current_exe, &final_path) {
        return Ok(false);
    }

    let partial_path = install_dir.join(PARTIAL_EXE_NAME);
    fs::copy(&current_exe, &partial_path).map_err(|error| {
        format!(
            "Could not stage the update at {}: {error}",
            partial_path.display()
        )
    })?;
    replace_installed_exe(&partial_path, &final_path)?;
    create_desktop_shortcut(&final_path)?;
    open_installed_app(&final_path)?;
    Ok(true)
}

pub struct UpdatingApp {
    download_url: String,
    asset_name: String,
    status: String,
    started: bool,
    failed: bool,
    result_rx: Option<Receiver<Result<PathBuf, String>>>,
}

impl UpdatingApp {
    pub fn new(download_url: String, _release_url: String, asset_name: String) -> Self {
        Self {
            download_url,
            asset_name,
            status: format!("Preparing update install in {}...", app_install_dir_label()),
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
        self.status = format!(
            "Downloading {} and installing it as {}...",
            self.asset_name, INSTALLED_EXE_NAME
        );
        let download_url = self.download_url.clone();
        let asset_name = self.asset_name.clone();
        let (tx, rx) = mpsc::channel();
        thread::spawn(move || {
            let result = install_update(&download_url, &asset_name);
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
                    Ok(path) => match open_installed_app(&path) {
                        Ok(()) => {
                            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
                            format!("Installed {} and opened the new version.", path.display())
                        }
                        Err(error) => {
                            self.failed = true;
                            format!(
                                "Installed {}, but could not open it: {error}",
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
                            RichText::new(INSTALLED_EXE_NAME)
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
                    if self.failed && ui.button("Retry").clicked() {
                        self.retry();
                    }
                });
            });

        ctx.request_repaint_after(Duration::from_millis(150));
    }
}

fn install_update(download_url: &str, asset_name: &str) -> Result<PathBuf, String> {
    if !download_url.starts_with("https://") {
        return Err("The release asset URL is not a secure GitHub download URL.".to_owned());
    }
    if !is_installable_asset_name(asset_name) {
        return Err(format!(
            "{asset_name} is not a supported Windows executable asset."
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

    let install_dir = app_install_dir()?;
    fs::create_dir_all(&install_dir).map_err(|error| error.to_string())?;
    let final_path = install_dir.join(INSTALLED_EXE_NAME);
    let partial_path = install_dir.join(PARTIAL_EXE_NAME);

    let bytes = response.bytes().map_err(|error| error.to_string())?;
    fs::write(&partial_path, bytes).map_err(|error| error.to_string())?;
    replace_installed_exe(&partial_path, &final_path)?;
    create_desktop_shortcut(&final_path)?;

    Ok(final_path)
}

fn app_install_dir() -> Result<PathBuf, String> {
    settings_path()
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "Could not resolve the Chosen Visualizer settings folder.".to_owned())
}

fn app_install_dir_label() -> String {
    app_install_dir()
        .map(|path| path.display().to_string())
        .unwrap_or_else(|error| error)
}

fn replace_installed_exe(partial_path: &Path, final_path: &Path) -> Result<(), String> {
    let mut last_error = String::new();

    for _ in 0..24 {
        match fs::remove_file(final_path) {
            Ok(()) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => {
                last_error = error.to_string();
                thread::sleep(Duration::from_millis(250));
                continue;
            }
        }

        match fs::rename(partial_path, final_path) {
            Ok(()) => return Ok(()),
            Err(error) => {
                last_error = error.to_string();
                thread::sleep(Duration::from_millis(250));
            }
        }
    }

    Err(format!(
        "Could not replace {}. Close any running Chosen Visualizer windows and retry. Last error: {last_error}",
        final_path.display()
    ))
}

fn open_installed_app(path: &Path) -> Result<(), String> {
    let mut command = Command::new(path);
    if let Some(parent) = path.parent() {
        command.current_dir(parent);
    }
    command
        .spawn()
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn create_desktop_shortcut(target: &Path) -> Result<(), String> {
    let target = target
        .canonicalize()
        .unwrap_or_else(|_| target.to_path_buf());
    let working_directory = target
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| PathBuf::from("."));
    let icon_location = format!("{},0", target.display());
    let script = format!(
        "$desktop = [Environment]::GetFolderPath('Desktop'); \
         $shortcutPath = Join-Path $desktop {}; \
         $shell = New-Object -ComObject WScript.Shell; \
         $shortcut = $shell.CreateShortcut($shortcutPath); \
         $shortcut.TargetPath = {}; \
         $shortcut.WorkingDirectory = {}; \
         $shortcut.IconLocation = {}; \
         $shortcut.Save();",
        powershell_quote(DESKTOP_SHORTCUT_NAME),
        powershell_quote(&target.display().to_string()),
        powershell_quote(&working_directory.display().to_string()),
        powershell_quote(&icon_location),
    );

    let status = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .status()
        .map_err(|error| error.to_string())?;

    if status.success() {
        Ok(())
    } else {
        Err(format!("Could not create the desktop shortcut: {status}"))
    }
}

#[cfg(not(windows))]
fn create_desktop_shortcut(_target: &Path) -> Result<(), String> {
    Ok(())
}

#[cfg(windows)]
fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace("'", "''"))
}

fn same_path(left: &Path, right: &Path) -> bool {
    let left = left.canonicalize().unwrap_or_else(|_| left.to_path_buf());
    let right = right.canonicalize().unwrap_or_else(|_| right.to_path_buf());

    if cfg!(windows) {
        left.to_string_lossy()
            .eq_ignore_ascii_case(&right.to_string_lossy())
    } else {
        left == right
    }
}

#[cfg(test)]
mod tests {
    use super::{
        GithubRelease, compare_release_candidates, compare_versions, is_installable_asset_name,
    };
    use std::cmp::Ordering;

    #[test]
    fn compares_version_tags() {
        assert_eq!(compare_versions("v1.0.5", "1.0.4"), Ordering::Greater);
        assert_eq!(compare_versions("1.0.4", "v1.0.4"), Ordering::Equal);
        assert_eq!(compare_versions("v1.0.3", "1.0.4"), Ordering::Less);
    }

    #[test]
    fn prefers_stable_release_when_versions_match() {
        let stable = test_release("v1.0.5", false);
        let prerelease = test_release("v1.0.5-test", true);
        assert_eq!(
            compare_release_candidates(&stable, &prerelease),
            Ordering::Greater
        );
    }

    fn test_release(tag_name: &str, prerelease: bool) -> GithubRelease {
        GithubRelease {
            tag_name: tag_name.to_owned(),
            name: None,
            html_url: String::new(),
            draft: false,
            prerelease,
            assets: Vec::new(),
        }
    }

    #[test]
    fn only_installs_windows_exe_assets() {
        assert!(is_installable_asset_name("chosen-visualizer-v1.0.5.exe"));
        assert!(is_installable_asset_name("ChosenVisualizerSetup.exe"));
        assert!(!is_installable_asset_name("ChosenVisualizerSetup.msi"));
        assert!(!is_installable_asset_name("source.zip"));
        assert!(!is_installable_asset_name("chosen-visualizer.pdb"));
    }

    #[test]
    fn recognizes_versioned_update_assets() {
        assert!(super::is_versioned_update_asset_name(
            "chosen-visualizer-v1.0.5.exe"
        ));
        assert!(!super::is_versioned_update_asset_name(
            "chosen-visualizer.exe"
        ));
        assert!(!super::is_versioned_update_asset_name(
            "chosen-visualizer-updater.exe"
        ));
    }
}
