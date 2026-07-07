//! AF tuning oscilloscope and level badge.

use eframe::egui::{
    self, Align2, Button, Color32, CornerRadius, FontId, Frame, Pos2, Rect, RichText, Sense,
    Shape, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::{attach_rich_tooltip, ACCENT, MUTED, OK, SURFACE, TRACE, WARN};

use super::level::{dbm_to_s_reading, rf_level_dbm, AudioLevelHint, HALF_SCALE};
use super::af_scope_state::{
    downsample_waveform, waveform_envelope_overlay, AfScopeMode, AfScopeViewState,
};

fn hint_short(h: AudioLevelHint) -> &'static str {
    match h {
        AudioLevelHint::Idle => "—",
        AudioLevelHint::TooQuiet => "LOW",
        AudioLevelHint::SweetSpot => "OK",
        AudioLevelHint::TooHot => "HOT",
    }
}

fn hint_tip_lines(h: AudioLevelHint) -> &'static [(&'static str, Color32)] {
    match h {
        AudioLevelHint::Idle => &[
            ("Waiting", MUTED),
            ("Audio stream not active yet.", MUTED),
        ],
        AudioLevelHint::TooQuiet => &[
            ("Too quiet", WARN),
            ("Raise RF gain — S-meter should rise.", MUTED),
            ("IQ AGC still has headroom (high ×).", OK),
        ],
        AudioLevelHint::SweetSpot => &[
            ("Sweet spot", OK),
            ("RF level healthy, IQ AGC not pinned, AF near half scale.", MUTED),
        ],
        AudioLevelHint::TooHot => &[
            ("Too hot", WARN),
            (
                "Lower RF gain — S-meter or IQ envelope hot; IQ AGC crushed or AF clipping.",
                MUTED,
            ),
        ],
    }
}

fn hint_accent(h: AudioLevelHint) -> Color32 {
    match h {
        AudioLevelHint::Idle => MUTED,
        AudioLevelHint::TooQuiet => WARN,
        AudioLevelHint::SweetSpot => OK,
        AudioLevelHint::TooHot => Color32::from_rgb(248, 113, 113),
    }
}

pub struct AfScopeParams<'a> {
    pub envelope: &'a [f32],
    pub waveform: &'a [f32],
    pub peak: f32,
    pub peak_display: f32,
    pub rms: f32,
    pub agc_gain: f32,
    pub agc_envelope: f32,
    pub agc_enabled: bool,
    pub agc_target: f32,
    pub iq_headroom: f32,
    pub rssi_dbm: Option<f32>,
    pub iq_rf_level: f32,
    pub streaming: bool,
    pub hint: AudioLevelHint,
}

pub fn show_af_tuning_panel(ui: &mut Ui, view: &mut AfScopeViewState, p: &AfScopeParams<'_>) {
    let panel_w = ui.available_width();
    Frame::new()
        .fill(Color32::from_rgb(18, 22, 30))
        .stroke(Stroke::new(1.0, Color32::from_rgb(42, 52, 70)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_max_width(panel_w);
            scope_control_strip(ui, view, p.waveform, p.hint);
            ui.add_space(4.0);
            show_af_scope(ui, view, p);
            ui.add_space(6.0);
            metric_row(ui, p);
        });
}

/// Single-line mode / accuracy / status controls — fixed height, no plot overlap.
const CONTROL_STRIP_H: f32 = 18.0;
/// Fixed width so LOW / OK / HOT never reflow sibling chips.
const STATUS_CHIP_W: f32 = 30.0;

const METRIC_CHIP_W: f32 = 48.0;
const METRIC_CHIP_H: f32 = 32.0;
const METRIC_ROW_H: f32 = 34.0;
const SCOPE_CHIP_W: f32 = 24.0;
const SCOPE_CHIP_H: f32 = 14.0;

fn metric_chip_with_tip(
    ui: &mut Ui,
    label: &str,
    value: &str,
    accent: Color32,
    tip: &[(&str, Color32)],
) {
    let response = ui.allocate_ui_with_layout(
        Vec2::new(METRIC_CHIP_W, METRIC_CHIP_H),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            ui.set_min_size(Vec2::new(METRIC_CHIP_W, METRIC_CHIP_H));
            ui.set_max_size(Vec2::new(METRIC_CHIP_W, METRIC_CHIP_H));
            ui.label(egui::RichText::new(label).small().color(MUTED));
            ui.label(egui::RichText::new(value).monospace().color(accent));
        },
    );
    attach_rich_tooltip(&response.response, Some(label), tip);
}

fn metric_row(ui: &mut Ui, p: &AfScopeParams<'_>) {
    let rf_value = if p.streaming {
        dbm_to_s_reading(rf_level_dbm(p.rssi_dbm, p.iq_rf_level))
    } else {
        "—".to_string()
    };
    let row_w = ui.available_width().max(1.0);
    ui.allocate_ui_with_layout(
        Vec2::new(row_w, METRIC_ROW_H),
        egui::Layout::top_down(egui::Align::LEFT),
        |ui| {
            egui::ScrollArea::horizontal()
                .id_salt("af_scope_metrics")
                .max_width(row_w)
                .show(ui, |ui| {
                    ui.set_min_height(METRIC_ROW_H);
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = 12.0;
                        metric_chip_with_tip(
                            ui,
                            "Peak",
                            &format!("{:.3}", p.peak),
                            TRACE,
                            &[
                                ("Post-AGC audio", TRACE),
                                (
                                    "Instantaneous |audio| peak — aim near half scale (~0.45) when tuning RF gain.",
                                    MUTED,
                                ),
                            ],
                        );
                        metric_chip_with_tip(
                            ui,
                            "RMS",
                            &format!("{:.3}", p.rms),
                            MUTED,
                            &[
                                ("Average level", ACCENT),
                                (
                                    "Short-term RMS of demod audio — steadier than peak between keying edges.",
                                    MUTED,
                                ),
                            ],
                        );
                        if p.agc_enabled {
                            metric_chip_with_tip(
                                ui,
                                "IQ AGC",
                                &format!("{:.1}×", p.agc_gain),
                                ACCENT,
                                &[
                                    ("Software IF loop", ACCENT),
                                    (
                                        "Compensates RF level before demod — high × boosts weak signals, \
                                         low × pulls back hot RF. Independent of the S-meter.",
                                        MUTED,
                                    ),
                                ],
                            );
                            metric_chip_with_tip(
                                ui,
                                "Env",
                                &format!("{:.3}", p.agc_envelope),
                                MUTED,
                                &[
                                    ("IQ envelope", ACCENT),
                                    (
                                        "Level the AGC loop is tracking on filtered IQ — rises when RF is strong, \
                                         falls between characters.",
                                        MUTED,
                                    ),
                                ],
                            );
                            metric_chip_with_tip(
                                ui,
                                "Tgt",
                                &format!("{:.2}", p.agc_target),
                                MUTED,
                                &[
                                    ("AGC target", ACCENT),
                                    ("Desired IQ envelope level — set in CW demod → Level (AGC).", MUTED),
                                ],
                            );
                        }
                        metric_chip_with_tip(
                            ui,
                            "RF",
                            &rf_value,
                            if p.streaming { OK } else { MUTED },
                            &[
                                ("Pre-AGC IQ", OK),
                                (
                                    "S-unit from IQ level before software AGC — same scale as the S-meter. \
                                     Raise RF gain until RF/peak look healthy without HOT.",
                                    MUTED,
                                ),
                            ],
                        );
                        metric_chip_with_tip(
                            ui,
                            "IQ buf",
                            &format!("{:>3.0}%", p.iq_headroom * 100.0),
                            MUTED,
                            &[
                                ("Engine buffer", ACCENT),
                                (
                                    "IQ ring-buffer fill — sustained high % means the pump is falling behind \
                                     the sample stream.",
                                    MUTED,
                                ),
                            ],
                        );
                    });
                });
        },
    );
}

pub fn show_af_scope(ui: &mut Ui, view: &mut AfScopeViewState, p: &AfScopeParams<'_>) {
    let h = 96.0;
    let w = ui.available_width().max(1.0);
    let (outer, resp) = ui.allocate_exact_size(Vec2::new(w, h), Sense::hover());
    attach_rich_tooltip(
        &resp,
        Some("AF trace"),
        match view.mode {
            AfScopeMode::Level => &[
                ("Level mode", TRACE),
                (
                    "Envelope history left → right. Tune RF gain so peaks sit near ±half scale.",
                    MUTED,
                ),
                ("Accuracy", ACCENT),
                ("Coarse / Medium / Fine changes time resolution and smoothing.", MUTED),
            ][..],
            AfScopeMode::Waveform => &[
                ("Waveform mode", TRACE),
                (
                    "Raw demod audio cycles with envelope overlay — same half-scale guides for RF tuning.",
                    MUTED,
                ),
                ("Hold", OK),
                ("Freeze the current trace (oscilloscope hold).", MUTED),
                ("Phosphor", ACCENT),
                (
                    "Stack triggered sweeps; brighter regions show where the signal repeats.",
                    MUTED,
                ),
            ][..],
        },
    );
    let painter = ui.painter_at(outer);

    const LABEL_W: f32 = 20.0;
    const METER_W: f32 = 12.0;
    const INSET: f32 = 2.0;
    let plot = Rect::from_min_max(
        Pos2::new(outer.left() + LABEL_W, outer.top() + INSET),
        Pos2::new(outer.right() - METER_W - INSET * 2.0, outer.bottom() - INSET),
    );
    if plot.width() < 8.0 || plot.height() < 8.0 {
        return;
    }
    let meter_rect = Rect::from_min_max(
        Pos2::new(outer.right() - METER_W - INSET, plot.top()),
        Pos2::new(outer.right() - INSET, plot.bottom()),
    );

    paint_plot_background(&painter, plot);
    paint_grid(&painter, plot);
    paint_clip_zones(&painter, plot);
    paint_half_scale_guides(&painter, plot);
    match view.mode {
        AfScopeMode::Level => paint_level_trace(&painter, plot, p),
        AfScopeMode::Waveform => paint_waveform_scope(&painter, plot, view, p),
    }
    paint_peak_meter(&painter, meter_rect, p.peak_display, p.hint);
    paint_y_labels(&painter, outer, plot);
}

fn scope_control_strip(
    ui: &mut Ui,
    view: &mut AfScopeViewState,
    waveform: &[f32],
    hint: AudioLevelHint,
) {
    let strip_w = ui.available_width().max(1.0);
    ui.allocate_ui_with_layout(
        Vec2::new(strip_w, CONTROL_STRIP_H),
        egui::Layout::right_to_left(egui::Align::Center),
        |ui| {
            status_chip(ui, hint);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing = Vec2::new(3.0, 0.0);
                if scope_chip(
                    ui,
                    view.mode.chip(),
                    true,
                    TRACE,
                    Some("Mode — click to toggle ENV / WAV"),
                )
                .clicked()
                {
                    view.mode = view.mode.toggle();
                }
                if scope_chip(
                    ui,
                    view.accuracy.chip(),
                    true,
                    ACCENT,
                    Some("Accuracy — click to cycle C / M / F"),
                )
                .clicked()
                {
                    view.accuracy = view.accuracy.next();
                }
                if view.mode == AfScopeMode::Waveform {
                    if scope_chip(ui, "H", view.hold, OK, Some("Hold / freeze trace")).clicked() {
                        view.set_hold(!view.hold, waveform);
                    }
                    if scope_chip(ui, "P", view.phosphor, ACCENT, Some("Phosphor overlay")).clicked()
                    {
                        view.phosphor = !view.phosphor;
                    }
                    if scope_chip(ui, "×", false, MUTED, Some("Clear phosphor")).clicked() {
                        view.clear_phosphor();
                    }
                }
            });
        },
    );
}

fn status_chip(ui: &mut Ui, hint: AudioLevelHint) {
    let accent = hint_accent(hint);
    let text = RichText::new(hint_short(hint))
        .monospace()
        .size(9.0)
        .color(accent);
    let resp = ui.add(
        Button::new(text)
            .fill(Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 28))
            .stroke(Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 160),
            ))
            .corner_radius(3.0)
            .min_size(Vec2::new(STATUS_CHIP_W, SCOPE_CHIP_H)),
    );
    attach_rich_tooltip(&resp, Some("AF level"), hint_tip_lines(hint));
}

fn paint_plot_background(painter: &eframe::egui::Painter, rect: Rect) {
    painter.rect_filled(rect, 6.0, SURFACE);
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_rgb(48, 58, 76)),
        StrokeKind::Inside,
    );
    let vignette = Color32::from_rgba_unmultiplied(0, 0, 0, 35);
    painter.rect_filled(
        Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), rect.top() + 8.0)),
        0.0,
        vignette,
    );
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - 8.0), rect.right_bottom()),
        0.0,
        vignette,
    );
}

fn paint_grid(painter: &eframe::egui::Painter, rect: Rect) {
    let mid_y = rect.center().y;
    let half_h = rect.height() * 0.44;
    let grid = Color32::from_rgba_unmultiplied(80, 95, 120, 45);
    for frac in [0.25, 0.5, 0.75] {
        for sign in [-1.0f32, 1.0] {
            let y = mid_y - sign * frac * half_h;
            painter.line_segment(
                [Pos2::new(rect.left() + 2.0, y), Pos2::new(rect.right() - 2.0, y)],
                Stroke::new(1.0, grid),
            );
        }
    }
    painter.line_segment(
        [Pos2::new(rect.left() + 2.0, mid_y), Pos2::new(rect.right() - 2.0, mid_y)],
        Stroke::new(1.25, Color32::from_rgba_unmultiplied(100, 120, 150, 90)),
    );
}

fn paint_clip_zones(painter: &eframe::egui::Painter, rect: Rect) {
    let band = rect.height() * 0.08;
    let hot = Color32::from_rgba_unmultiplied(248, 113, 113, 18);
    painter.rect_filled(
        Rect::from_min_max(rect.left_top(), Pos2::new(rect.right(), rect.top() + band)),
        0.0,
        hot,
    );
    painter.rect_filled(
        Rect::from_min_max(Pos2::new(rect.left(), rect.bottom() - band), rect.right_bottom()),
        0.0,
        hot,
    );
}

fn paint_half_scale_guides(painter: &eframe::egui::Painter, rect: Rect) {
    let mid_y = rect.center().y;
    let half_h = rect.height() * 0.44;
    let guide = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 130);
    for sign in [-1.0f32, 1.0] {
        let y = mid_y - sign * HALF_SCALE * half_h;
        let mut pts = Vec::new();
        let x0 = rect.left() + 4.0;
        let x1 = rect.right() - 4.0;
        let dash = 5.0;
        let mut x = x0;
        while x < x1 {
            let x_end = (x + dash).min(x1);
            pts.push(Pos2::new(x, y));
            pts.push(Pos2::new(x_end, y));
            x += dash * 2.2;
        }
        painter.add(Shape::line(pts, Stroke::new(1.0, guide)));
    }
}

fn scope_chip(
    ui: &mut Ui,
    label: &str,
    active: bool,
    accent: Color32,
    tip: Option<&str>,
) -> egui::Response {
    let text = RichText::new(label)
        .monospace()
        .size(9.0)
        .color(if active { accent } else { MUTED });
    let fill = if active {
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 32)
    } else {
        Color32::from_rgba_unmultiplied(8, 10, 14, 140)
    };
    let stroke = if active {
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 180))
    } else {
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 72, 92, 120))
    };
    let resp = ui.add(
        Button::new(text)
            .fill(fill)
            .stroke(stroke)
            .corner_radius(3.0)
            .min_size(Vec2::new(SCOPE_CHIP_W, SCOPE_CHIP_H)),
    );
    if let Some(tip) = tip {
        resp.on_hover_text(tip)
    } else {
        resp
    }
}

fn paint_level_trace(painter: &eframe::egui::Painter, rect: Rect, p: &AfScopeParams<'_>) {
    if p.envelope.len() < 2 {
        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - 10.0),
            Align2::CENTER_BOTTOM,
            "No audio",
            FontId::monospace(10.0),
            MUTED,
        );
        return;
    }

    let mid_y = rect.center().y;
    let half_h = rect.height() * 0.44;
    let n = p.envelope.len();
    let col_w = (rect.width() - 4.0) / n as f32;
    let clip_top = rect.top() + rect.height() * 0.08;
    let clip_bot = rect.bottom() - rect.height() * 0.08;
    let mut clipped = false;

    for (col, &env) in p.envelope.iter().enumerate() {
        let e = env.clamp(0.0, 1.15);
        let x = rect.left() + 2.0 + col as f32 * col_w + col_w * 0.5;
        let y_top = mid_y - e * half_h;
        let y_bot = mid_y + e * half_h;
        if y_top <= clip_top || y_bot >= clip_bot {
            clipped = true;
        }
        painter.line_segment(
            [Pos2::new(x, y_top), Pos2::new(x, y_bot)],
            Stroke::new(1.35, TRACE),
        );
    }

    if clipped {
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(251, 146, 120, 90)),
            StrokeKind::Inside,
        );
    }
}

fn paint_waveform_scope(
    painter: &eframe::egui::Painter,
    rect: Rect,
    view: &AfScopeViewState,
    p: &AfScopeParams<'_>,
) {
    let live = view.waveform_for_display(p.waveform);
    if live.len() < 4 {
        painter.text(
            Pos2::new(rect.center().x, rect.bottom() - 10.0),
            Align2::CENTER_BOTTOM,
            "No audio",
            FontId::monospace(10.0),
            MUTED,
        );
        return;
    }

    let window = view.accuracy.waveform_samples(live.len());
    let start = live.len().saturating_sub(window);
    let slice = &live[start..];
    let plot_cols = (rect.width() as usize).clamp(64, 512);
    let mid_y = rect.center().y;
    let half_h = rect.height() * 0.44;
    let clip_top = rect.top() + rect.height() * 0.08;
    let clip_bot = rect.bottom() - rect.height() * 0.08;

    if view.phosphor {
        let (grid, gcols, grows) = view.phosphor_grid();
        let cell_w = rect.width() / gcols as f32;
        let cell_h = rect.height() / grows as f32;
        for row in 0..grows {
            for col in 0..gcols {
                let idx = row * gcols + col;
                let Some(&intensity) = grid.get(idx) else {
                    continue;
                };
                if intensity < 0.08 {
                    continue;
                }
                let t = (intensity / 10.0).clamp(0.0, 1.0);
                let color = Color32::from_rgba_unmultiplied(
                    (30.0 + t * 120.0) as u8,
                    (70.0 + t * 150.0) as u8,
                    (160.0 + t * 95.0) as u8,
                    (40.0 + t * 200.0) as u8,
                );
                let cell = Rect::from_min_max(
                    Pos2::new(rect.left() + col as f32 * cell_w, rect.top() + row as f32 * cell_h),
                    Pos2::new(
                        rect.left() + (col + 1) as f32 * cell_w,
                        rect.top() + (row + 1) as f32 * cell_h,
                    ),
                );
                painter.rect_filled(cell, 0.0, color);
            }
        }
    }

    let env = waveform_envelope_overlay(slice, plot_cols);
    let env_stroke = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 140);
    let col_w = rect.width() / plot_cols as f32;
    for (col, &e) in env.iter().enumerate() {
        let x = rect.left() + col as f32 * col_w + col_w * 0.5;
        let y_top = mid_y - e.clamp(0.0, 1.15) * half_h;
        let y_bot = mid_y + e.clamp(0.0, 1.15) * half_h;
        painter.line_segment(
            [Pos2::new(x, y_top), Pos2::new(x, y_bot)],
            Stroke::new(1.0, env_stroke),
        );
    }

    let trace = downsample_waveform(slice, plot_cols);
    if trace.len() < 2 {
        return;
    }
    let mut points = Vec::with_capacity(trace.len());
    let mut clipped = false;
    for (i, &sample) in trace.iter().enumerate() {
        let x = rect.left() + 2.0 + i as f32 * (rect.width() - 4.0) / (trace.len() - 1).max(1) as f32;
        let y = mid_y - sample.clamp(-1.15, 1.15) * half_h;
        if y <= clip_top || y >= clip_bot {
            clipped = true;
        }
        points.push(Pos2::new(x, y));
    }
    painter.add(Shape::line(
        points,
        Stroke::new(1.2, Color32::from_rgba_unmultiplied(TRACE.r(), TRACE.g(), TRACE.b(), 220)),
    ));

    if clipped {
        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(251, 146, 120, 90)),
            StrokeKind::Inside,
        );
    }
}

fn paint_peak_meter(
    painter: &eframe::egui::Painter,
    rect: Rect,
    peak: f32,
    hint: AudioLevelHint,
) {
    painter.rect_filled(rect, 3.0, Color32::from_rgb(12, 16, 22));
    painter.rect_stroke(
        rect,
        3.0,
        Stroke::new(1.0, Color32::from_rgb(50, 60, 78)),
        StrokeKind::Inside,
    );

    let fill_frac = (peak / 1.0).clamp(0.0, 1.0);
    let target_frac = HALF_SCALE;
    let bar_h = rect.height() - 4.0;
    let fill_h = bar_h * fill_frac;
    let fill_rect = Rect::from_min_max(
        Pos2::new(rect.left() + 2.0, rect.bottom() - 2.0 - fill_h),
        Pos2::new(rect.right() - 2.0, rect.bottom() - 2.0),
    );
    let fill_color = hint_accent(hint);
    painter.rect_filled(fill_rect, 2.0, Color32::from_rgba_unmultiplied(
        fill_color.r(),
        fill_color.g(),
        fill_color.b(),
        200,
    ));

    let target_y = rect.bottom() - 2.0 - bar_h * target_frac;
    painter.line_segment(
        [
            Pos2::new(rect.left(), target_y),
            Pos2::new(rect.right(), target_y),
        ],
        Stroke::new(1.5, ACCENT),
    );
}

fn paint_y_labels(painter: &eframe::egui::Painter, outer: Rect, plot: Rect) {
    let mid_y = plot.center().y;
    let half_h = plot.height() * 0.44;
    let label_x = outer.left() + 4.0;
    let mono = FontId::monospace(9.0);
    let label_c = Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 180);

    painter.text(
        Pos2::new(label_x, plot.top() + 2.0),
        Align2::LEFT_TOP,
        "+1",
        mono.clone(),
        label_c,
    );
    painter.text(
        Pos2::new(label_x, mid_y),
        Align2::LEFT_CENTER,
        "0",
        mono.clone(),
        label_c,
    );
    painter.text(
        Pos2::new(label_x, plot.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        "−1",
        mono,
        label_c,
    );

    let half_y = mid_y - HALF_SCALE * half_h;
    painter.text(
        Pos2::new(label_x, half_y),
        Align2::LEFT_CENTER,
        "−6",
        FontId::monospace(8.0),
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 160),
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meters::agc_loop::if_agc_fill;
    use crate::meters::level::{classify_level, dbm_to_needle_t, dbm_to_s_reading, rf_level_dbm};

    #[test]
    fn quiet_when_peak_low() {
        assert_eq!(
            classify_level(0.02, true, 20.0, 0.01, 0.25, true),
            AudioLevelHint::TooQuiet
        );
    }

    #[test]
    fn hot_when_clipping() {
        assert_eq!(
            classify_level(0.95, true, 0.05, 0.5, 0.25, true),
            AudioLevelHint::TooHot
        );
    }

    #[test]
    fn sweet_in_mid_range() {
        assert_eq!(
            classify_level(0.35, true, 2.0, 0.2, 0.25, true),
            AudioLevelHint::SweetSpot
        );
    }

    #[test]
    fn rf_level_follows_iq_and_ignores_hardware_rssi() {
        // S9 calibration anchor: iq == 0.1 → −73 dBm.
        assert!((rf_level_dbm(None, 0.1) - (-73.0)).abs() < 0.5);
        // Hardware RSSI must not change the needle — it is a reference only.
        assert_eq!(rf_level_dbm(Some(-30.0), 0.1), rf_level_dbm(None, 0.1));
        // Software RF gain (×10 IQ ≈ +20 dB) moves the needle by ~20 dB.
        let lo = rf_level_dbm(None, 0.01);
        let hi = rf_level_dbm(None, 0.1);
        assert!((hi - lo - 20.0).abs() < 0.5);
        // Silence pins to the bottom of the scale.
        assert_eq!(rf_level_dbm(None, 0.0), -127.0);
    }

    #[test]
    fn s_reading_s9_and_over() {
        assert_eq!(dbm_to_s_reading(-73.0), "S9");
        assert_eq!(dbm_to_s_reading(-63.0), "S9+2");
    }

    #[test]
    fn s_reading_weak_signal() {
        assert_eq!(dbm_to_s_reading(-100.0), "S5");
    }

    #[test]
    fn if_agc_fill_monotonic_with_gain() {
        assert!(if_agc_fill(2.0, true) > if_agc_fill(1.0, true));
        assert!(if_agc_fill(16.0, true) > if_agc_fill(2.0, true));
        assert!((if_agc_fill(1.0, false) - 0.5).abs() < 1e-6);
    }

    #[test]
    fn needle_position_tracks_dbm_monotonically() {
        let t_quiet = dbm_to_needle_t(-120.0);
        let t_loud = dbm_to_needle_t(-70.0);
        assert!(t_loud > t_quiet);
        assert!((dbm_to_needle_t(-127.0) - 0.0).abs() < 1e-6);
        assert!((dbm_to_needle_t(-33.0) - 1.0).abs() < 1e-6);
    }
}
