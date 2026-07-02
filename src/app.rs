use crate::{
    audio::{AudioEngine, AudioFrame},
    settings::{ColorPreset, Settings, TaskbarEdge, VisualizerMode, settings_path},
    tray::{self, TrayCommand, TrayController},
    visualizer::VisualizerState,
    window_control::{self, DisplayArea, NativeWindowHandle, WindowFlags},
};
use eframe::{
    egui,
    egui::{Color32, FontFamily, FontId, RichText, Stroke, Vec2},
};
#[cfg(windows)]
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::time::{Duration, Instant};

pub struct ChosenVisualizerApp {
    title: &'static str,
    audio: AudioEngine,
    visualizer: VisualizerState,
    settings_visualizer: VisualizerState,
    settings: Settings,
    native_window: Option<NativeWindowHandle>,
    tray: TrayController,
    last_window_flags: WindowFlags,
    last_frame_at: Instant,
    last_save_at: Instant,
    pending_save: bool,
    show_about: bool,
}

impl ChosenVisualizerApp {
    pub fn new(cc: &eframe::CreationContext<'_>, title: &'static str) -> Self {
        install_style(&cc.egui_ctx);
        let settings = Settings::load();
        let native_window = native_window_handle(cc);
        let tray = tray::init(cc.egui_ctx.clone());
        window_control::apply_egui_viewport(&cc.egui_ctx, &settings);
        window_control::apply_native(native_window, &settings);

        Self {
            title,
            audio: AudioEngine::start(),
            visualizer: VisualizerState::default(),
            settings_visualizer: VisualizerState::default(),
            native_window,
            tray,
            last_window_flags: WindowFlags::from(&settings),
            settings,
            last_frame_at: Instant::now(),
            last_save_at: Instant::now(),
            pending_save: false,
            show_about: false,
        }
    }

    fn mark_changed(&mut self) {
        self.settings.normalize();
        self.pending_save = true;
    }

    fn maybe_save(&mut self) {
        if self.pending_save && self.last_save_at.elapsed() > Duration::from_millis(450) {
            self.settings.save();
            self.pending_save = false;
            self.last_save_at = Instant::now();
        }
    }

    fn apply_window_changes(&mut self, ctx: &egui::Context) {
        let flags = WindowFlags::from(&self.settings);
        if flags != self.last_window_flags {
            window_control::apply_egui_viewport(ctx, &self.settings);
            window_control::apply_native(self.native_window, &self.settings);
            self.last_window_flags = flags;
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        if ctx.input(|input| input.key_pressed(egui::Key::F10)) {
            self.open_settings_view(ctx);
        }
    }

    fn open_settings_view(&mut self, ctx: &egui::Context) {
        self.settings.show_settings = true;
        self.mark_changed();
        self.apply_window_changes(ctx);
        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
        ctx.request_repaint();
    }

    fn handle_tray(&mut self, ctx: &egui::Context) {
        match self.tray.take_command() {
            TrayCommand::None => {}
            TrayCommand::Show => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(true));
                ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
            }
            TrayCommand::OpenSettings => {
                self.open_settings_view(ctx);
            }
            TrayCommand::Hide => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
            }
            TrayCommand::Quit => {
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn top_bar(&mut self, ctx: &egui::Context) {
        if !self.settings.show_top_bar
            || (self.settings.desktop_widget && self.settings.visualizer_only_widget)
        {
            return;
        }

        egui::TopBottomPanel::top("top_bar")
            .exact_height(if self.settings.compact_controls {
                38.0
            } else {
                46.0
            })
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(18, 20, 23))
                    .inner_margin(egui::Margin::symmetric(14.0, 7.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new(self.title)
                            .size(16.0)
                            .color(Color32::from_rgb(232, 233, 234)),
                    );
                    ui.separator();
                    if ui.button("Open settings window").clicked() {
                        self.settings.show_settings = true;
                        self.mark_changed();
                    }
                    if ui.button("Visualizer only").clicked() {
                        self.settings.show_top_bar = false;
                        self.settings.frameless = true;
                        self.settings.desktop_widget = true;
                        self.settings.visualizer_only_widget = true;
                        self.mark_changed();
                    }
                    if ui.button("Hide to tray").clicked() {
                        ctx.send_viewport_cmd(egui::ViewportCommand::Visible(false));
                    }
                    if ui.button("Reset").clicked() {
                        self.settings = Settings::default();
                        self.mark_changed();
                    }
                    if ui.button("About").clicked() {
                        self.settings.show_settings = true;
                        self.show_about = true;
                        self.mark_changed();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(settings_path().display().to_string())
                                .size(11.0)
                                .color(Color32::from_rgb(130, 135, 142)),
                        );
                    });
                });
            });
    }

    fn settings_panel(&mut self, ctx: &egui::Context, audio: &AudioFrame) {
        if !self.settings.show_settings {
            return;
        }

        let mut changed = false;
        let mut hide_settings = false;

        egui::SidePanel::right("settings_panel")
            .resizable(true)
            .default_width(340.0)
            .width_range(290.0..=450.0)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(20, 22, 25))
                    .inner_margin(egui::Margin::same(14.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("Settings").strong().size(14.0));
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Hide settings").clicked() {
                            hide_settings = true;
                        }
                    });
                });
                ui.separator();

                egui::ScrollArea::vertical()
                    .id_salt("settings_panel_scroll")
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        ui.label(RichText::new("Audio").strong().size(14.0));
                        ui.add_space(6.0);
                        status_row(ui, audio);
                        ui.add_space(8.0);
                        changed |= slider(
                            ui,
                            &mut self.settings.sensitivity,
                            0.2..=5.0,
                            "Sensitivity",
                            "How strongly audio moves the visualizer",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.noise_gate,
                            0.0..=0.2,
                            "Noise gate",
                            "Suppresses low-level background noise",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.bass_boost,
                            0.5..=3.0,
                            "Bass boost",
                            "Extra weight for low frequencies",
                        );
                        ui.separator();

                        ui.label(RichText::new("Visualizer").strong().size(14.0));
                        ui.add_space(6.0);
                        egui::ComboBox::from_label("Mode")
                            .selected_text(self.settings.mode.label())
                            .show_ui(ui, |ui| {
                                for mode in VisualizerMode::ALL {
                                    changed |= ui
                                        .selectable_value(&mut self.settings.mode, mode, mode.label())
                                        .changed();
                                }
                            });
                        egui::ComboBox::from_label("Color")
                            .selected_text(self.settings.color_preset.label())
                            .show_ui(ui, |ui| {
                                for preset in ColorPreset::ALL {
                                    changed |= ui
                                        .selectable_value(
                                            &mut self.settings.color_preset,
                                            preset,
                                            preset.label(),
                                        )
                                        .changed();
                                }
                            });
                        ui.add_space(4.0);
                        ui.label(RichText::new("Presets").strong().size(13.0));
                        ui.horizontal_wrapped(|ui| {
                            for preset in VisualizerPreset::ALL {
                                if ui.button(preset.label()).clicked() {
                                    self.apply_visualizer_preset(preset);
                                    changed = true;
                                }
                            }
                        });
                        if self.settings.color_preset == ColorPreset::Custom {
                            ui.horizontal(|ui| {
                                ui.label("Custom color");
                                let mut color = Color32::from_rgb(
                                    self.settings.custom_color[0],
                                    self.settings.custom_color[1],
                                    self.settings.custom_color[2],
                                );
                                if ui.color_edit_button_srgba(&mut color).changed() {
                                    self.settings.custom_color = [color.r(), color.g(), color.b()];
                                    changed = true;
                                }
                            });
                        }
                        changed |= usize_slider(ui, &mut self.settings.bars, 16..=192, "Bands");
                        changed |= slider(
                            ui,
                            &mut self.settings.smoothing,
                            0.0..=0.96,
                            "Smoothing",
                            "Higher values create slower movement",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.falloff,
                            0.0..=0.99,
                            "Falloff",
                            "Controls how quickly bars settle",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.line_width,
                            1.0..=8.0,
                            "Line width",
                            "Used by waveform and radial modes",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.fill_opacity,
                            0.1..=1.0,
                            "Visualizer opacity",
                            "Opacity for bars and shapes",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.glow_strength,
                            0.0..=1.0,
                            "Glow strength",
                            "Soft bloom around bars and lines",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.color_drift,
                            0.0..=1.0,
                            "Color drift",
                            "Slowly shifts gradient intensity with the beat",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.bar_gap,
                            0.0..=8.0,
                            "Bar gap",
                            "Horizontal spacing between bars",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.bar_rounding,
                            0.0..=1.0,
                            "Bar rounding",
                            "Corner roundness of each bar",
                        );
                        changed |= slider(
                            ui,
                            &mut self.settings.background_alpha,
                            0.0..=1.0,
                            "Background opacity",
                            "Use lower values for overlay-style windows",
                        );
                        changed |= ui
                            .checkbox(&mut self.settings.show_grid, "Background grid")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.show_peaks, "Peak markers")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.show_meter, "Bass/mid/treble meter")
                            .changed();
                        ui.separator();

                        ui.label(RichText::new("Desktop Placement").strong().size(14.0));
                        ui.add_space(6.0);
                        if ui
                            .checkbox(&mut self.settings.desktop_widget, "Desktop widget mode")
                            .changed()
                        {
                            if self.settings.desktop_widget {
                                self.settings.visualizer_only_widget = true;
                            }
                            changed = true;
                        }
                        if ui
                            .checkbox(
                                &mut self.settings.desktop_only,
                                "Stay on desktop only (never overlay apps)",
                            )
                            .on_hover_text(
                                "Keeps the visualizer pinned to the desktop behind other windows so it never covers your apps.",
                            )
                            .changed()
                        {
                            if self.settings.desktop_only {
                                self.settings.always_on_top = false;
                            }
                            changed = true;
                        }
                        changed |= ui
                            .checkbox(
                                &mut self.settings.visualizer_only_widget,
                                "Widget visualizer-only (hide all overlays)",
                            )
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.frameless, "Frameless window")
                            .changed();
                        ui.add_enabled_ui(!self.settings.desktop_only, |ui| {
                            changed |= ui
                                .checkbox(&mut self.settings.always_on_top, "Always on top")
                                .on_disabled_hover_text(
                                    "Disabled while \"Stay on desktop only\" is enabled.",
                                )
                                .changed();
                        });
                        changed |= ui
                            .checkbox(&mut self.settings.click_through, "Click-through overlay")
                            .changed();
                        changed |= ui
                            .checkbox(&mut self.settings.show_top_bar, "Show app controls")
                            .changed();
                        ui.horizontal(|ui| {
                            changed |= int_drag(ui, &mut self.settings.desktop_x, "X");
                            changed |= int_drag(ui, &mut self.settings.desktop_y, "Y");
                        });
                        ui.horizontal(|ui| {
                            changed |= int_drag(ui, &mut self.settings.desktop_width, "Width");
                            changed |= int_drag(ui, &mut self.settings.desktop_height, "Height");
                        });
                        let display_area = self.display_area(ctx);
                        ui.label(
                            RichText::new(format!(
                                "Detected work area: {}x{} at {}, {}",
                                display_area.width,
                                display_area.height,
                                display_area.x,
                                display_area.y
                            ))
                            .size(12.0)
                            .color(Color32::from_rgb(170, 174, 178)),
                        );
                        ui.horizontal_wrapped(|ui| {
                            for preset in PlacementPreset::ALL {
                                if ui.button(preset.label()).clicked() {
                                    self.apply_placement_preset(preset, display_area);
                                    changed = true;
                                }
                            }
                        });
                        ui.label(
                            RichText::new("F10 restores controls when the visualizer is running alone.")
                                .size(12.0)
                                .color(Color32::from_rgb(170, 174, 178)),
                        );
                        if self.settings.color_preset == ColorPreset::CoverArt {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new("Cover art colors (coming soon)")
                                    .strong()
                                    .size(14.0),
                            );
                            ui.label(
                                RichText::new("Automatic album artwork colors are disabled while the media artwork path is made crash-safe. The built-in fallback palette is used for now.")
                                    .size(12.0)
                                    .color(Color32::from_rgb(170, 174, 178)),
                            );
                        }
                        ui.separator();

                        ui.label(RichText::new("Taskbar Strip").strong().size(14.0));
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Taskbar visualizer is coming soon.")
                                .size(12.0)
                                .color(Color32::from_rgb(170, 174, 178)),
                        );
                        ui.add_enabled_ui(false, |ui| {
                            ui.checkbox(&mut self.settings.taskbar_strip, "Taskbar strip mode");
                            egui::ComboBox::from_label("Edge")
                                .selected_text(self.settings.taskbar_edge.label())
                                .show_ui(ui, |ui| {
                                    for edge in TaskbarEdge::ALL {
                                        ui.selectable_value(
                                            &mut self.settings.taskbar_edge,
                                            edge,
                                            edge.label(),
                                        );
                                    }
                                });
                            slider(
                                ui,
                                &mut self.settings.strip_thickness,
                                48.0..=260.0,
                                "Strip thickness",
                                "Height or width of the strip",
                            );
                        });
                        changed |= ui
                            .checkbox(&mut self.settings.compact_controls, "Compact top bar")
                            .changed();
                        changed |= u32_slider(
                            ui,
                            &mut self.settings.target_fps,
                            30..=144,
                            "Target FPS",
                        );
                        if ui.button("About").clicked() {
                            self.show_about = true;
                        }

                        if let Some(note) = window_control::platform_note() {
                            ui.add_space(8.0);
                            ui.label(
                                RichText::new(note)
                                    .size(12.0)
                                    .color(Color32::from_rgb(170, 174, 178)),
                            );
                        }
                    });
            });

        if hide_settings {
            self.settings.show_settings = false;
            changed = true;
        }

        if changed {
            self.mark_changed();
        }
    }

    fn settings_top_bar(&mut self, ctx: &egui::Context) {
        egui::TopBottomPanel::top("settings_window_top_bar")
            .exact_height(44.0)
            .frame(
                egui::Frame::none()
                    .fill(Color32::from_rgb(18, 20, 23))
                    .inner_margin(egui::Margin::symmetric(14.0, 7.0)),
            )
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.heading(
                        RichText::new("Chosen Visualizer Settings")
                            .size(15.0)
                            .color(Color32::from_rgb(232, 233, 234)),
                    );
                    ui.separator();
                    if ui.button("About").clicked() {
                        self.show_about = true;
                    }
                    if ui.button("Hide settings").clicked() {
                        self.settings.show_settings = false;
                        self.mark_changed();
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.label(
                            RichText::new(settings_path().display().to_string())
                                .size(11.0)
                                .color(Color32::from_rgb(130, 135, 142)),
                        );
                    });
                });
            });
    }

    fn display_area(&self, ctx: &egui::Context) -> DisplayArea {
        if let Some(area) = window_control::current_display_area(self.native_window) {
            return area;
        }

        ctx.input(|input| {
            let viewport = input.viewport();
            if let Some(rect) = viewport.inner_rect {
                DisplayArea {
                    x: rect.min.x.round() as i32,
                    y: rect.min.y.round() as i32,
                    width: rect.width().round() as i32,
                    height: rect.height().round() as i32,
                }
            } else if let Some(size) = viewport.monitor_size {
                DisplayArea {
                    x: 0,
                    y: 0,
                    width: size.x.round() as i32,
                    height: size.y.round() as i32,
                }
            } else {
                DisplayArea {
                    x: 0,
                    y: 0,
                    width: 1920,
                    height: 1080,
                }
            }
        })
    }

    fn apply_placement_preset(&mut self, preset: PlacementPreset, area: DisplayArea) {
        let (width_ratio, height_ratio) = match preset.size {
            PlacementSize::Compact => (0.30, 0.16),
            PlacementSize::Wide => (0.46, 0.18),
            PlacementSize::Band => (0.72, 0.14),
        };
        let width = ((area.width as f32 * width_ratio).round() as i32).clamp(280, 1600);
        let height = ((area.height as f32 * height_ratio).round() as i32).clamp(96, 420);
        let margin = ((area.width.min(area.height) as f32 * 0.035).round() as i32).clamp(18, 72);
        let center_x = area.x + (area.width - width) / 2;
        let center_y = area.y + (area.height - height) / 2;
        let right = area.x + area.width - width - margin;
        let bottom = area.y + area.height - height - margin;

        let (x, y) = match preset.anchor {
            PlacementAnchor::TopLeft => (area.x + margin, area.y + margin),
            PlacementAnchor::Top => (center_x, area.y + margin),
            PlacementAnchor::TopRight => (right, area.y + margin),
            PlacementAnchor::Left => (area.x + margin, center_y),
            PlacementAnchor::Center => (center_x, center_y),
            PlacementAnchor::Right => (right, center_y),
            PlacementAnchor::BottomLeft => (area.x + margin, bottom),
            PlacementAnchor::Bottom => (center_x, bottom),
            PlacementAnchor::BottomRight => (right, bottom),
            PlacementAnchor::LowerThird => (
                center_x,
                area.y + ((area.height as f32 * 0.66).round() as i32) - height / 2,
            ),
            PlacementAnchor::TopBand => (area.x + margin, area.y + margin),
            PlacementAnchor::BottomBand => (area.x + margin, bottom),
        };

        self.settings.desktop_widget = true;
        self.settings.visualizer_only_widget = true;
        self.settings.frameless = true;
        self.settings.background_alpha = 0.0;
        self.settings.show_grid = false;
        self.settings.desktop_width = width;
        self.settings.desktop_height = height;
        self.settings.desktop_x = x;
        self.settings.desktop_y = y;
    }

    fn apply_visualizer_preset(&mut self, preset: VisualizerPreset) {
        match preset {
            VisualizerPreset::CleanBars => {
                self.settings.mode = VisualizerMode::Bars;
                self.settings.color_preset = ColorPreset::Steel;
                self.settings.bars = 96;
                self.settings.smoothing = 0.68;
                self.settings.falloff = 0.88;
                self.settings.line_width = 2.0;
                self.settings.fill_opacity = 0.86;
                self.settings.glow_strength = 0.12;
                self.settings.color_drift = 0.05;
                self.settings.bar_gap = 2.0;
                self.settings.bar_rounding = 0.35;
                self.settings.show_peaks = false;
            }
            VisualizerPreset::MirrorGlow => {
                self.settings.mode = VisualizerMode::MirrorBars;
                self.settings.color_preset = ColorPreset::Ember;
                self.settings.bars = 72;
                self.settings.smoothing = 0.74;
                self.settings.falloff = 0.92;
                self.settings.line_width = 2.0;
                self.settings.fill_opacity = 0.78;
                self.settings.glow_strength = 0.45;
                self.settings.color_drift = 0.18;
                self.settings.bar_gap = 3.0;
                self.settings.bar_rounding = 0.60;
                self.settings.show_peaks = true;
            }
            VisualizerPreset::WaveRibbon => {
                self.settings.mode = VisualizerMode::Waveform;
                self.settings.color_preset = ColorPreset::Forest;
                self.settings.bars = 128;
                self.settings.smoothing = 0.82;
                self.settings.falloff = 0.95;
                self.settings.line_width = 3.0;
                self.settings.fill_opacity = 0.76;
                self.settings.glow_strength = 0.22;
                self.settings.color_drift = 0.10;
                self.settings.bar_gap = 1.0;
                self.settings.bar_rounding = 0.25;
                self.settings.show_peaks = false;
            }
            VisualizerPreset::RadialPulse => {
                self.settings.mode = VisualizerMode::Radial;
                self.settings.color_preset = ColorPreset::Slate;
                self.settings.bars = 88;
                self.settings.smoothing = 0.70;
                self.settings.falloff = 0.90;
                self.settings.line_width = 2.5;
                self.settings.fill_opacity = 0.82;
                self.settings.glow_strength = 0.30;
                self.settings.color_drift = 0.20;
                self.settings.bar_gap = 2.0;
                self.settings.bar_rounding = 0.50;
                self.settings.show_peaks = false;
            }
            VisualizerPreset::ParticleField => {
                self.settings.mode = VisualizerMode::Particles;
                self.settings.color_preset = ColorPreset::Graphite;
                self.settings.bars = 64;
                self.settings.smoothing = 0.62;
                self.settings.falloff = 0.86;
                self.settings.line_width = 2.0;
                self.settings.fill_opacity = 0.70;
                self.settings.glow_strength = 0.36;
                self.settings.color_drift = 0.28;
                self.settings.bar_gap = 2.0;
                self.settings.bar_rounding = 0.40;
                self.settings.show_peaks = false;
            }
        }
    }

    fn show_about_popup(&mut self, ctx: &egui::Context) {
        if !self.show_about {
            return;
        }

        let mut open = self.show_about;
        egui::Window::new("About Chosen Visualizer")
            .open(&mut open)
            .collapsible(false)
            .resizable(false)
            .default_width(560.0)
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.heading(RichText::new(self.title).size(20.0));
                            ui.label(
                                RichText::new("Audio-reactive desktop visualizer")
                                    .color(Color32::from_rgb(170, 174, 178)),
                            );
                        });
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            ui.label(
                                RichText::new("v1.0.0")
                                    .strong()
                                    .color(Color32::from_rgb(202, 207, 213)),
                            );
                        });
                    });
                    ui.add_space(6.0);
                    ui.separator();

                    egui::Grid::new("about_summary_grid")
                        .num_columns(2)
                        .spacing(Vec2::new(18.0, 8.0))
                        .striped(true)
                        .show(ui, |ui| {
                            about_key(ui, "Status");
                            ui.label("Early Access build, local settings saved automatically");
                            ui.end_row();

                            about_key(ui, "Info");
                            ui.label("This build is early access and may contain bugs. Please report issues on GitHub, and does not have auto update. Settings are saved automatically in the app data folder.");
                            ui.end_row();

                            about_key(ui, "Settings");
                            ui.label(
                                RichText::new(settings_path().display().to_string())
                                    .size(12.0)
                                    .color(Color32::from_rgb(170, 174, 178)),
                            );
                            ui.end_row();

                            about_key(ui, "Restore");
                            ui.label("Press F10 or use the tray Open settings action");
                            ui.end_row();
                        });

                    ui.separator();

                    ui.label(RichText::new("Current focus").strong().size(14.0));
                    ui.columns(2, |columns| {
                        about_list(
                            &mut columns[0],
                            &[
                                "Live loopback audio visualization",
                                "Screen-aware widget placement",
                                "Persistent settings and tray controls",
                            ],
                        );
                        about_list(
                            &mut columns[1],
                            &[
                                "Visualizer-only widget mode",
                                "Scrollable settings panel",
                                "Theme and placement presets",
                            ],
                        );
                    });

                    ui.separator();

                    ui.label(RichText::new("Roadmap").strong().size(14.0));
                    ui.columns(2, |columns| {
                        columns[0].label(RichText::new("Near term").strong());
                        about_list(
                            &mut columns[0],
                            &[
                                "Safer media artwork color extraction",
                                "Theme preset browser",
                                "Import and export profiles",
                                "Hotkeys for common tray actions",
                                "Per-monitor DPI polish",
                            ],
                        );

                        columns[1].label(RichText::new("Later").strong());
                        about_list(
                            &mut columns[1],
                            &[
                                "Taskbar strip mode",
                                "Multi-monitor placement profiles",
                                "Performance and CPU controls",
                                "Audio source picker",
                                "Optional startup integration",
                            ],
                        );
                    });

                    ui.separator();
                    ui.label(
                        RichText::new(
                            "Cover art colors are marked coming soon until the album-art pipeline is stable.",
                        )
                        .size(12.0)
                        .color(Color32::from_rgb(170, 174, 178)),
                    );
                    ui.add_space(10.0);
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Close").clicked() {
                            self.show_about = false;
                        }
                    });
                });
            });
        self.show_about = open && self.show_about;
    }

    fn settings_window(&mut self, parent_ctx: &egui::Context, audio: &AudioFrame, dt: f32) {
        if !self.settings.show_settings {
            return;
        }

        let viewport_id = egui::ViewportId::from_hash_of("settings_window");
        let builder = egui::ViewportBuilder::default()
            .with_title("Chosen Visualizer Settings")
            .with_inner_size([1140.0, 760.0])
            .with_min_inner_size([760.0, 460.0]);

        parent_ctx.show_viewport_immediate(viewport_id, builder, |ctx, _class| {
            if ctx.input(|i| i.viewport().close_requested()) {
                self.settings.show_settings = false;
                self.mark_changed();
            }

            self.settings_top_bar(ctx);
            self.settings_panel(ctx, audio);

            egui::CentralPanel::default()
                .frame(egui::Frame::none().fill(Color32::from_rgb(14, 15, 17)))
                .show(ctx, |ui| {
                    let rect = ui.max_rect();
                    self.settings_visualizer
                        .paint(ui, rect, audio, &self.settings, dt);
                });
            self.show_about_popup(ctx);
        });
    }
}

#[derive(Clone, Copy)]
struct PlacementPreset {
    label: &'static str,
    anchor: PlacementAnchor,
    size: PlacementSize,
}

#[derive(Clone, Copy)]
enum VisualizerPreset {
    CleanBars,
    MirrorGlow,
    WaveRibbon,
    RadialPulse,
    ParticleField,
}

impl VisualizerPreset {
    const ALL: [Self; 5] = [
        Self::CleanBars,
        Self::MirrorGlow,
        Self::WaveRibbon,
        Self::RadialPulse,
        Self::ParticleField,
    ];

    fn label(self) -> &'static str {
        match self {
            Self::CleanBars => "Clean bars",
            Self::MirrorGlow => "Mirror glow",
            Self::WaveRibbon => "Wave ribbon",
            Self::RadialPulse => "Radial pulse",
            Self::ParticleField => "Particle field",
        }
    }
}

impl PlacementPreset {
    const ALL: [Self; 12] = [
        Self::new("Top left", PlacementAnchor::TopLeft, PlacementSize::Compact),
        Self::new("Top", PlacementAnchor::Top, PlacementSize::Wide),
        Self::new(
            "Top right",
            PlacementAnchor::TopRight,
            PlacementSize::Compact,
        ),
        Self::new("Left", PlacementAnchor::Left, PlacementSize::Compact),
        Self::new("Center", PlacementAnchor::Center, PlacementSize::Wide),
        Self::new("Right", PlacementAnchor::Right, PlacementSize::Compact),
        Self::new(
            "Lower third",
            PlacementAnchor::LowerThird,
            PlacementSize::Wide,
        ),
        Self::new(
            "Bottom left",
            PlacementAnchor::BottomLeft,
            PlacementSize::Compact,
        ),
        Self::new("Bottom", PlacementAnchor::Bottom, PlacementSize::Wide),
        Self::new(
            "Bottom right",
            PlacementAnchor::BottomRight,
            PlacementSize::Compact,
        ),
        Self::new("Top band", PlacementAnchor::TopBand, PlacementSize::Band),
        Self::new(
            "Bottom band",
            PlacementAnchor::BottomBand,
            PlacementSize::Band,
        ),
    ];

    const fn new(label: &'static str, anchor: PlacementAnchor, size: PlacementSize) -> Self {
        Self {
            label,
            anchor,
            size,
        }
    }

    fn label(self) -> &'static str {
        self.label
    }
}

#[derive(Clone, Copy)]
enum PlacementAnchor {
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
    LowerThird,
    TopBand,
    BottomBand,
}

#[derive(Clone, Copy)]
enum PlacementSize {
    Compact,
    Wide,
    Band,
}

impl eframe::App for ChosenVisualizerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.handle_tray(ctx);
        self.handle_shortcuts(ctx);

        let now = Instant::now();
        let dt = (now - self.last_frame_at).as_secs_f32().clamp(0.001, 0.1);
        self.last_frame_at = now;

        self.top_bar(ctx);
        let audio = self.audio.frame(
            self.settings.bars,
            self.settings.sensitivity,
            self.settings.noise_gate,
            self.settings.bass_boost,
        );
        self.settings_window(ctx, &audio, dt);

        let panel_fill = if self.settings.desktop_widget {
            Color32::TRANSPARENT
        } else {
            Color32::from_rgb(14, 15, 17)
        };
        egui::CentralPanel::default()
            .frame(egui::Frame::none().fill(panel_fill))
            .show(ctx, |ui| {
                let rect = ui.max_rect();
                self.visualizer.paint(ui, rect, &audio, &self.settings, dt);
            });

        self.apply_window_changes(ctx);
        self.maybe_save();

        let fps = self.settings.target_fps.max(1);
        ctx.request_repaint_after(Duration::from_secs_f32(1.0 / fps as f32));
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.settings.save();
    }

    fn clear_color(&self, _visuals: &egui::Visuals) -> [f32; 4] {
        if self.settings.desktop_widget {
            [0.0, 0.0, 0.0, 0.0]
        } else {
            [14.0 / 255.0, 15.0 / 255.0, 17.0 / 255.0, 1.0]
        }
    }
}

#[cfg(windows)]
fn native_window_handle(cc: &eframe::CreationContext<'_>) -> Option<NativeWindowHandle> {
    let handle = cc.window_handle().ok()?.as_raw();
    match handle {
        RawWindowHandle::Win32(handle) => Some(handle.hwnd.get() as NativeWindowHandle),
        _ => None,
    }
}

#[cfg(not(windows))]
fn native_window_handle(_cc: &eframe::CreationContext<'_>) -> Option<NativeWindowHandle> {
    None
}

fn install_style(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals = egui::Visuals::dark();
    style.visuals.window_fill = Color32::from_rgb(20, 22, 25);
    style.visuals.panel_fill = Color32::from_rgb(18, 20, 23);
    style.visuals.faint_bg_color = Color32::from_rgb(29, 32, 36);
    style.visuals.extreme_bg_color = Color32::from_rgb(10, 11, 13);
    style.visuals.widgets.inactive.bg_fill = Color32::from_rgb(34, 37, 42);
    style.visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 52, 58);
    style.visuals.widgets.active.bg_fill = Color32::from_rgb(62, 67, 75);
    style.visuals.widgets.inactive.fg_stroke = Stroke::new(1.0, Color32::from_rgb(222, 224, 226));
    style.visuals.selection.bg_fill = Color32::from_rgb(78, 92, 110);
    style.visuals.selection.stroke = Stroke::new(1.0, Color32::from_rgb(235, 236, 237));
    style.spacing.item_spacing = Vec2::new(8.0, 8.0);
    style.spacing.button_padding = Vec2::new(10.0, 5.0);
    style.text_styles.insert(
        egui::TextStyle::Heading,
        FontId::new(18.0, FontFamily::Proportional),
    );
    style.text_styles.insert(
        egui::TextStyle::Body,
        FontId::new(13.5, FontFamily::Proportional),
    );
    ctx.set_style(style);
}

fn status_row(ui: &mut egui::Ui, audio: &AudioFrame) {
    let color = if audio.active {
        Color32::from_rgb(153, 190, 160)
    } else {
        Color32::from_rgb(207, 166, 132)
    };
    ui.horizontal_wrapped(|ui| {
        ui.label(
            RichText::new(if audio.active { "Live" } else { "Fallback" })
                .color(color)
                .strong(),
        );
        ui.label(
            RichText::new(audio.source_label.as_str()).color(Color32::from_rgb(164, 169, 176)),
        );
    });
    if let Some(error) = &audio.error {
        ui.label(
            RichText::new(error)
                .size(12.0)
                .color(Color32::from_rgb(207, 166, 132)),
        );
    }
}

fn about_key(ui: &mut egui::Ui, label: &str) {
    ui.label(
        RichText::new(label)
            .strong()
            .color(Color32::from_rgb(202, 207, 213)),
    );
}

fn about_list(ui: &mut egui::Ui, items: &[&str]) {
    for item in items {
        ui.label(format!("- {item}"));
    }
}

fn slider(
    ui: &mut egui::Ui,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    label: &str,
    tooltip: &str,
) -> bool {
    let response = ui.add(egui::Slider::new(value, range).text(label));
    if !tooltip.is_empty() {
        response.clone().on_hover_text(tooltip);
    }
    response.changed()
}

fn int_drag(ui: &mut egui::Ui, value: &mut i32, label: &str) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .speed(4)
            .prefix(format!("{label}: ")),
    )
    .changed()
}

fn usize_slider(
    ui: &mut egui::Ui,
    value: &mut usize,
    range: std::ops::RangeInclusive<usize>,
    label: &str,
) -> bool {
    ui.add(egui::Slider::new(value, range).text(label))
        .changed()
}

fn u32_slider(
    ui: &mut egui::Ui,
    value: &mut u32,
    range: std::ops::RangeInclusive<u32>,
    label: &str,
) -> bool {
    ui.add(egui::Slider::new(value, range).text(label))
        .changed()
}
