# Changelog

## 1.0.5 - 2026-07-03

- Adds a self-install bridge for versioned GitHub release executables launched by older updaters.
- Installs a directly launched release executable into `%APPDATA%\Chosen Visualizer\chosen-visualizer.exe`.
- Updates the desktop shortcut and opens the stable installed executable after self-installing.
## 1.0.4 - 2026-07-03

- Installs downloaded updates into the Chosen Visualizer settings folder instead of the system temp folder.
- Saves the installed application as `chosen-visualizer.exe` without a version number in the filename.
- Runs updates through `chosen-visualizer-updater.exe` in the same folder so the main executable can be replaced.
- Creates or updates the desktop shortcut to point at the installed executable.
- Opens the newly installed executable after the old app closes.
## 1.0.3 - 2026-07-03

- Published the updater flow against public GitHub releases, with no embedded GitHub token support.
- Added a restrictive All Rights Reserved repository license and README license notice.
- Improved update detection to prefer stable releases over matching pre-release test tags.
- Removed the release-page action from the update UI so update prompts stay focused on installing.
- Changed install actions so launching the updater closes the active widget/settings windows.
- Changed the updater window so it closes itself after successfully starting the downloaded update executable or installer.

## 1.0.2

- Added GitHub release update detection with a dedicated updater window.
- Added support for multiple visualizers on screen at the same time.
- Version labels now come from Cargo package metadata.
