use crate::{
    audio::AudioFrame,
    settings::{ColorPreset, Settings, VisualizerMode},
};
use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Rect, Rounding, Shape, Stroke, pos2, vec2,
};
use std::{f32::consts::TAU, fs, time::Instant};

#[derive(Default)]
pub struct VisualizerState {
    smooth: Vec<f32>,
    peaks: Vec<f32>,
    particle_phase: f32,
    color_phase: f32,
    cached_cover_source: String,
    cached_cover_palette: Option<(Color32, Color32)>,
    last_cover_attempt: Option<Instant>,
}

impl VisualizerState {
    pub fn paint(
        &mut self,
        ui: &mut egui::Ui,
        rect: Rect,
        audio: &AudioFrame,
        settings: &Settings,
        dt: f32,
    ) {
        let painter = ui.painter_at(rect);
        paint_background(&painter, rect, settings);

        if self.smooth.len() != audio.spectrum.len() {
            self.smooth = vec![0.0; audio.spectrum.len()];
            self.peaks = vec![0.0; audio.spectrum.len()];
        }

        for (i, target) in audio.spectrum.iter().enumerate() {
            let current = self.smooth[i];
            let attack = 1.0 - settings.smoothing;
            let next = if *target > current {
                current + (*target - current) * attack.max(0.08)
            } else {
                (current * settings.falloff).max(*target * (1.0 - settings.smoothing * 0.6))
            };
            self.smooth[i] = next.clamp(0.0, 1.0);
            self.peaks[i] = self.peaks[i].max(self.smooth[i]);
            self.peaks[i] = (self.peaks[i] - dt * 0.34).max(self.smooth[i]);
        }

        self.color_phase = (self.color_phase + dt * (0.25 + audio.volume * 1.5)) % 1.0;

        let (mut primary, mut secondary) = settings.accent_colors();
        if settings.color_preset == ColorPreset::CoverArt {
            if let Some((p, s)) = self.cover_palette(settings) {
                primary = p;
                secondary = s;
            }
        }

        if settings.color_drift > 0.0 {
            let drift = (self.color_phase * TAU).sin() * 0.5 + 0.5;
            let drift = drift * settings.color_drift;
            let p = mix(primary, secondary, drift * 0.7);
            let s = mix(secondary, primary, drift * 0.4);
            primary = p;
            secondary = s;
        }
        match settings.mode {
            VisualizerMode::Bars => self.paint_bars(&painter, rect, primary, secondary, settings),
            VisualizerMode::MirrorBars => {
                self.paint_mirror_bars(&painter, rect, primary, secondary, settings)
            }
            VisualizerMode::Waveform => {
                self.paint_waveform(&painter, rect, audio, primary, secondary, settings)
            }
            VisualizerMode::Radial => {
                self.paint_radial(&painter, rect, primary, secondary, settings)
            }
            VisualizerMode::Particles => {
                self.paint_particles(&painter, rect, audio, primary, secondary, settings, dt)
            }
        }

        let hide_overlays = settings.desktop_widget && settings.visualizer_only_widget;
        if settings.show_meter && !hide_overlays {
            paint_meters(&painter, rect, audio, primary, secondary);
        }

        if !audio.active && !hide_overlays {
            paint_status(&painter, rect, audio);
        }
    }

    fn cover_palette(&mut self, settings: &Settings) -> Option<(Color32, Color32)> {
        let source = settings.cover_art_source.trim();
        if source.is_empty() {
            self.cached_cover_source.clear();
            self.cached_cover_palette = None;
            self.last_cover_attempt = None;
            return None;
        }

        if self.cached_cover_source != source {
            self.cached_cover_source = source.to_owned();
            self.cached_cover_palette = None;
            self.last_cover_attempt = None;
        }

        if self.cached_cover_palette.is_none() {
            let can_retry = self
                .last_cover_attempt
                .map(|t| t.elapsed().as_secs_f32() >= 10.0)
                .unwrap_or(true);
            if can_retry {
                self.cached_cover_palette = load_cover_art_palette(source);
                self.last_cover_attempt = Some(Instant::now());
            }
        }

        self.cached_cover_palette
    }

    fn paint_bars(
        &self,
        painter: &Painter,
        rect: Rect,
        primary: Color32,
        secondary: Color32,
        settings: &Settings,
    ) {
        let values = &self.smooth;
        if values.is_empty() {
            return;
        }
        let gap = settings
            .bar_gap
            .min(rect.width() / values.len() as f32 * 0.35)
            .max(0.0);
        let width = ((rect.width() - gap * (values.len().saturating_sub(1)) as f32)
            / values.len() as f32)
            .max(1.5);
        let base = rect.bottom() - 18.0;
        let max_h = (rect.height() - 42.0).max(20.0);
        let rounding = (width * settings.bar_rounding).min(7.0);

        for (i, value) in values.iter().enumerate() {
            let x = rect.left() + i as f32 * (width + gap);
            let shaped = value.powf(0.82);
            let h = (shaped * max_h).max(1.0);
            let r = Rect::from_min_max(pos2(x, base - h), pos2(x + width, base));
            let color = mix(secondary, primary, i as f32 / values.len() as f32)
                .linear_multiply(settings.fill_opacity);
            if settings.glow_strength > 0.0 {
                painter.rect_filled(
                    r.expand(1.0 + settings.glow_strength * 5.0),
                    Rounding::same((rounding + 2.0).min(9.0)),
                    color.linear_multiply(0.12 * settings.glow_strength),
                );
            }
            painter.rect_filled(r, Rounding::same(rounding), color);

            if settings.show_peaks && i < self.peaks.len() {
                let peak_y = base - self.peaks[i].powf(0.82) * max_h;
                painter.line_segment(
                    [pos2(x, peak_y), pos2(x + width, peak_y)],
                    Stroke::new(
                        1.0,
                        Color32::from_rgba_premultiplied(
                            primary.r(),
                            primary.g(),
                            primary.b(),
                            180,
                        ),
                    ),
                );
            }
        }
    }

    fn paint_mirror_bars(
        &self,
        painter: &Painter,
        rect: Rect,
        primary: Color32,
        secondary: Color32,
        settings: &Settings,
    ) {
        let values = &self.smooth;
        if values.is_empty() {
            return;
        }
        let center = rect.center().y;
        let gap = settings
            .bar_gap
            .min(rect.width() / values.len() as f32 * 0.34)
            .max(0.0);
        let width = ((rect.width() - gap * (values.len().saturating_sub(1)) as f32)
            / values.len() as f32)
            .max(1.2);
        let max_h = (rect.height() * 0.42).max(18.0);
        let rounding = (width * settings.bar_rounding).min(7.0);

        for (i, value) in values.iter().enumerate() {
            let x = rect.left() + i as f32 * (width + gap);
            let h = value.powf(0.78) * max_h + 1.0;
            let color = mix(primary, secondary, i as f32 / values.len() as f32)
                .linear_multiply(settings.fill_opacity);
            let top = Rect::from_min_max(pos2(x, center - h), pos2(x + width, center - 1.5));
            let bottom = Rect::from_min_max(pos2(x, center + 1.5), pos2(x + width, center + h));
            if settings.glow_strength > 0.0 {
                painter.rect_filled(
                    top.expand(0.8 + settings.glow_strength * 4.0),
                    Rounding::same((rounding + 2.0).min(9.0)),
                    color.linear_multiply(0.1 * settings.glow_strength),
                );
                painter.rect_filled(
                    bottom.expand(0.8 + settings.glow_strength * 4.0),
                    Rounding::same((rounding + 2.0).min(9.0)),
                    color.linear_multiply(0.08 * settings.glow_strength),
                );
            }
            painter.rect_filled(top, Rounding::same(rounding), color);
            painter.rect_filled(
                bottom,
                Rounding::same(rounding),
                color.linear_multiply(0.72),
            );
        }

        painter.line_segment(
            [pos2(rect.left(), center), pos2(rect.right(), center)],
            Stroke::new(1.0, Color32::from_rgba_premultiplied(220, 220, 216, 38)),
        );
    }

    fn paint_waveform(
        &self,
        painter: &Painter,
        rect: Rect,
        audio: &AudioFrame,
        primary: Color32,
        secondary: Color32,
        settings: &Settings,
    ) {
        if audio.waveform.len() < 2 {
            return;
        }
        let center = rect.center().y;
        let amp = rect.height() * 0.38 * settings.sensitivity.min(2.2);
        let mut points = Vec::with_capacity(audio.waveform.len());
        for (i, sample) in audio.waveform.iter().enumerate() {
            let x = egui::lerp(
                rect.left()..=rect.right(),
                i as f32 / (audio.waveform.len() - 1) as f32,
            );
            let y = center - sample * amp;
            points.push(pos2(x, y));
        }

        let mut fill = points.clone();
        fill.push(pos2(rect.right(), center));
        fill.push(pos2(rect.left(), center));
        painter.add(Shape::convex_polygon(
            fill,
            Color32::from_rgba_premultiplied(
                secondary.r(),
                secondary.g(),
                secondary.b(),
                (70.0 * settings.fill_opacity) as u8,
            ),
            Stroke::NONE,
        ));
        painter.add(Shape::line(
            points,
            Stroke::new(settings.line_width, primary),
        ));
        if settings.glow_strength > 0.0 {
            painter.add(Shape::line(
                audio
                    .waveform
                    .iter()
                    .enumerate()
                    .map(|(i, sample)| {
                        let x = egui::lerp(
                            rect.left()..=rect.right(),
                            i as f32 / (audio.waveform.len() - 1) as f32,
                        );
                        let y = center - sample * amp;
                        pos2(x, y)
                    })
                    .collect(),
                Stroke::new(
                    settings.line_width + 4.0 * settings.glow_strength,
                    primary.linear_multiply(0.2 * settings.glow_strength),
                ),
            ));
        }

        let mid_color = Color32::from_rgba_premultiplied(230, 230, 226, 36);
        painter.line_segment(
            [pos2(rect.left(), center), pos2(rect.right(), center)],
            Stroke::new(1.0, mid_color),
        );
    }

    fn paint_radial(
        &self,
        painter: &Painter,
        rect: Rect,
        primary: Color32,
        secondary: Color32,
        settings: &Settings,
    ) {
        let values = &self.smooth;
        if values.is_empty() {
            return;
        }
        let center = rect.center();
        let radius = rect.width().min(rect.height()) * 0.22;
        let max_extra = rect.width().min(rect.height()) * 0.25;
        let mut inner = Vec::with_capacity(values.len());
        let mut outer = Vec::with_capacity(values.len());

        for (i, value) in values.iter().enumerate() {
            let angle = -TAU * 0.25 + TAU * i as f32 / values.len() as f32;
            let dir = vec2(angle.cos(), angle.sin());
            inner.push(center + dir * radius);
            outer.push(center + dir * (radius + value.powf(0.78) * max_extra));
        }

        for i in 0..values.len() {
            let next = (i + 1) % values.len();
            let t = i as f32 / values.len() as f32;
            let color = mix(primary, secondary, t).linear_multiply(settings.fill_opacity);
            painter.line_segment(
                [inner[i], outer[i]],
                Stroke::new(settings.line_width, color),
            );
            if values[i] > 0.15 {
                painter.line_segment(
                    [outer[i], outer[next]],
                    Stroke::new(1.0, color.linear_multiply(0.44)),
                );
            }
        }

        painter.circle_stroke(
            center,
            radius,
            Stroke::new(1.0, Color32::from_rgba_premultiplied(220, 220, 216, 50)),
        );
        painter.circle_filled(
            center,
            radius * 0.08 + self.smooth.iter().copied().fold(0.0, f32::max) * 7.0,
            primary.linear_multiply(0.65),
        );
    }

    fn paint_particles(
        &mut self,
        painter: &Painter,
        rect: Rect,
        audio: &AudioFrame,
        primary: Color32,
        secondary: Color32,
        settings: &Settings,
        dt: f32,
    ) {
        self.particle_phase = (self.particle_phase + dt * (0.24 + audio.volume * 0.8)) % 1.0;
        let count = self.smooth.len().min(128);
        if count == 0 {
            return;
        }
        let center = rect.center();
        let span = rect.width().min(rect.height());
        for i in 0..count {
            let value = self.smooth[i];
            let lane = i as f32 / count as f32;
            let angle =
                TAU * ((lane * 0.618_034 + self.particle_phase * (0.12 + value * 0.15)) % 1.0);
            let radius = span * (0.08 + lane * 0.36 + value * 0.18);
            let wobble = (self.particle_phase * TAU + i as f32 * 0.37).sin() * span * 0.015;
            let pos = center + vec2(angle.cos(), angle.sin()) * (radius + wobble);
            let size = 2.0 + value.powf(0.6) * 9.0;
            let color = mix(primary, secondary, lane)
                .linear_multiply((0.35 + value * settings.fill_opacity).min(1.0));
            painter.circle_filled(pos, size, color);
        }

        let line_y = rect.bottom() - 28.0;
        let width = rect.width() * audio.volume.clamp(0.0, 1.0);
        painter.rect_filled(
            Rect::from_min_size(pos2(rect.left(), line_y), vec2(width, 3.0)),
            Rounding::same(1.5),
            primary.linear_multiply(0.72),
        );
    }
}

fn paint_background(painter: &Painter, rect: Rect, settings: &Settings) {
    if settings.desktop_widget {
        return;
    }

    let alpha = (255.0 * settings.background_alpha)
        .round()
        .clamp(0.0, 255.0) as u8;
    if alpha > 0 {
        painter.rect_filled(
            rect,
            Rounding::ZERO,
            Color32::from_rgba_premultiplied(14, 15, 17, alpha),
        );
    }

    if !settings.show_grid {
        return;
    }

    let grid =
        Color32::from_rgba_premultiplied(235, 235, 230, (16.0 * settings.background_alpha) as u8);
    let step = 64.0;
    let mut x = rect.left() + step - rect.left() % step;
    while x < rect.right() {
        painter.line_segment(
            [pos2(x, rect.top()), pos2(x, rect.bottom())],
            Stroke::new(1.0, grid),
        );
        x += step;
    }
    let mut y = rect.top() + step - rect.top() % step;
    while y < rect.bottom() {
        painter.line_segment(
            [pos2(rect.left(), y), pos2(rect.right(), y)],
            Stroke::new(1.0, grid),
        );
        y += step;
    }
}

fn paint_meters(
    painter: &Painter,
    rect: Rect,
    audio: &AudioFrame,
    primary: Color32,
    secondary: Color32,
) {
    let meter_rect = Rect::from_min_size(rect.left_top() + vec2(18.0, 18.0), vec2(190.0, 78.0));
    painter.rect_filled(
        meter_rect,
        Rounding::same(6.0),
        Color32::from_rgba_premultiplied(22, 24, 27, 210),
    );
    painter.rect_stroke(
        meter_rect,
        Rounding::same(6.0),
        Stroke::new(1.0, Color32::from_rgba_premultiplied(230, 230, 226, 30)),
    );

    let rows = [("B", audio.bass), ("M", audio.mids), ("T", audio.treble)];
    for (i, (label, value)) in rows.iter().enumerate() {
        let y = meter_rect.top() + 15.0 + i as f32 * 19.0;
        painter.text(
            pos2(meter_rect.left() + 12.0, y),
            Align2::LEFT_CENTER,
            *label,
            FontId::monospace(12.0),
            Color32::from_rgb(180, 184, 188),
        );
        let track = Rect::from_min_size(pos2(meter_rect.left() + 34.0, y - 4.0), vec2(132.0, 8.0));
        painter.rect_filled(track, Rounding::same(3.0), Color32::from_rgb(35, 38, 42));
        let fill = Rect::from_min_size(
            track.min,
            vec2(track.width() * value.clamp(0.0, 1.0), track.height()),
        );
        painter.rect_filled(fill, Rounding::same(3.0), mix(secondary, primary, *value));
    }
}

fn paint_status(painter: &Painter, rect: Rect, audio: &AudioFrame) {
    let text = audio.error.as_deref().unwrap_or("Waiting for audio");
    let box_rect = Rect::from_center_size(rect.center(), vec2(rect.width().min(430.0), 78.0));
    painter.rect_filled(
        box_rect,
        Rounding::same(7.0),
        Color32::from_rgba_premultiplied(24, 26, 29, 232),
    );
    painter.rect_stroke(
        box_rect,
        Rounding::same(7.0),
        Stroke::new(1.0, Color32::from_rgba_premultiplied(230, 230, 226, 35)),
    );
    painter.text(
        box_rect.center_top() + vec2(0.0, 21.0),
        Align2::CENTER_CENTER,
        text,
        FontId::proportional(15.0),
        Color32::from_rgb(220, 222, 224),
    );
    painter.text(
        box_rect.center_bottom() - vec2(0.0, 22.0),
        Align2::CENTER_CENTER,
        audio.source_label.as_str(),
        FontId::proportional(12.0),
        Color32::from_rgb(145, 150, 156),
    );
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    let inv = 1.0 - t;
    Color32::from_rgba_premultiplied(
        (a.r() as f32 * inv + b.r() as f32 * t).round() as u8,
        (a.g() as f32 * inv + b.g() as f32 * t).round() as u8,
        (a.b() as f32 * inv + b.b() as f32 * t).round() as u8,
        (a.a() as f32 * inv + b.a() as f32 * t).round() as u8,
    )
}

fn load_cover_art_palette(source: &str) -> Option<(Color32, Color32)> {
    let bytes = if source.starts_with("http://") || source.starts_with("https://") {
        let response = reqwest::blocking::get(source).ok()?;
        let status = response.status();
        if !status.is_success() {
            return None;
        }
        response.bytes().ok()?.to_vec()
    } else {
        fs::read(source).ok()?
    };

    load_cover_art_palette_from_bytes(&bytes)
}

fn load_cover_art_palette_from_bytes(bytes: &[u8]) -> Option<(Color32, Color32)> {
    let image = image::load_from_memory(bytes).ok()?.to_rgb8();
    let pixels = image.as_raw();
    if pixels.len() < 3 {
        return None;
    }

    let mut vibrant_sum = [0.0_f32; 3];
    let mut deep_sum = [0.0_f32; 3];
    let mut vibrant_weight = 0.0_f32;
    let mut deep_weight = 0.0_f32;

    let count = (pixels.len() / 3).max(1);
    let step = (count / 14_000).max(1);
    for i in (0..count).step_by(step) {
        let idx = i * 3;
        let r = pixels[idx] as f32 / 255.0;
        let g = pixels[idx + 1] as f32 / 255.0;
        let b = pixels[idx + 2] as f32 / 255.0;

        let max = r.max(g).max(b);
        let min = r.min(g).min(b);
        let sat = max - min;
        let luma = 0.2126 * r + 0.7152 * g + 0.0722 * b;

        let vivid = 0.18 + sat * 1.1 + (0.58 - (luma - 0.58).abs()).max(0.0) * 0.65;
        vibrant_sum[0] += r * vivid;
        vibrant_sum[1] += g * vivid;
        vibrant_sum[2] += b * vivid;
        vibrant_weight += vivid;

        let deep = 0.16 + (1.0 - luma) * 0.95 + sat * 0.35;
        deep_sum[0] += r * deep;
        deep_sum[1] += g * deep;
        deep_sum[2] += b * deep;
        deep_weight += deep;
    }

    if vibrant_weight <= f32::EPSILON || deep_weight <= f32::EPSILON {
        return None;
    }

    let vibrant = [
        (vibrant_sum[0] / vibrant_weight).powf(0.9),
        (vibrant_sum[1] / vibrant_weight).powf(0.9),
        (vibrant_sum[2] / vibrant_weight).powf(0.9),
    ];
    let deep = [
        (deep_sum[0] / deep_weight * 0.62).clamp(0.0, 1.0),
        (deep_sum[1] / deep_weight * 0.62).clamp(0.0, 1.0),
        (deep_sum[2] / deep_weight * 0.62).clamp(0.0, 1.0),
    ];

    Some((
        Color32::from_rgb(
            (vibrant[0] * 255.0).round() as u8,
            (vibrant[1] * 255.0).round() as u8,
            (vibrant[2] * 255.0).round() as u8,
        ),
        Color32::from_rgb(
            (deep[0] * 255.0).round() as u8,
            (deep[1] * 255.0).round() as u8,
            (deep[2] * 255.0).round() as u8,
        ),
    ))
}
