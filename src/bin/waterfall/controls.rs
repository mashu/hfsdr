//! Scroll-friendly control widgets for the side panel.

use std::ops::RangeInclusive;

use eframe::egui::{ComboBox, DragValue, FontId, Response, RichText, Slider, Ui, Vec2};

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
