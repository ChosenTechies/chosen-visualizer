#[cfg(windows)]
use crate::settings::settings_path;
#[cfg(windows)]
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};
use std::{env, path::PathBuf};
#[cfg(windows)]
use std::{thread, time::Duration};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrayCommand {
    None,
    Show,
    OpenSettings,
    Hide,
    Quit,
}

#[derive(Clone, Default)]
pub struct TrayController {
    #[cfg(windows)]
    command: Arc<AtomicU8>,
}

impl TrayController {
    #[cfg(windows)]
    pub fn take_command(&self) -> TrayCommand {
        match self.command.swap(0, Ordering::SeqCst) {
            1 => TrayCommand::Show,
            2 => TrayCommand::OpenSettings,
            3 => TrayCommand::Hide,
            4 => TrayCommand::Quit,
            _ => TrayCommand::None,
        }
    }

    #[cfg(not(windows))]
    pub fn take_command(&self) -> TrayCommand {
        TrayCommand::None
    }
}

#[cfg(windows)]
pub fn init(ctx: eframe::egui::Context) -> TrayController {
    let command = Arc::new(AtomicU8::new(0));
    let command_for_thread = command.clone();

    thread::spawn(move || {
        use tray_item::TrayItem;

        let mut tray = match TrayItem::new("Chosen Visualizer", load_icon_source()) {
            Ok(tray) => tray,
            Err(_) => return,
        };

        let _ = tray.add_label("Running in background");

        let show_cmd = command_for_thread.clone();
        let show_ctx = ctx.clone();
        let _ = tray.add_menu_item("Show", move || {
            show_cmd.store(1, Ordering::SeqCst);
            show_ctx.request_repaint();
        });

        let settings_cmd = command_for_thread.clone();
        let settings_ctx = ctx.clone();
        let _ = tray.add_menu_item("Open settings", move || {
            settings_cmd.store(2, Ordering::SeqCst);
            settings_ctx.request_repaint();
        });

        let hide_cmd = command_for_thread.clone();
        let hide_ctx = ctx.clone();
        let _ = tray.add_menu_item("Hide", move || {
            hide_cmd.store(3, Ordering::SeqCst);
            hide_ctx.request_repaint();
        });

        let quit_cmd = command_for_thread.clone();
        let quit_ctx = ctx.clone();
        let _ = tray.add_menu_item("Quit", move || {
            quit_cmd.store(4, Ordering::SeqCst);
            quit_ctx.request_repaint();
        });

        loop {
            thread::sleep(Duration::from_secs(3600));
        }
    });

    TrayController { command }
}

#[cfg(windows)]
fn load_icon_source() -> tray_item::IconSource {
    use tray_item::IconSource;
    use windows_sys::Win32::UI::WindowsAndMessaging::{IDI_APPLICATION, LoadIconW};

    if let Some(ico_path) = ensure_logo_ico() {
        if let Some(hicon) = load_hicon_from_file(&ico_path) {
            return IconSource::RawIcon(hicon);
        }
    }

    for path in icon_candidates() {
        if let Some(hicon) = load_hicon_from_file(&path) {
            return IconSource::RawIcon(hicon);
        }
    }

    // Keep tray available even if custom icon file cannot be loaded.
    let fallback = unsafe { LoadIconW(std::ptr::null_mut(), IDI_APPLICATION) } as isize;
    IconSource::RawIcon(fallback)
}

#[cfg(windows)]
fn icon_candidates() -> Vec<PathBuf> {
    let mut out = Vec::new();
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("chosen-visualizer.ico"));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        out.push(cwd.join("chosen-visualizer.ico"));
    }
    if let Ok(exe) = env::current_exe() {
        if let Some(dir) = exe.parent() {
            out.push(dir.join("chosen-visualizer.png"));
        }
    }
    if let Ok(cwd) = env::current_dir() {
        out.push(cwd.join("chosen-visualizer.png"));
    }
    out
}

#[cfg(windows)]
fn load_hicon_from_file(path: &std::path::Path) -> Option<isize> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        IMAGE_ICON, LR_DEFAULTSIZE, LR_LOADFROMFILE, LoadImageW,
    };

    let wide: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();

    let handle = unsafe {
        LoadImageW(
            std::ptr::null_mut(),
            wide.as_ptr(),
            IMAGE_ICON,
            0,
            0,
            LR_LOADFROMFILE | LR_DEFAULTSIZE,
        )
    };

    if handle.is_null() {
        None
    } else {
        Some(handle as isize)
    }
}

#[cfg(windows)]
fn ensure_logo_ico() -> Option<PathBuf> {
    use ico::{IconDir, IconDirEntry, IconImage, ResourceType};
    use image::imageops::FilterType;

    let source = image::load_from_memory(include_bytes!("../chosen-visualizer.png"))
        .ok()?
        .to_rgba8();
    let mut icon_dir = IconDir::new(ResourceType::Icon);
    for size in [16_u32, 24, 32, 48, 64, 128, 256] {
        let resized = image::imageops::resize(&source, size, size, FilterType::Lanczos3);
        let image = IconImage::from_rgba_data(size, size, resized.into_raw());
        let entry = IconDirEntry::encode(&image).ok()?;
        icon_dir.add_entry(entry);
    }

    let out_path = settings_path()
        .parent()
        .map(|path| path.join("chosen-visualizer-tray.ico"))
        .unwrap_or_else(|| PathBuf::from("chosen-visualizer-tray.ico"));
    if let Some(parent) = out_path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    let mut file = std::fs::File::create(&out_path).ok()?;
    icon_dir.write(&mut file).ok()?;
    Some(out_path)
}

#[cfg(not(windows))]
pub fn init(_ctx: eframe::egui::Context) -> TrayController {
    TrayController::default()
}
