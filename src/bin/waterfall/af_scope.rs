//! AF oscilloscope and RF-gain tuning hints (FTX-1 / classic superhet style).
//!
//! Bipolar audio trace around zero: barely lifting = too little front-end gain;
//! pinned to the rails = AGC riding noise continuously.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};

use crate::theme::{attach_rich_tooltip, ACCENT, MUTED, OK, SURFACE, TRACE, TRACE_GLOW, WARN};

pub const SCOPE_LEN: usize = 320;
/// Classic “half scale” target (~−6 dB of full swing).
pub const HALF_SCALE: f32 = 0.45;
const LOOP_METER_LABEL_H: f32 = 13.0;
const LOOP_METER_LABEL_GAP: f32 = 2.0;
const LOOP_METER_BAR_H: f32 = 7.0;
const LOOP_METER_ROW_H: f32 = LOOP_METER_LABEL_H + LOOP_METER_LABEL_GAP + LOOP_METER_BAR_H;
const LOOP_METER_ROW_GAP: f32 = 5.0;
const ANALOG_SMETER_FACE_MARGIN: f32 = 2.0;
const ANALOG_SMETER_ARC_TOP_PAD: f32 = 4.0;
const ANALOG_SMETER_ARC_H_MIN: f32 = 82.0;
const ANALOG_SMETER_ARC_H_MAX: f32 = 112.0;
const ANALOG_SMETER_CAPTION_H: f32 = 16.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioLevelHint {
    Idle,
    TooQuiet,
    SweetSpot,
    TooHot,
}

pub fn classify_level(
    peak: f32,
    agc_enabled: bool,
    agc_gain: f32,
    agc_envelope: f32,
    agc_target: f32,
    streaming: bool,
) -> AudioLevelHint {
    if !streaming {
        return AudioLevelHint::Idle;
    }
    if peak < 1e-5 && agc_envelope < 1e-5 {
        return AudioLevelHint::Idle;
    }
    let agc_starved = agc_enabled && agc_gain > 14.0;
    let agc_saturated = agc_enabled && agc_gain < 0.12;
    let rf_hot = agc_enabled && agc_envelope > agc_target * 2.5;
    if peak < 0.07 || agc_starved {
        return AudioLevelHint::TooQuiet;
    }
    if peak > 0.88 || agc_saturated || rf_hot {
        return AudioLevelHint::TooHot;
    }
    if peak >= 0.10 && peak <= 0.70 && !agc_starved && !agc_saturated {
        return AudioLevelHint::SweetSpot;
    }
    if peak > 0.70 {
        AudioLevelHint::TooHot
    } else {
        AudioLevelHint::SweetSpot
    }
}

/// Fixed IQ reference for S-unit calibration (not tied to the live AGC target knob).
const SMETER_IQ_REF: f32 = 0.25;

/// Pre-software-AGC IQ level mapped to an approximate dBm scale.
pub fn iq_rf_level_to_dbm(iq_rf_level: f32) -> f32 {
    let ratio = iq_rf_level.max(1e-7) / SMETER_IQ_REF;
    (-127.0 + 20.0 * ratio.log10()).clamp(SMETER_DBM_MIN, SMETER_DBM_MAX)
}

fn combine_dbm_max(a: f32, b: f32) -> f32 {
    let pa = 10f32.powf(a / 10.0);
    let pb = 10f32.powf(b / 10.0);
    10.0 * pa.max(pb).log10()
}

/// RF level for the S-meter needle (hardware + pre-AGC IQ; independent of software AGC).
pub fn rf_level_dbm(rssi_dbm: Option<f32>, iq_rf_level: f32) -> f32 {
    let iq_dbm = iq_rf_level_to_dbm(iq_rf_level);
    let Some(hw) = rssi_dbm else {
        return iq_dbm;
    };
    if iq_rf_level > 1e-6 {
        combine_dbm_max(iq_dbm, hw)
    } else {
        hw
    }
}

/// Map dBm to classic S-unit readout (S1..S9, S9+n).
pub fn dbm_to_s_reading(dbm: f32) -> String {
    if dbm >= -73.0 {
        let over = ((dbm + 73.0) / 6.0).round().max(0.0) as i32;
        if over == 0 {
            "S9".to_string()
        } else {
            format!("S9+{over}")
        }
    } else {
        let s = ((dbm + 127.0) / 6.0).ceil().clamp(1.0, 9.0) as i32;
        format!("S{s}")
    }
}

const SMETER_DBM_MIN: f32 = -127.0;
const SMETER_DBM_MAX: f32 = -33.0;

fn dbm_to_needle_t(dbm: f32) -> f32 {
    ((dbm - SMETER_DBM_MIN) / (SMETER_DBM_MAX - SMETER_DBM_MIN)).clamp(0.0, 1.0)
}

fn needle_angle(t: f32) -> f32 {
    std::f32::consts::PI * (1.0 - t)
}

pub struct AnalogSmeterParams {
    pub dbm: f32,
    pub hw_rssi_dbm: Option<f32>,
    pub streaming: bool,
}

/// One-line RF level for the status bar (when the RX panel is hidden).
pub fn show_status_rf_meter(ui: &mut Ui, dbm: f32, hw_rssi_dbm: Option<f32>) {
    let reading = dbm_to_s_reading(dbm);
    let resp = ui.label(
        egui::RichText::new(format!("{reading} {dbm:.0} dBm"))
            .small()
            .monospace()
            .color(OK),
    );
    if hw_rssi_dbm.is_some() {
        attach_rich_tooltip(
            &resp,
            Some("S-meter"),
            &[
                ("Pre-AGC IQ", ACCENT),
                (
                    "Needle tracks IQ level before software AGC — rises when RF gain increases.",
                    MUTED,
                ),
                ("Kiwi HW", OK),
                (
                    "Hardware SND meter is shown in the Meter panel for comparison; it may stay flat when Kiwi RF AGC is on.",
                    MUTED,
                ),
            ],
        );
    } else {
        attach_rich_tooltip(
            &resp,
            Some("S-meter"),
            &[
                ("Pre-AGC IQ", ACCENT),
                (
                    "Needle tracks IQ level before software AGC — rises when RF gain increases.",
                    MUTED,
                ),
            ],
        );
    }
}

fn smeter_scale_marks() -> [(&'static str, f32); 8] {
    [
        ("S1", -121.0),
        ("S3", -109.0),
        ("S5", -97.0),
        ("S7", -85.0),
        ("S9", -73.0),
        ("+10", -63.0),
        ("+20", -53.0),
        ("+40", -33.0),
    ]
}

fn arc_point(center: Pos2, radius: f32, t: f32) -> Pos2 {
    let a = needle_angle(t);
    Pos2::new(center.x + radius * a.cos(), center.y - radius * a.sin())
}

fn arc_zone_color(t: f32) -> Color32 {
    let dbm = SMETER_DBM_MIN + t * (SMETER_DBM_MAX - SMETER_DBM_MIN);
    if dbm >= -53.0 {
        Color32::from_rgba_unmultiplied(248, 113, 113, 100)
    } else if dbm >= -63.0 {
        Color32::from_rgba_unmultiplied(251, 191, 36, 85)
    } else if dbm >= -73.0 {
        Color32::from_rgba_unmultiplied(110, 231, 183, 95)
    } else if dbm >= -85.0 {
        Color32::from_rgba_unmultiplied(110, 231, 183, 55)
    } else {
        Color32::from_rgba_unmultiplied(56, 189, 248, 45)
    }
}

fn paint_meter_bezel(painter: &eframe::egui::Painter, face_rect: Rect) {
    let outer = Color32::from_rgb(10, 13, 18);
    let bezel = Color32::from_rgb(38, 46, 62);
    let highlight = Color32::from_rgba_unmultiplied(255, 255, 255, 18);
    let well = Color32::from_rgb(14, 18, 26);

    painter.rect_filled(face_rect, 8.0, outer);
    painter.rect_stroke(face_rect, 8.0, Stroke::new(1.0, bezel), StrokeKind::Outside);
    let inset = face_rect.shrink(1.0);
    painter.rect_stroke(inset, 7.0, Stroke::new(1.0, highlight), StrokeKind::Inside);
    let inner = face_rect.shrink2(Vec2::new(3.0, 3.0));
    painter.rect_filled(inner, 6.0, well);
}

fn paint_arc_track(painter: &eframe::egui::Painter, center: Pos2, radius: f32) {
    let track_r = radius * 0.84;
    let steps = 96;
    for i in 0..steps {
        let t0 = i as f32 / steps as f32;
        let t1 = (i + 1) as f32 / steps as f32;
        let color = arc_zone_color((t0 + t1) * 0.5);
        painter.line_segment(
            [arc_point(center, track_r, t0), arc_point(center, track_r, t1)],
            Stroke::new(5.0, color),
        );
    }
    // Inner shadow edge
    let inner_r = radius * 0.76;
    let shadow_steps = 64;
    let mut shadow_pts = Vec::with_capacity(shadow_steps + 1);
    for i in 0..=shadow_steps {
        let t = i as f32 / shadow_steps as f32;
        shadow_pts.push(arc_point(center, inner_r, t));
    }
    painter.add(Shape::line(
        shadow_pts,
        Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(0, 0, 0, 80),
        ),
    ));
    // Outer crisp rim
    let outer_r = radius * 0.90;
    let mut rim_pts = Vec::with_capacity(shadow_steps + 1);
    for i in 0..=shadow_steps {
        let t = i as f32 / shadow_steps as f32;
        rim_pts.push(arc_point(center, outer_r, t));
    }
    painter.add(Shape::line(
        rim_pts,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(120, 140, 170, 90)),
    ));
}

fn paint_scale_ticks(painter: &eframe::egui::Painter, center: Pos2, radius: f32) {
    let tick_minor = Color32::from_rgba_unmultiplied(100, 115, 140, 160);
    let tick_major = Color32::from_rgba_unmultiplied(190, 200, 220, 220);
    let label_muted = Color32::from_rgba_unmultiplied(130, 142, 165, 220);
    let label_bright = Color32::from_rgba_unmultiplied(210, 220, 235, 240);
    let font_px = (7.5 * (radius / 58.0)).clamp(7.0, 9.5);

    for (label, mark_dbm) in smeter_scale_marks() {
        let t = dbm_to_needle_t(mark_dbm);
        let major = label.starts_with('S') || label == "+20";
        let inner = radius * (if major { 0.70 } else { 0.74 });
        let outer = radius * (if major { 0.92 } else { 0.86 });
        painter.line_segment(
            [arc_point(center, inner, t), arc_point(center, outer, t)],
            Stroke::new(if major { 1.25 } else { 0.75 }, if major { tick_major } else { tick_minor }),
        );
        if major || label == "+40" {
            let lp = arc_point(center, radius * 0.98, t);
            painter.text(
                lp,
                Align2::CENTER_CENTER,
                label,
                FontId::proportional(font_px),
                if label == "S9" || label == "+20" {
                    label_bright
                } else {
                    label_muted
                },
            );
        }
    }
}

fn paint_meter_needle(
    painter: &eframe::egui::Painter,
    center: Pos2,
    radius: f32,
    needle_t: f32,
    streaming: bool,
) {
    let a = needle_angle(needle_t);
    let dir = Vec2::new(a.cos(), -a.sin());
    let len = radius * 0.86;
    let tip = center + dir * len;
    let base_l = center + Vec2::new(-dir.y, dir.x) * 2.0;
    let base_r = center + Vec2::new(dir.y, -dir.x) * 2.0;

    let accent = if !streaming {
        MUTED
    } else {
        let dbm = SMETER_DBM_MIN + needle_t * (SMETER_DBM_MAX - SMETER_DBM_MIN);
        if dbm >= -53.0 {
            Color32::from_rgb(248, 113, 113)
        } else if dbm >= -85.0 {
            OK
        } else {
            Color32::from_rgb(248, 140, 140)
        }
    };

    if streaming {
        painter.add(Shape::convex_polygon(
            vec![
                tip + dir * 2.0,
                base_l,
                base_r,
            ],
            Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 35),
            Stroke::NONE,
        ));
        painter.line_segment(
            [center + dir * 4.0, tip],
            Stroke::new(3.0, Color32::from_black_alpha(90)),
        );
    }

    painter.line_segment([center + dir * 5.0, tip], Stroke::new(1.75, accent));
    painter.circle_filled(center, 5.0, Color32::from_rgb(32, 38, 50));
    painter.circle_stroke(center, 5.0, Stroke::new(1.0, Color32::from_rgb(70, 82, 104)));
    painter.circle_filled(center, 2.0, Color32::from_rgb(150, 160, 180));
    painter.circle_filled(center + dir * 1.5, 0.8, Color32::from_rgba_unmultiplied(255, 255, 255, 60));
}

fn paint_analog_s_meter_face(
    painter: &eframe::egui::Painter,
    face_rect: Rect,
    arc_rect: Rect,
    needle_t: f32,
    streaming: bool,
) {
    paint_meter_bezel(painter, face_rect);

    let center = Pos2::new(arc_rect.center().x, arc_rect.bottom() - 2.0);
    let radius = (arc_rect.width() * 0.47)
        .min(arc_rect.height() - 4.0)
        .clamp(42.0, 98.0);

    paint_arc_track(painter, center, radius);
    paint_scale_ticks(painter, center, radius);
    paint_meter_needle(painter, center, radius, needle_t, streaming);
}

fn paint_smeter_caption(
    painter: &eframe::egui::Painter,
    face_rect: Rect,
    caption_y: f32,
    reading: &str,
    sub: &str,
    streaming: bool,
) {
    let strip_top = caption_y - ANALOG_SMETER_CAPTION_H * 0.5;
    let strip = Rect::from_min_max(
        Pos2::new(face_rect.left(), strip_top),
        Pos2::new(face_rect.right(), face_rect.bottom()),
    );
    painter.rect_filled(strip, 0.0, Color32::from_rgba_unmultiplied(8, 10, 14, 200));
    painter.line_segment(
        [Pos2::new(strip.left(), strip.top()), Pos2::new(strip.right(), strip.top())],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(56, 189, 248, 50)),
    );
    painter.text(
        Pos2::new(face_rect.left() + 8.0, caption_y),
        Align2::LEFT_CENTER,
        "S-METER",
        FontId::proportional(8.5),
        Color32::from_rgba_unmultiplied(130, 145, 170, 220),
    );
    let value_color = if streaming { OK } else { MUTED };
    painter.text(
        Pos2::new(face_rect.right() - 8.0, caption_y),
        Align2::RIGHT_CENTER,
        format!("{reading}  {sub}"),
        FontId::monospace(10.5),
        value_color,
    );
}

/// Classic analog S-meter with needle and S-unit scale (RF front-end panel only).
pub fn show_analog_s_meter(ui: &mut Ui, p: &AnalogSmeterParams) -> eframe::egui::Response {
    let full_w = ui.available_width();
    let arc_h = (full_w * 0.44).clamp(ANALOG_SMETER_ARC_H_MIN, ANALOG_SMETER_ARC_H_MAX);
    let total_h = ANALOG_SMETER_ARC_TOP_PAD + arc_h + ANALOG_SMETER_CAPTION_H + 2.0;
    let (outer, resp) = ui.allocate_exact_size(Vec2::new(full_w, total_h), Sense::hover());
    let face = outer.shrink2(Vec2::new(ANALOG_SMETER_FACE_MARGIN, 2.0));
    let arc_rect = Rect::from_min_max(
        Pos2::new(face.left(), face.top() + ANALOG_SMETER_ARC_TOP_PAD),
        Pos2::new(face.right(), face.top() + ANALOG_SMETER_ARC_TOP_PAD + arc_h),
    );

    let target_t = if p.streaming {
        dbm_to_needle_t(p.dbm)
    } else {
        0.0
    };
    let needle_t = target_t;

    let painter = ui.painter_at(outer);
    paint_analog_s_meter_face(&painter, face, arc_rect, needle_t, p.streaming);

    let reading = if p.streaming {
        dbm_to_s_reading(p.dbm)
    } else {
        "—".to_string()
    };
    let sub = if p.streaming {
        format!("{:.0} dBm", p.dbm)
    } else {
        "offline".to_string()
    };
    let caption_y = face.top()
        + ANALOG_SMETER_ARC_TOP_PAD
        + arc_h
        + ANALOG_SMETER_CAPTION_H * 0.5;
    paint_smeter_caption(&painter, face, caption_y, &reading, &sub, p.streaming);

    if p.hw_rssi_dbm.is_some() {
        attach_rich_tooltip(
            &resp,
            Some("S-meter"),
            &[
                ("Pre-AGC IQ", ACCENT),
                (
                    "Needle follows IQ level before software AGC — should move when you change RF gain.",
                    MUTED,
                ),
                ("Kiwi HW", OK),
                (
                    "SND hardware readout may stay flat when Kiwi RF AGC is on; compare in the caption if shown.",
                    MUTED,
                ),
            ],
        );
    } else {
        attach_rich_tooltip(
            &resp,
            Some("S-meter"),
            &[
                ("Pre-AGC IQ", ACCENT),
                (
                    "Needle follows IQ level before software AGC — should move when you change RF gain.",
                    MUTED,
                ),
            ],
        );
    }
    resp
}

/// IF AGC bar: high = boosting weak RF, low = pulling back hot RF (classic dual-loop).
fn if_agc_fill(agc_gain: f32, agc_enabled: bool) -> f32 {
    if !agc_enabled {
        return 0.5;
    }
    let g = agc_gain.clamp(0.02, 64.0);
    (g.log10() / 64.0_f32.log10()).clamp(0.0, 1.0)
}

fn af_peak_fill(peak: f32) -> f32 {
    (peak / HALF_SCALE).clamp(0.0, 1.0)
}

pub struct DualAgcParams {
    pub rf_dbm: f32,
    pub hw_rssi_dbm: Option<f32>,
    pub agc_gain: f32,
    pub agc_enabled: bool,
    pub audio_peak: f32,
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
pub fn show_dual_agc_loop(ui: &mut Ui, p: &DualAgcParams) {
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
                        format!("{:.1}×", p.agc_gain),
                        if_agc_fill(p.agc_gain, true),
                        ACCENT,
                    )
                } else if live {
                    ("off".to_string(), 0.0, MUTED)
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
                    (format!("{:.2}", p.audio_peak), af_peak_fill(p.audio_peak))
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
    pub samples: &'a [f32],
    pub peak: f32,
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

pub fn show_af_tuning_panel(ui: &mut Ui, p: &AfScopeParams<'_>) {
    Frame::new()
        .fill(Color32::from_rgb(18, 22, 30))
        .stroke(Stroke::new(1.0, Color32::from_rgb(42, 52, 70)))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(egui::Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("AF scope").strong().color(ACCENT));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    status_badge(ui, p.hint);
                });
            });
            ui.add_space(4.0);
            show_af_scope(ui, p);
            ui.add_space(6.0);
            metric_row(ui, p);
        });
}

fn status_badge(ui: &mut Ui, hint: AudioLevelHint) {
    let accent = hint_accent(hint);
    let (rect, resp) = ui.allocate_exact_size(Vec2::new(52.0, 20.0), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect(
        rect,
        4.0,
        Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 24),
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 160)),
        StrokeKind::Inside,
    );
    painter.circle_filled(
        Pos2::new(rect.left() + 8.0, rect.center().y),
        3.0,
        accent,
    );
    painter.text(
        Pos2::new(rect.center().x + 4.0, rect.center().y),
        Align2::CENTER_CENTER,
        hint_short(hint),
        FontId::monospace(10.0),
        accent,
    );
    attach_rich_tooltip(&resp, Some("AF level"), hint_tip_lines(hint));
}

fn metric_chip(ui: &mut Ui, label: &str, value: &str, accent: Color32) {
    ui.vertical(|ui| {
        ui.label(egui::RichText::new(label).small().color(MUTED));
        ui.label(egui::RichText::new(value).monospace().color(accent));
    });
}

fn metric_row(ui: &mut Ui, p: &AfScopeParams<'_>) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 16.0;
        metric_chip(ui, "Peak", &format!("{:.3}", p.peak), TRACE);
        metric_chip(ui, "RMS", &format!("{:.3}", p.rms), MUTED);
        if p.agc_enabled {
            metric_chip(ui, "IQ AGC", &format!("{:.1}×", p.agc_gain), ACCENT);
            metric_chip(
                ui,
                "Env",
                &format!("{:.3}", p.agc_envelope),
                MUTED,
            );
            metric_chip(ui, "Tgt", &format!("{:.2}", p.agc_target), MUTED);
        }
        if p.streaming {
            let rf_dbm = rf_level_dbm(p.rssi_dbm, p.iq_rf_level);
            metric_chip(ui, "RF", &dbm_to_s_reading(rf_dbm), OK);
        }
        metric_chip(
            ui,
            "IQ buf",
            &format!("{:.0}%", p.iq_headroom * 100.0),
            MUTED,
        );
    });
}

pub fn show_af_scope(ui: &mut Ui, p: &AfScopeParams<'_>) {
    let h = 96.0;
    let (outer, _resp) =
        ui.allocate_exact_size(Vec2::new(ui.available_width().max(220.0), h), Sense::hover());
    let painter = ui.painter_at(outer);

    let plot = outer.shrink2(Vec2::new(28.0, 6.0));
    let meter_w = 14.0;
    let meter_rect = Rect::from_min_max(
        Pos2::new(outer.right() - meter_w - 4.0, plot.top()),
        Pos2::new(outer.right() - 4.0, plot.bottom()),
    );
    let plot = plot.with_max_x(meter_rect.left() - 6.0);

    paint_plot_background(&painter, plot);
    paint_grid(&painter, plot);
    paint_clip_zones(&painter, plot);
    paint_half_scale_guides(&painter, plot);
    paint_trace(&painter, plot, p);
    paint_peak_meter(&painter, meter_rect, p.peak, p.hint);
    paint_y_labels(&painter, outer, plot);
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

fn paint_trace(painter: &eframe::egui::Painter, rect: Rect, p: &AfScopeParams<'_>) {
    if p.samples.len() < 2 {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No audio",
            FontId::monospace(11.0),
            MUTED,
        );
        return;
    }

    let mid_y = rect.center().y;
    let half_h = rect.height() * 0.44;
    let n = p.samples.len();
    let dx = (rect.width() - 4.0) / (n - 1) as f32;

    let mut pts = Vec::with_capacity(n);
    let mut clipped = false;
    let clip_top = rect.top() + rect.height() * 0.08;
    let clip_bot = rect.bottom() - rect.height() * 0.08;

    for (i, &s) in p.samples.iter().enumerate() {
        let x = rect.left() + 2.0 + i as f32 * dx;
        let norm = s.clamp(-1.15, 1.15);
        let y = mid_y - norm * half_h;
        if y <= clip_top || y >= clip_bot {
            clipped = true;
        }
        pts.push(Pos2::new(x, y));
    }

    let trace = if clipped {
        Color32::from_rgb(251, 146, 120)
    } else {
        TRACE
    };
    let glow = if clipped {
        Color32::from_rgba_unmultiplied(251, 146, 120, 80)
    } else {
        TRACE_GLOW
    };

    painter.add(Shape::line(pts.clone(), Stroke::new(3.5, glow)));
    painter.add(Shape::line(pts, Stroke::new(1.35, trace)));
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
    fn rf_level_prefers_iq_when_available() {
        let iq_dbm = rf_level_dbm(None, 0.5);
        assert!((iq_dbm - (-121.0)).abs() < 0.5);
        assert_eq!(rf_level_dbm(Some(-80.0), 0.0), -80.0);
        assert_eq!(rf_level_dbm(None, 0.0), -127.0);
        let blended = rf_level_dbm(Some(-90.0), 0.5);
        assert!((blended - (-90.0)).abs() < 0.01 || blended > -90.0);
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
}
