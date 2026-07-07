//! Dual-loop meter panel (S-meter + IF AGC + AF peak).

use eframe::egui::{self, Color32, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

use crate::theme::{attach_rich_tooltip, ACCENT, MUTED, TRACE};

use super::level::HALF_SCALE;
use super::motion::MeterSmoothed;
use super::s_meter::{show_analog_s_meter, AnalogSmeterParams};

const LOOP_METER_LABEL_H: f32 = 13.0;
const LOOP_METER_LABEL_GAP: f32 = 2.0;
const LOOP_METER_BAR_H: f32 = 7.0;
const LOOP_METER_ROW_H: f32 = LOOP_METER_LABEL_H + LOOP_METER_LABEL_GAP + LOOP_METER_BAR_H;
const LOOP_METER_ROW_GAP: f32 = 5.0;
/// IF AGC bar: high = boosting weak RF, low = pulling back hot RF (classic dual-loop).
pub(crate) fn if_agc_fill(agc_gain: f32, agc_enabled: bool) -> f32 {
    if !agc_enabled {
        return 0.5;
    }
    let g = agc_gain.clamp(0.02, 64.0);
    (g.log10() / 64.0_f32.log10()).clamp(0.0, 1.0)
}

pub(crate) fn af_peak_fill(peak: f32) -> f32 {
    (peak / HALF_SCALE).clamp(0.0, 1.0)
}

pub struct DualAgcParams {
    pub rf_dbm: f32,
    pub hw_rssi_dbm: Option<f32>,
    pub agc_enabled: bool,
    pub streaming: bool,
}

fn paint_modern_meter_bar(
    painter: &eframe::egui::Painter,
    rect: Rect,
    fill: f32,
    accent: Color32,
    live: bool,
) {
    let r = rect.height() * 0.5;
    painter.rect_filled(rect, r, Color32::from_rgb(12, 15, 22));
    painter.rect_stroke(
        rect,
        r,
        Stroke::new(1.0, Color32::from_rgb(32, 40, 54)),
        StrokeKind::Inside,
    );
    let inner = rect.shrink2(Vec2::new(1.0, 1.0));
    let inner_r = (inner.height() * 0.5).max(1.0);
    if live {
        let t = fill.clamp(0.0, 1.0);
        if t > 0.002 {
            let fill_w = inner.width() * t;
            let fill_rect = Rect::from_min_max(
                inner.left_top(),
                Pos2::new(inner.left() + fill_w, inner.bottom()),
            );
            let glow = Color32::from_rgba_unmultiplied(
                accent.r(),
                accent.g(),
                accent.b(),
                55,
            );
            painter.rect_filled(fill_rect.expand(0.5), inner_r + 0.5, glow);
            painter.rect_filled(fill_rect, inner_r, accent);
            let shine = Rect::from_min_max(
                fill_rect.left_top(),
                Pos2::new(fill_rect.right(), fill_rect.top() + fill_rect.height() * 0.38),
            );
            painter.rect_filled(
                shine,
                inner_r,
                Color32::from_rgba_unmultiplied(255, 255, 255, 28),
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn paint_loop_meter(
    ui: &mut Ui,
    label: &str,
    value: &str,
    fill: f32,
    accent: Color32,
    tip: &[(&str, Color32)],
    live: bool,
    width: f32,
) {
    ui.allocate_ui_with_layout(
        Vec2::new(width, LOOP_METER_ROW_H),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_max_width(width);
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new(label).small().color(MUTED));
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        let color = if live { accent } else { MUTED };
                        ui.label(egui::RichText::new(value).monospace().small().color(color));
                    },
                );
            });
            let (rect, resp) =
                ui.allocate_exact_size(Vec2::new(width, LOOP_METER_BAR_H), Sense::hover());
            paint_modern_meter_bar(&ui.painter_at(rect), rect, fill, accent, live);
            if live {
                attach_rich_tooltip(&resp, Some(label), tip);
            }
        },
    );
}

/// RF / IF / AF feedback — analog S-meter plus IF/AF level bars.
pub fn show_dual_agc_loop(ui: &mut Ui, p: &DualAgcParams, smoothed: MeterSmoothed) {
    let panel_w = ui.available_width();
    let block_h = LOOP_METER_ROW_H * 2.0 + LOOP_METER_ROW_GAP;
    ui.vertical(|ui| {
        ui.set_max_width(panel_w);
        ui.add_space(2.0);
        show_analog_s_meter(
            ui,
            &AnalogSmeterParams {
                dbm: p.rf_dbm,
                hw_rssi_dbm: p.hw_rssi_dbm,
                streaming: p.streaming,
            },
            smoothed.needle_t,
        );
        ui.add_space(LOOP_METER_ROW_GAP);
        ui.allocate_ui_with_layout(
            Vec2::new(panel_w, block_h),
            egui::Layout::top_down(egui::Align::LEFT),
            |ui| {
                ui.set_max_width(panel_w);
                let live = p.streaming;
                let (if_value, if_fill, if_accent) = if live && p.agc_enabled {
                    (
                        format!("{:.1}×", smoothed.if_gain_display),
                        smoothed.if_fill,
                        ACCENT,
                    )
                } else if live {
                    ("off".to_string(), smoothed.if_fill, MUTED)
                } else {
                    ("—".to_string(), 0.0, MUTED)
                };
                paint_loop_meter(
                    ui,
                    "IF IQ AGC",
                    &if_value,
                    if_fill,
                    if_accent,
                    &[
                        ("Software loop", ACCENT),
                        (
                            "Compensates RF level — independent of the S-meter needle.",
                            MUTED,
                        ),
                    ],
                    live,
                    panel_w,
                );
                ui.add_space(LOOP_METER_ROW_GAP);
                let (af_value, af_fill) = if live {
                    (format!("{:.2}", smoothed.af_peak_display), smoothed.af_fill)
                } else {
                    ("—".to_string(), 0.0)
                };
                paint_loop_meter(
                    ui,
                    "AF peak",
                    &af_value,
                    af_fill,
                    TRACE,
                    &[
                        ("Post-AGC audio", TRACE),
                        ("Stable near half scale when IQ AGC is on.", MUTED),
                    ],
                    live,
                    panel_w,
                );
            },
        );
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn if_agc_fill_disabled_is_mid() {
        assert!((if_agc_fill(32.0, false) - 0.5).abs() < 1e-5);
    }

    #[test]
    fn if_agc_fill_scales_log_gain() {
        let low = if_agc_fill(2.0, true);
        let mid = if_agc_fill(8.0, true);
        let high = if_agc_fill(64.0, true);
        assert!(low < mid);
        assert!(mid < high);
        assert!((high - 1.0).abs() < 0.01);
    }

    #[test]
    fn af_peak_fill_clamps_to_half_scale() {
        assert!((af_peak_fill(0.0) - 0.0).abs() < 1e-5);
        assert!((af_peak_fill(HALF_SCALE) - 1.0).abs() < 1e-5);
        assert!((af_peak_fill(10.0) - 1.0).abs() < 1e-5);
    }
}
