//! Scroll-friendly control widgets for the side panel.

use std::ops::RangeInclusive;

use eframe::egui::{
    self, Align, Button, Color32, ComboBox, CornerRadius, DragValue, FontId, Frame, Layout,
    Margin, Response, RichText, Sense, Slider, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::{chip_hovered, ACCENT, MUTED};

pub fn scroll_slider_f32(ui: &mut Ui, value: &mut f32, range: RangeInclusive<f32>, label: &str) -> Response {
    let span = *range.end() - *range.start();
    scroll_slider_f32_step(ui, value, range, label, span / 120.0)
}

/// Like [`scroll_slider_f32`] but with an explicit wheel step (Hz, dB, etc.).
pub fn scroll_slider_f32_step(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    label: &str,
    scroll_step: f32,
) -> Response {
    let response = ui.add(Slider::new(value, range.clone()).text(label));
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 2.0 {
            let delta = if scroll > 0.0 { scroll_step } else { -scroll_step };
            *value = (*value + delta).clamp(*range.start(), *range.end());
        }
    }
    response
}

pub struct OffsetControlOutput {
    pub changed: bool,
    pub clear_clicked: bool,
}

pub struct RitControlOutput {
    pub changed: bool,
    pub clear_clicked: bool,
    pub toggle_clicked: bool,
}

/// Bidirectional Hz offset track with readout and ±10 Hz steps.
pub fn offset_control(
    ui: &mut Ui,
    title: &str,
    subtitle: &str,
    clear_tooltip: &str,
    value: &mut f32,
    range: RangeInclusive<f32>,
    step_hint: &str,
) -> OffsetControlOutput {
    let min_hz = *range.start();
    let max_hz = *range.end();
    let mut out = OffsetControlOutput {
        changed: false,
        clear_clicked: false,
    };
    let active = value.abs() > 0.5;
    let value_color = if active { ACCENT } else { MUTED };

    Frame::new()
        .fill(Color32::from_rgb(18, 22, 30))
        .corner_radius(CornerRadius::same(8))
        .stroke(Stroke::new(1.0, Color32::from_rgb(42, 50, 66)))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            let w = ui.available_width();
            ui.set_min_width(w);
            ui.set_max_width(w);

            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(title)
                        .small()
                        .strong()
                        .color(if active { ACCENT } else { MUTED }),
                );
                ui.label(RichText::new(subtitle).small().color(MUTED));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let sign = if *value > 0.5 {
                        "+"
                    } else if *value < -0.5 {
                        "−"
                    } else {
                        ""
                    };
                    let text = if active {
                        format!("{sign}{:.0} Hz", value.abs())
                    } else {
                        "0 Hz".to_string()
                    };
                    ui.label(RichText::new(text).monospace().strong().color(value_color));
                });
            });
            ui.add_space(6.0);

            if offset_slider_track(ui, value, min_hz, max_hz, 1.0, step_hint).changed() {
                out.changed = true;
            }
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if offset_step_button(ui, "−10", false).clicked() {
                    *value = (*value - 10.0).clamp(min_hz, max_hz);
                    out.changed = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if offset_step_button(ui, "+10", false).clicked() {
                        *value = (*value + 10.0).clamp(min_hz, max_hz);
                        out.changed = true;
                    }
                    if offset_step_button(ui, "Clear", active)
                        .on_hover_text(clear_tooltip)
                        .clicked()
                    {
                        out.clear_clicked = true;
                    }
                });
            });
        });
    out
}

/// Bandpass center offset from the VFO (filter shift).
pub fn filter_shift_control(
    ui: &mut Ui,
    shift_hz: &mut f32,
    range: RangeInclusive<f32>,
) -> OffsetControlOutput {
    offset_control(
        ui,
        "SHIFT",
        "filter vs VFO",
        "Reset bandpass center to VFO",
        shift_hz,
        range,
        "Drag or scroll",
    )
}

/// RIT: listen offset without moving the RX center frequency.
pub fn rit_control(
    ui: &mut Ui,
    rit_on: &mut bool,
    rit_hz: &mut f32,
    range: RangeInclusive<f32>,
) -> RitControlOutput {
    let min_hz = *range.start();
    let max_hz = *range.end();
    let mut out = RitControlOutput {
        changed: false,
        clear_clicked: false,
        toggle_clicked: false,
    };
    let active = *rit_on && rit_hz.abs() > 0.5;
    let value_color = if *rit_on { ACCENT } else { MUTED };

    Frame::new()
        .fill(Color32::from_rgb(18, 22, 30))
        .corner_radius(CornerRadius::same(8))
        .stroke(Stroke::new(1.0, Color32::from_rgb(42, 50, 66)))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            let w = ui.available_width();
            ui.set_min_width(w);
            ui.set_max_width(w);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let toggle_label = if *rit_on { "RIT on" } else { "RIT off" };
                if ui
                    .add(
                        Button::new(RichText::new(toggle_label).small().strong().color(value_color))
                            .fill(if *rit_on {
                                Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 36)
                            } else {
                                Color32::from_rgb(28, 33, 44)
                            })
                            .stroke(if *rit_on {
                                Stroke::new(
                                    1.0,
                                    Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 120),
                                )
                            } else {
                                Stroke::new(1.0, Color32::from_rgb(48, 56, 72))
                            })
                            .corner_radius(CornerRadius::same(5))
                            .min_size(Vec2::new(0.0, 24.0)),
                    )
                    .on_hover_text("Toggle RIT (R) — listen offset; RX MHz unchanged")
                    .clicked()
                {
                    *rit_on = !*rit_on;
                    out.toggle_clicked = true;
                }
                ui.label(
                    RichText::new("listen offset · RX MHz unchanged")
                        .small()
                        .color(MUTED),
                );
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let sign = if *rit_hz > 0.5 {
                        "+"
                    } else if *rit_hz < -0.5 {
                        "−"
                    } else {
                        ""
                    };
                    let text = if active {
                        format!("{sign}{:.0} Hz", rit_hz.abs())
                    } else if *rit_on {
                        "0 Hz".to_string()
                    } else {
                        "off".to_string()
                    };
                    ui.label(
                        RichText::new(text)
                            .monospace()
                            .strong()
                            .color(if active { ACCENT } else { MUTED }),
                    );
                });
            });
            ui.add_space(6.0);

            if offset_slider_track(ui, rit_hz, min_hz, max_hz, 1.0, ", / . ±10 Hz").changed() {
                out.changed = true;
                *rit_on = true;
            }
            ui.add_space(6.0);

            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                if offset_step_button(ui, "−10", false).clicked() {
                    *rit_hz = (*rit_hz - 10.0).clamp(min_hz, max_hz);
                    *rit_on = true;
                    out.changed = true;
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if offset_step_button(ui, "+10", false).clicked() {
                        *rit_hz = (*rit_hz + 10.0).clamp(min_hz, max_hz);
                        *rit_on = true;
                        out.changed = true;
                    }
                    if offset_step_button(ui, "Clear", active)
                        .on_hover_text("Clear RIT — reset offset to 0 (\\)")
                        .clicked()
                    {
                        out.clear_clicked = true;
                    }
                });
            });
        });
    out
}

fn offset_slider_track(
    ui: &mut Ui,
    value: &mut f32,
    min_hz: f32,
    max_hz: f32,
    scroll_step: f32,
    hover_hint: &str,
) -> Response {
    let height = 28.0;
    let width = ui.available_width();
    let (rect, mut response) =
        ui.allocate_exact_size(Vec2::new(width, height), Sense::click_and_drag());

    let track_h = 6.0;
    let track = egui::Rect::from_center_size(
        rect.center(),
        Vec2::new(rect.width(), track_h),
    );
    let painter = ui.painter_at(rect);
    painter.rect(
        track,
        CornerRadius::same(3),
        Color32::from_rgb(32, 38, 50),
        Stroke::NONE,
        StrokeKind::Inside,
    );

    let span = max_hz - min_hz;
    let frac = if span > 0.0 {
        (*value - min_hz) / span
    } else {
        0.5
    };
    let zero_frac = (-min_hz / span).clamp(0.0, 1.0);
    let thumb_x = egui::lerp(track.left()..=track.right(), frac);
    let zero_x = egui::lerp(track.left()..=track.right(), zero_frac);

    let fill_left = thumb_x.min(zero_x);
    let fill_right = thumb_x.max(zero_x);
    if (fill_right - fill_left) > 0.5 {
        let fill_color = if (*value).abs() > 0.5 {
            Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 90)
        } else {
            Color32::TRANSPARENT
        };
        painter.rect(
            egui::Rect::from_min_max(
                egui::pos2(fill_left, track.top()),
                egui::pos2(fill_right, track.bottom()),
            ),
            CornerRadius::same(3),
            fill_color,
            Stroke::NONE,
            StrokeKind::Inside,
        );
    }

    painter.vline(
        zero_x,
        track.top() - 2.0..=track.bottom() + 2.0,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 120)),
    );

    let thumb_r = if response.hovered() || response.dragged() { 7.0 } else { 6.0 };
    let thumb_color = if (*value).abs() > 0.5 { ACCENT } else { MUTED };
    painter.circle(
        egui::pos2(thumb_x, track.center().y),
        thumb_r,
        thumb_color,
        Stroke::new(1.5, Color32::from_rgb(14, 17, 24)),
    );

    if let Some(pos) = response.interact_pointer_pos() {
        if response.dragged() || response.clicked() {
            let t = ((pos.x - track.left()) / track.width()).clamp(0.0, 1.0);
            let next = min_hz + t * span;
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                response.mark_changed();
            }
        }
    }

    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 2.0 {
            let delta = if scroll > 0.0 { scroll_step } else { -scroll_step };
            let next = (*value + delta).clamp(min_hz, max_hz);
            if (next - *value).abs() > f32::EPSILON {
                *value = next;
                response.mark_changed();
            }
        }
        response = response.on_hover_text(format!(
            "{hover_hint} · {min_hz:.0}…{max_hz:.0} Hz"
        ));
    }

    response
}

fn offset_step_button(ui: &mut Ui, label: &str, accent: bool) -> Response {
    let (fill, stroke, text) = if accent {
        (
            Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 28),
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 100),
            ),
            ACCENT,
        )
    } else {
        (
            Color32::from_rgb(28, 33, 44),
            Stroke::new(1.0, Color32::from_rgb(48, 56, 72)),
            Color32::from_rgb(190, 198, 214),
        )
    };
    ui.add(
        Button::new(RichText::new(label).small().color(text))
            .fill(fill)
            .stroke(stroke)
            .corner_radius(CornerRadius::same(5))
            .min_size(Vec2::new(0.0, 22.0)),
    )
}

pub fn scroll_slider_log_f32(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    label: &str,
) -> Response {
    let response = ui.add(Slider::new(value, range.clone()).logarithmic(true).text(label));
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 2.0 {
            let factor = if scroll > 0.0 { 0.94 } else { 1.06 };
            let new = (*value * factor).round();
            *value = new.clamp(*range.start(), *range.end());
        }
    }
    response
}

/// Preset combobox plus a custom numeric field (e.g. Kiwi connection options).
pub fn preset_combo_u32(
    ui: &mut Ui,
    id: &str,
    value: &mut u32,
    presets: &[(&str, u32)],
    custom_label: &str,
    custom_range: RangeInclusive<u32>,
) -> bool {
    let mut changed = false;
    let selected = presets
        .iter()
        .find(|(_, v)| *v == *value)
        .map(|(label, _)| *label)
        .unwrap_or("Custom");
    ui.horizontal(|ui| {
        ComboBox::from_id_salt(id)
            .selected_text(selected)
            .width(140.0)
            .show_ui(ui, |ui| {
                for (label, preset) in presets {
                    if ui.selectable_label(*value == *preset, *label).clicked() {
                        *value = *preset;
                        changed = true;
                    }
                }
            });
        let custom = ui
            .add(
                DragValue::new(value)
                    .range(custom_range)
                    .speed(100.0)
                    .prefix(custom_label),
            )
            .changed();
        changed |= custom;
    });
    changed
}

/// Preset combobox plus custom `f64` field (e.g. LNB offset in kHz).
pub fn preset_combo_f64(
    ui: &mut Ui,
    id: &str,
    value: &mut f64,
    presets: &[(&str, f64)],
    custom_label: &str,
    custom_range: RangeInclusive<f64>,
) -> bool {
    let mut changed = false;
    let selected = presets
        .iter()
        .find(|(_, v)| (*v - *value).abs() < 0.5)
        .map(|(label, _)| *label)
        .unwrap_or("Custom");
    ui.horizontal(|ui| {
        ComboBox::from_id_salt(id)
            .selected_text(selected)
            .width(140.0)
            .show_ui(ui, |ui| {
                for (label, preset) in presets {
                    if ui.selectable_label((*value - *preset).abs() < 0.5, *label).clicked() {
                        *value = *preset;
                        changed = true;
                    }
                }
            });
        let custom = ui
            .add(
                DragValue::new(value)
                    .range(custom_range)
                    .speed(10.0)
                    .prefix(custom_label),
            )
            .changed();
        changed |= custom;
    });
    changed
}

const VFO_MIN_HZ: u64 = 0;
const VFO_MAX_HZ: u64 = 60_000_000;
/// Digit place values for `XX.XXX.XXX` MHz (tens MHz … 1 Hz).
const VFO_PLACES: [u64; 8] = [10_000_000, 1_000_000, 100_000, 10_000, 1_000, 100, 10, 1];

/// Radio-style VFO: each digit scrolls independently (Hz resolution at the right).
pub fn vfo_wheel_khz(ui: &mut Ui, center_khz: &mut f64) -> bool {
    let mut hz = (*center_khz * 1000.0).round().clamp(0.0, VFO_MAX_HZ as f64) as u64;
    let mut changed = false;

    ui.vertical(|ui| {
        ui.label(RichText::new("RX frequency").small().color(MUTED));
        ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            for (i, &place) in VFO_PLACES.iter().enumerate() {
                if i == 2 || i == 5 {
                    ui.label(RichText::new(".").size(16.0).color(MUTED));
                }
                if digit_wheel(ui, &mut hz, place) {
                    changed = true;
                }
            }
            ui.add_space(4.0);
            ui.label(RichText::new("MHz").small().color(MUTED));
        });
    });

    if changed {
        *center_khz = hz as f64 / 1000.0;
    }
    changed
}

fn digit_wheel(ui: &mut Ui, hz: &mut u64, place: u64) -> bool {
    let digit = ((*hz / place) % 10) as u8;
    let size = Vec2::new(24.0, 30.0);
    let (rect, resp) = ui.allocate_exact_size(size, eframe::egui::Sense::hover());
    let hovered = chip_hovered(ui, rect, &resp);
    if hovered {
        let label = if place >= 1_000_000 {
            format!("{:.0} MHz per step", place as f64 / 1_000_000.0)
        } else if place >= 1_000 {
            format!("{:.0} kHz per step", place as f64 / 1_000.0)
        } else {
            format!("{place} Hz per step")
        };
        resp.clone().on_hover_text(label);
    }
    let fill = if hovered {
        eframe::egui::Color32::from_rgb(48, 58, 78)
    } else {
        eframe::egui::Color32::from_rgb(32, 38, 52)
    };
    let stroke = if hovered {
        eframe::egui::Stroke::new(1.0, ACCENT)
    } else {
        eframe::egui::Stroke::new(1.0, eframe::egui::Color32::from_rgb(55, 65, 85))
    };
    ui.painter().rect(
        rect,
        eframe::egui::CornerRadius::same(4),
        fill,
        stroke,
        eframe::egui::StrokeKind::Inside,
    );
    ui.painter().text(
        rect.center(),
        eframe::egui::Align2::CENTER_CENTER,
        digit.to_string(),
        FontId::monospace(15.0),
        if hovered { ACCENT } else { eframe::egui::Color32::WHITE },
    );

    if hovered {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 1.0 {
            let dir = if scroll > 0.0 { 1i64 } else { -1i64 };
            let next = (*hz as i64 + dir * place as i64).clamp(VFO_MIN_HZ as i64, VFO_MAX_HZ as i64) as u64;
            if next != *hz {
                *hz = next;
                return true;
            }
        }
    }
    false
}
