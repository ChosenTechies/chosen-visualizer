use eframe::egui::Color32;
use serde::{Deserialize, Serialize};
use std::{env, fs, path::PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum VisualizerMode {
    Bars,
    MirrorBars,
    Waveform,
    Radial,
    Particles,
}

impl VisualizerMode {
    pub const ALL: [Self; 5] = [
        Self::Bars,
        Self::MirrorBars,
        Self::Waveform,
        Self::Radial,
        Self::Particles,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Bars => "Bars",
            Self::MirrorBars => "Mirror bars",
            Self::Waveform => "Waveform",
            Self::Radial => "Radial",
            Self::Particles => "Particles",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ColorPreset {
    Slate,
    Graphite,
    Ember,
    Forest,
    Steel,
    CoverArt,
    Custom,
}

impl ColorPreset {
    pub const ALL: [Self; 7] = [
        Self::Slate,
        Self::Graphite,
        Self::Ember,
        Self::Forest,
        Self::Steel,
        Self::CoverArt,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Slate => "Slate",
            Self::Graphite => "Graphite",
            Self::Ember => "Ember",
            Self::Forest => "Forest",
            Self::Steel => "Steel",
            Self::CoverArt => "Cover art (coming soon)",
            Self::Custom => "Custom",
        }
    }

    pub fn colors(self, custom: [u8; 3]) -> (Color32, Color32) {
        match self {
            Self::Slate => (
                Color32::from_rgb(184, 196, 211),
                Color32::from_rgb(88, 111, 140),
            ),
            Self::Graphite => (
                Color32::from_rgb(224, 224, 220),
                Color32::from_rgb(112, 112, 108),
            ),
            Self::Ember => (
                Color32::from_rgb(226, 167, 118),
                Color32::from_rgb(146, 80, 54),
            ),
            Self::Forest => (
                Color32::from_rgb(167, 202, 171),
                Color32::from_rgb(74, 118, 91),
            ),
            Self::Steel => (
                Color32::from_rgb(166, 187, 198),
                Color32::from_rgb(72, 99, 114),
            ),
            Self::CoverArt => (
                Color32::from_rgb(198, 111, 194),
                Color32::from_rgb(104, 55, 121),
            ),
            Self::Custom => {
                let primary = Color32::from_rgb(custom[0], custom[1], custom[2]);
                let secondary = Color32::from_rgb(
                    ((custom[0] as u16 * 3 + 36) / 5).min(255) as u8,
                    ((custom[1] as u16 * 3 + 36) / 5).min(255) as u8,
                    ((custom[2] as u16 * 3 + 36) / 5).min(255) as u8,
                );
                (primary, secondary)
            }
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaskbarEdge {
    Bottom,
    Top,
    Left,
    Right,
}

impl TaskbarEdge {
    pub const ALL: [Self; 4] = [Self::Bottom, Self::Top, Self::Left, Self::Right];

    pub fn label(self) -> &'static str {
        match self {
            Self::Bottom => "Bottom",
            Self::Top => "Top",
            Self::Left => "Left",
            Self::Right => "Right",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub mode: VisualizerMode,
    pub color_preset: ColorPreset,
    pub custom_color: [u8; 3],
    pub bars: usize,
    pub smoothing: f32,
    pub sensitivity: f32,
    pub noise_gate: f32,
    pub bass_boost: f32,
    pub falloff: f32,
    pub line_width: f32,
    pub fill_opacity: f32,
    pub background_alpha: f32,
    pub show_grid: bool,
    pub bar_gap: f32,
    pub bar_rounding: f32,
    pub glow_strength: f32,
    pub color_drift: f32,
    pub show_settings: bool,
    pub show_top_bar: bool,
    pub show_peaks: bool,
    pub show_meter: bool,
    pub visualizer_only_widget: bool,
    pub compact_controls: bool,
    pub always_on_top: bool,
    pub click_through: bool,
    pub frameless: bool,
    pub desktop_widget: bool,
    pub desktop_only: bool,
    pub cover_art_source: String,
    pub desktop_x: i32,
    pub desktop_y: i32,
    pub desktop_width: i32,
    pub desktop_height: i32,
    pub taskbar_strip: bool,
    pub taskbar_edge: TaskbarEdge,
    pub strip_thickness: f32,
    pub target_fps: u32,
    #[serde(default = "default_visualizer_count")]
    pub visualizer_count: usize,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            mode: VisualizerMode::Bars,
            color_preset: ColorPreset::Custom,
            custom_color: [194, 74, 190],
            bars: 72,
            smoothing: 0.72,
            sensitivity: 1.35,
            noise_gate: 0.015,
            bass_boost: 1.18,
            falloff: 0.90,
            line_width: 2.0,
            fill_opacity: 0.82,
            background_alpha: 0.0,
            show_grid: true,
            bar_gap: 3.0,
            bar_rounding: 0.45,
            glow_strength: 0.18,
            color_drift: 0.08,
            show_settings: true,
            show_top_bar: true,
            show_peaks: false,
            show_meter: true,
            visualizer_only_widget: true,
            compact_controls: false,
            always_on_top: false,
            click_through: false,
            frameless: false,
            desktop_widget: true,
            desktop_only: true,
            cover_art_source: String::new(),
            desktop_x: 120,
            desktop_y: 120,
            desktop_width: 760,
            desktop_height: 260,
            taskbar_strip: false,
            taskbar_edge: TaskbarEdge::Bottom,
            strip_thickness: 96.0,
            target_fps: 60,
            visualizer_count: 1,
        }
    }
}

impl Settings {
    pub fn load() -> Self {
        let path = settings_path();
        let loaded = fs::read_to_string(&path)
            .ok()
            .and_then(|text| toml::from_str::<Settings>(&text).ok());
        let mut settings = loaded.unwrap_or_default();
        settings.normalize();
        settings
    }

    pub fn save(&self) {
        let path = settings_path();
        if let Some(parent) = path.parent() {
            let _ = fs::create_dir_all(parent);
        }
        if let Ok(text) = toml::to_string_pretty(self) {
            let _ = fs::write(path, text);
        }
    }

    pub fn normalize(&mut self) {
        self.bars = self.bars.clamp(16, 192);
        self.smoothing = self.smoothing.clamp(0.0, 0.96);
        self.sensitivity = self.sensitivity.clamp(0.2, 5.0);
        self.noise_gate = self.noise_gate.clamp(0.0, 0.2);
        self.bass_boost = self.bass_boost.clamp(0.5, 3.0);
        self.falloff = self.falloff.clamp(0.0, 0.99);
        self.line_width = self.line_width.clamp(1.0, 8.0);
        self.fill_opacity = self.fill_opacity.clamp(0.1, 1.0);
        self.background_alpha = self.background_alpha.clamp(0.0, 1.0);
        self.bar_gap = self.bar_gap.clamp(0.0, 8.0);
        self.bar_rounding = self.bar_rounding.clamp(0.0, 1.0);
        self.glow_strength = self.glow_strength.clamp(0.0, 1.0);
        self.color_drift = self.color_drift.clamp(0.0, 1.0);
        self.desktop_x = self.desktop_x.clamp(-20000, 20000);
        self.desktop_y = self.desktop_y.clamp(-20000, 20000);
        self.desktop_width = self.desktop_width.clamp(240, 3840);
        self.desktop_height = self.desktop_height.clamp(80, 2160);
        self.strip_thickness = self.strip_thickness.clamp(48.0, 260.0);
        self.target_fps = self.target_fps.clamp(30, 144);
        self.visualizer_count = self.visualizer_count.clamp(1, 6);
        self.taskbar_strip = false;

        if self.desktop_widget {
            self.background_alpha = 0.0;
            self.show_grid = false;
            self.frameless = true;
            if self.visualizer_only_widget {
                self.show_top_bar = false;
                self.click_through = true;
                self.show_meter = false;
            }
        }

        // When "desktop only" is enabled the widget must stay pinned to the
        // desktop and never float above other applications.
        if self.desktop_only {
            self.always_on_top = false;
        }

        self.cover_art_source = self.cover_art_source.trim().to_owned();
        if self.color_preset == ColorPreset::CoverArt {
            self.cover_art_source.clear();
        }
    }

    pub fn accent_colors(&self) -> (Color32, Color32) {
        self.color_preset.colors(self.custom_color)
    }
}

fn default_visualizer_count() -> usize {
    1
}

pub fn settings_path() -> PathBuf {
    #[cfg(windows)]
    {
        if let Ok(appdata) = env::var("APPDATA") {
            return PathBuf::from(appdata)
                .join("Chosen Visualizer")
                .join("settings.toml");
        }
    }

    env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join("chosen_visualizer_settings.toml")
}
