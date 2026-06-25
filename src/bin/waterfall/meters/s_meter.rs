//! Analog S-meter face and status-bar RF readout.

use eframe::egui::{
    self, Align2, Color32, FontId, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, Vec2,
};

use crate::theme::{attach_rich_tooltip, ACCENT, MUTED, OK};

use super::level::{
    dbm_to_needle_t, dbm_to_s_reading, needle_angle, SMETER_DBM_MAX, SMETER_DBM_MIN,
};

const ANALOG_SMETER_FACE_MARGIN: f32 = 2.0;
const ANALOG_SMETER_ARC_TOP_PAD: f32 = 4.0;
const ANALOG_SMETER_ARC_H_MIN: f32 = 82.0;
const ANALOG_SMETER_ARC_H_MAX: f32 = 112.0;
const ANALOG_SMETER_CAPTION_H: f32 = 16.0;
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
pub fn show_analog_s_meter(
    ui: &mut Ui,
    p: &AnalogSmeterParams,
    needle_t: f32,
) -> eframe::egui::Response {
    let full_w = ui.available_width();
    let arc_h = (full_w * 0.44).clamp(ANALOG_SMETER_ARC_H_MIN, ANALOG_SMETER_ARC_H_MAX);
    let total_h = ANALOG_SMETER_ARC_TOP_PAD + arc_h + ANALOG_SMETER_CAPTION_H + 2.0;
    let (outer, resp) = ui.allocate_exact_size(Vec2::new(full_w, total_h), Sense::hover());
    let face = outer.shrink2(Vec2::new(ANALOG_SMETER_FACE_MARGIN, 2.0));
    let arc_rect = Rect::from_min_max(
        Pos2::new(face.left(), face.top() + ANALOG_SMETER_ARC_TOP_PAD),
        Pos2::new(face.right(), face.top() + ANALOG_SMETER_ARC_TOP_PAD + arc_h),
    );

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::meters::level::{dbm_to_needle_t, needle_angle, SMETER_DBM_MAX, SMETER_DBM_MIN};

    #[test]
    fn scale_marks_cover_s9_and_plus() {
        let marks = smeter_scale_marks();
        assert!(marks.iter().any(|(l, _)| *l == "S9"));
        assert!(marks.iter().any(|(l, _)| *l == "+40"));
    }

    #[test]
    fn arc_point_lies_on_sem_circle() {
        let center = Pos2::new(100.0, 200.0);
        let p0 = arc_point(center, 50.0, 0.0);
        let p1 = arc_point(center, 50.0, 1.0);
        let r0 = ((p0.x - center.x).powi(2) + (p0.y - center.y).powi(2)).sqrt();
        let r1 = ((p1.x - center.x).powi(2) + (p1.y - center.y).powi(2)).sqrt();
        assert!((r0 - 50.0).abs() < 0.01);
        assert!((r1 - 50.0).abs() < 0.01);
        assert!(p0.y <= center.y);
        assert!(p1.y <= center.y);
    }

    #[test]
    fn arc_zone_color_warms_with_level() {
        let quiet = arc_zone_color(dbm_to_needle_t(-100.0));
        let hot = arc_zone_color(dbm_to_needle_t(-40.0));
        assert_ne!(quiet, hot);
        let dbm = SMETER_DBM_MIN + 0.5 * (SMETER_DBM_MAX - SMETER_DBM_MIN);
        let mid = arc_zone_color(dbm_to_needle_t(dbm));
        assert_ne!(mid, hot);
    }

    #[test]
    fn needle_angle_endpoints() {
        assert!((needle_angle(0.0) - std::f32::consts::PI).abs() < 1e-5);
        assert!(needle_angle(1.0).abs() < 1e-5);
    }
}
