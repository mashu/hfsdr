//! AF tuning oscilloscope and level badge.

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Frame, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};

use crate::theme::{attach_rich_tooltip, ACCENT, MUTED, OK, SURFACE, TRACE, TRACE_GLOW, WARN};

use super::level::{dbm_to_s_reading, rf_level_dbm, AudioLevelHint, HALF_SCALE};

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
