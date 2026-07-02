# Chosen Visualizer

Chosen Visualizer is a native Rust desktop audio visualizer for Windows. It captures the default system output through WASAPI loopback, analyzes it with an FFT, and renders a customizable dark themed visualizer with no neon/glow styling.

## Features

- Reacts to system/app audio through Windows WASAPI loopback.
- Preview signal fallback if loopback capture is unavailable.
- Visualizer modes: bars, mirrored bars, waveform, radial, and particles.
- Multiple visualizers can be shown on screen at the same time.
- Settings for bands, sensitivity, smoothing, noise gate, bass boost, falloff, line width, opacity, and FPS.
- Early-access update checks detect newer GitHub releases and open a dedicated updater window.
- Muted color presets plus custom color selection.
- Desktop widget mode: place the visualizer anywhere with X/Y/width/height controls, hide the app controls, and let it keep running as a desktop overlay.
- `F10` restores controls if the visualizer is running alone.
- Always-on-top, frameless window, click-through overlay, and taskbar strip mode.
- Taskbar strip mode parks the visualizer along the selected work-area edge. It does not inject into the Explorer taskbar process.
- Settings persist to `%APPDATA%\Chosen Visualizer\settings.toml` on Windows.

## Run

```powershell
cargo run
```

If Cargo is not on this shell's PATH, use:

```powershell
C:\Users\urmot\.cargo\bin\cargo.exe run
```

## Build

```powershell
cargo build --release
```

The release executable is written to `target\release\chosen-visualizer.exe`.

## Notes

- Click-through overlay makes the window ignore mouse input. Press `F10` first if the window still has focus, or edit `%APPDATA%\Chosen Visualizer\settings.toml` and set `click_through = false`.
- Desktop widget mode positions the app window like a desktop visualizer. It does not become part of Explorer's desktop icon layer.
- System audio capture is Windows-specific. Other targets build with a preview signal fallback.
