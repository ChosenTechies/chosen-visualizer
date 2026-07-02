#[cfg(not(windows))]
fn main() {
    eprintln!("Chosen Visualizer installer is Windows-only.");
    std::process::exit(1);
}

#[cfg(windows)]
fn main() {
    if let Err(error) = windows_installer::run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
}

#[cfg(windows)]
mod windows_installer {
    use std::{
        env,
        error::Error,
        ffi::OsStr,
        fs, io,
        os::windows::ffi::OsStrExt,
        path::{Path, PathBuf},
        process::Command as ProcessCommand,
    };

    use windows::{
        Win32::{
            Foundation::{ERROR_FILE_NOT_FOUND, ERROR_PATH_NOT_FOUND, ERROR_SUCCESS, WIN32_ERROR},
            System::{
                Com::{
                    CLSCTX_INPROC_SERVER, COINIT_APARTMENTTHREADED, CoCreateInstance,
                    CoInitializeEx, CoUninitialize, IPersistFile,
                },
                Registry::{
                    HKEY, HKEY_CURRENT_USER, KEY_SET_VALUE, REG_DWORD, REG_OPTION_NON_VOLATILE,
                    REG_SZ, RegCloseKey, RegCreateKeyExW, RegDeleteTreeW, RegDeleteValueW,
                    RegSetValueExW,
                },
            },
            UI::Shell::{IShellLinkW, ShellLink},
        },
        core::{Interface, PCWSTR},
    };

    const APP_NAME: &str = "Chosen Visualizer";
    const APP_VERSION: &str = "1.0.0";
    const APP_EXE: &str = "chosen-visualizer.exe";
    const INSTALLER_EXE: &str = "chosen-visualizer-installer.exe";
    const START_MENU_LINK: &str = "Chosen Visualizer.lnk";
    const DESKTOP_LINK: &str = "Chosen Visualizer.lnk";
    const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
    const UNINSTALL_KEY: &str =
        r"Software\Microsoft\Windows\CurrentVersion\Uninstall\Chosen Visualizer";

    type AppResult<T> = Result<T, Box<dyn Error>>;

    pub fn run() -> AppResult<()> {
        let options = Options::parse()?;

        match options.mode {
            Mode::Install => install(options),
            Mode::Uninstall => uninstall(),
            Mode::Help => {
                print_help();
                Ok(())
            }
        }
    }

    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Mode {
        Install,
        Uninstall,
        Help,
    }

    #[derive(Debug)]
    struct Options {
        mode: Mode,
        source: Option<PathBuf>,
        desktop_shortcut: bool,
        start_menu_shortcut: bool,
        startup: bool,
        launch: bool,
    }

    impl Options {
        fn parse() -> AppResult<Self> {
            let mut options = Self {
                mode: Mode::Install,
                source: None,
                desktop_shortcut: true,
                start_menu_shortcut: true,
                startup: false,
                launch: false,
            };

            let mut args = env::args_os().skip(1);
            while let Some(arg) = args.next() {
                let text = arg.to_string_lossy();
                match text.as_ref() {
                    "-h" | "--help" => options.mode = Mode::Help,
                    "--uninstall" => options.mode = Mode::Uninstall,
                    "--no-desktop-shortcut" => options.desktop_shortcut = false,
                    "--no-start-menu-shortcut" => options.start_menu_shortcut = false,
                    "--startup" => options.startup = true,
                    "--launch" => options.launch = true,
                    "--source" => {
                        let value = args
                            .next()
                            .ok_or_else(|| invalid("--source requires a path"))?;
                        options.source = Some(PathBuf::from(value));
                    }
                    value if value.starts_with("--source=") => {
                        options.source = Some(PathBuf::from(&value["--source=".len()..]));
                    }
                    value if !value.starts_with('-') && options.source.is_none() => {
                        options.source = Some(PathBuf::from(arg));
                    }
                    value => return Err(invalid(format!("unknown option: {value}")).into()),
                }
            }

            Ok(options)
        }
    }

    fn install(options: Options) -> AppResult<()> {
        let source_exe = locate_app_exe(options.source.as_deref())?;
        let install_dir = install_dir()?;
        fs::create_dir_all(&install_dir)?;

        let target_exe = install_dir.join(APP_EXE);
        copy_file_if_different(&source_exe, &target_exe)?;

        let installed_installer = install_dir.join(INSTALLER_EXE);
        let current_installer = env::current_exe()?;
        copy_file_if_different(&current_installer, &installed_installer)?;

        if options.start_menu_shortcut {
            let start_menu_dir = start_menu_dir()?.join(APP_NAME);
            fs::create_dir_all(&start_menu_dir)?;
            create_shortcut(
                &start_menu_dir.join(START_MENU_LINK),
                &target_exe,
                &install_dir,
                "Audio-reactive desktop visualizer",
            )?;
        }

        if options.desktop_shortcut {
            create_shortcut(
                &desktop_dir()?.join(DESKTOP_LINK),
                &target_exe,
                &install_dir,
                "Audio-reactive desktop visualizer",
            )?;
        }

        if options.startup {
            set_startup_entry(&target_exe)?;
        }

        register_uninstall_entry(&install_dir, &target_exe, &installed_installer)?;

        println!("Installed {APP_NAME} to {}", install_dir.display());
        println!(
            "Run it from the Start Menu, desktop shortcut, or {}",
            target_exe.display()
        );

        if options.launch {
            ProcessCommand::new(&target_exe)
                .current_dir(&install_dir)
                .spawn()?;
        }

        Ok(())
    }

    fn uninstall() -> AppResult<()> {
        let install_dir = install_dir()?;
        let target_exe = install_dir.join(APP_EXE);
        let installed_installer = install_dir.join(INSTALLER_EXE);
        let current_exe = env::current_exe().ok();

        remove_file_if_exists(start_menu_dir()?.join(APP_NAME).join(START_MENU_LINK))?;
        remove_dir_if_empty(start_menu_dir()?.join(APP_NAME))?;
        remove_file_if_exists(desktop_dir()?.join(DESKTOP_LINK))?;
        delete_startup_entry()?;
        delete_uninstall_entry()?;

        remove_file_if_exists(&target_exe)?;
        if current_exe
            .as_ref()
            .is_none_or(|current| !same_file(current, &installed_installer))
        {
            remove_file_if_exists(&installed_installer)?;
        }
        remove_dir_if_empty(&install_dir)?;

        println!("Uninstalled {APP_NAME}.");
        Ok(())
    }

    fn locate_app_exe(explicit: Option<&Path>) -> AppResult<PathBuf> {
        if let Some(path) = explicit {
            return existing_file(path);
        }

        let current_dir = env::current_dir()?;
        let current_exe_dir = env::current_exe()?
            .parent()
            .map(Path::to_path_buf)
            .unwrap_or_else(|| current_dir.clone());

        let candidates = [
            current_dir.join("target").join("release").join(APP_EXE),
            current_dir
                .parent()
                .unwrap_or(&current_dir)
                .join("target")
                .join("release")
                .join(APP_EXE),
            current_exe_dir.join(APP_EXE),
            current_exe_dir
                .parent()
                .unwrap_or(&current_exe_dir)
                .join(APP_EXE),
        ];

        candidates
            .into_iter()
            .find(|path| path.is_file())
            .ok_or_else(|| {
                invalid(format!(
                    "could not find {APP_EXE}; build the app with `cargo build --release` or pass --source <path>"
                ))
                .into()
            })
    }

    fn existing_file(path: &Path) -> AppResult<PathBuf> {
        if path.is_file() {
            Ok(path.to_path_buf())
        } else {
            Err(invalid(format!(
                "source executable does not exist: {}",
                path.display()
            ))
            .into())
        }
    }

    fn install_dir() -> AppResult<PathBuf> {
        Ok(env_path("LOCALAPPDATA")?.join("Programs").join(APP_NAME))
    }

    fn start_menu_dir() -> AppResult<PathBuf> {
        Ok(env_path("APPDATA")?
            .join("Microsoft")
            .join("Windows")
            .join("Start Menu")
            .join("Programs"))
    }

    fn desktop_dir() -> AppResult<PathBuf> {
        Ok(env_path("USERPROFILE")?.join("Desktop"))
    }

    fn env_path(name: &str) -> AppResult<PathBuf> {
        env::var_os(name)
            .map(PathBuf::from)
            .ok_or_else(|| invalid(format!("{name} is not set")).into())
    }

    fn copy_file_if_different(source: &Path, target: &Path) -> AppResult<()> {
        if same_file(source, target) {
            return Ok(());
        }
        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source, target)?;
        Ok(())
    }

    fn same_file(left: &Path, right: &Path) -> bool {
        let Ok(left) = left.canonicalize() else {
            return false;
        };
        let Ok(right) = right.canonicalize() else {
            return false;
        };
        left == right
    }

    fn create_shortcut(
        shortcut_path: &Path,
        target_path: &Path,
        working_dir: &Path,
        description: &str,
    ) -> AppResult<()> {
        let _com = ComApartment::new()?;
        let link: IShellLinkW =
            unsafe { CoCreateInstance(&ShellLink, None, CLSCTX_INPROC_SERVER)? };

        let target_wide = wide_null(target_path.as_os_str());
        let working_wide = wide_null(working_dir.as_os_str());
        let description_wide = wide_null(description);

        unsafe {
            link.SetPath(PCWSTR(target_wide.as_ptr()))?;
            link.SetWorkingDirectory(PCWSTR(working_wide.as_ptr()))?;
            link.SetDescription(PCWSTR(description_wide.as_ptr()))?;
            link.SetIconLocation(PCWSTR(target_wide.as_ptr()), 0)?;
        }

        if let Some(parent) = shortcut_path.parent() {
            fs::create_dir_all(parent)?;
        }

        let persist: IPersistFile = link.cast()?;
        let shortcut_wide = wide_null(shortcut_path.as_os_str());
        unsafe {
            persist.Save(PCWSTR(shortcut_wide.as_ptr()), true)?;
        }

        Ok(())
    }

    fn set_startup_entry(target_exe: &Path) -> AppResult<()> {
        let command = quote_path(target_exe);
        with_reg_key(RUN_KEY, |key| write_reg_string(key, APP_NAME, &command))
    }

    fn delete_startup_entry() -> AppResult<()> {
        with_reg_key(RUN_KEY, |key| {
            let value_name = wide_null(APP_NAME);
            let status = unsafe { RegDeleteValueW(key, PCWSTR(value_name.as_ptr())) };
            ignore_not_found(status, "delete startup registry value")
        })
    }

    fn register_uninstall_entry(
        install_dir: &Path,
        target_exe: &Path,
        installed_installer: &Path,
    ) -> AppResult<()> {
        let uninstall_command = format!("{} --uninstall", quote_path(installed_installer));

        with_reg_key(UNINSTALL_KEY, |key| {
            write_reg_string(key, "DisplayName", APP_NAME)?;
            write_reg_string(key, "DisplayVersion", APP_VERSION)?;
            write_reg_string(key, "Publisher", "Chosen Visualizer")?;
            write_reg_string(key, "InstallLocation", &install_dir.display().to_string())?;
            write_reg_string(key, "DisplayIcon", &target_exe.display().to_string())?;
            write_reg_string(key, "UninstallString", &uninstall_command)?;
            write_reg_dword(key, "NoModify", 1)?;
            write_reg_dword(key, "NoRepair", 1)
        })
    }

    fn delete_uninstall_entry() -> AppResult<()> {
        let key_name = wide_null(UNINSTALL_KEY);
        let status = unsafe { RegDeleteTreeW(HKEY_CURRENT_USER, PCWSTR(key_name.as_ptr())) };
        ignore_not_found(status, "delete uninstall registry key")
    }

    fn with_reg_key<F>(subkey: &str, f: F) -> AppResult<()>
    where
        F: FnOnce(HKEY) -> AppResult<()>,
    {
        let subkey_wide = wide_null(subkey);
        let mut key = HKEY(std::ptr::null_mut());
        let status = unsafe {
            RegCreateKeyExW(
                HKEY_CURRENT_USER,
                PCWSTR(subkey_wide.as_ptr()),
                0,
                PCWSTR::null(),
                REG_OPTION_NON_VOLATILE,
                KEY_SET_VALUE,
                None,
                &mut key,
                None,
            )
        };
        win32_status(status, "open registry key")?;

        let result = f(key);
        unsafe {
            let _ = RegCloseKey(key);
        }
        result
    }

    fn write_reg_string(key: HKEY, name: &str, value: &str) -> AppResult<()> {
        let name_wide = wide_null(name);
        let value_wide = wide_null(value);
        let bytes = wide_as_bytes(&value_wide);
        let status =
            unsafe { RegSetValueExW(key, PCWSTR(name_wide.as_ptr()), 0, REG_SZ, Some(bytes)) };
        win32_status(status, "write registry string")
    }

    fn write_reg_dword(key: HKEY, name: &str, value: u32) -> AppResult<()> {
        let name_wide = wide_null(name);
        let bytes = value.to_le_bytes();
        let status =
            unsafe { RegSetValueExW(key, PCWSTR(name_wide.as_ptr()), 0, REG_DWORD, Some(&bytes)) };
        win32_status(status, "write registry dword")
    }

    fn win32_status(status: WIN32_ERROR, action: &str) -> AppResult<()> {
        if status == ERROR_SUCCESS {
            Ok(())
        } else {
            let source = io::Error::from_raw_os_error(status.0 as i32);
            Err(io::Error::new(source.kind(), format!("{action}: {source}")).into())
        }
    }

    fn ignore_not_found(status: WIN32_ERROR, action: &str) -> AppResult<()> {
        if status == ERROR_SUCCESS
            || status == ERROR_FILE_NOT_FOUND
            || status == ERROR_PATH_NOT_FOUND
        {
            Ok(())
        } else {
            win32_status(status, action)
        }
    }

    fn remove_file_if_exists(path: impl AsRef<Path>) -> AppResult<()> {
        match fs::remove_file(path.as_ref()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn remove_dir_if_empty(path: impl AsRef<Path>) -> AppResult<()> {
        match fs::remove_dir(path.as_ref()) {
            Ok(()) => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(()),
            Err(error) if error.kind() == io::ErrorKind::DirectoryNotEmpty => Ok(()),
            Err(error) => Err(error.into()),
        }
    }

    fn quote_path(path: &Path) -> String {
        format!("\"{}\"", path.display())
    }

    fn wide_null(value: impl AsRef<OsStr>) -> Vec<u16> {
        value
            .as_ref()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect()
    }

    fn wide_as_bytes(value: &[u16]) -> &[u8] {
        unsafe {
            std::slice::from_raw_parts(value.as_ptr().cast::<u8>(), std::mem::size_of_val(value))
        }
    }

    fn invalid(message: impl Into<String>) -> io::Error {
        io::Error::new(io::ErrorKind::InvalidInput, message.into())
    }

    struct ComApartment;

    impl ComApartment {
        fn new() -> windows::core::Result<Self> {
            unsafe {
                CoInitializeEx(None, COINIT_APARTMENTTHREADED).ok()?;
            }
            Ok(Self)
        }
    }

    impl Drop for ComApartment {
        fn drop(&mut self) {
            unsafe {
                CoUninitialize();
            }
        }
    }

    fn print_help() {
        println!(
            "{APP_NAME} Installer\n\n\
Usage:\n  chosen-visualizer-installer.exe [options]\n\n\
Options:\n  --source <path>              Path to chosen-visualizer.exe\n  \
--no-desktop-shortcut       Do not create a desktop shortcut\n  \
--no-start-menu-shortcut    Do not create a Start Menu shortcut\n  \
--startup                   Add a per-user startup registry entry\n  \
--launch                    Launch the app after installing\n  \
--uninstall                 Remove installed files and registry entries\n  \
-h, --help                  Show this help"
        );
    }
}
