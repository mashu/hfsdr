//! Shared filter magnitude plot — zoomed span and painting.

use eframe::egui::{Color32, Painter, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, Vec2};

use hfsdr::{
    build_listen_filter_curves, channel_half_width_hz, filter_curve_span_hz, fir_cutoff_hz,
    CwChannelSettings, FilterCurve, FilterCurveRequest, OVERLAY_ATTEN_DB,
};

use crate::theme::{ACCENT, FILTER_EDGE, MUTED, SURFACE};

pub fn response_span_hz(settings: &CwChannelSettings, audio_rate: f32) -> f32 {
    let rate = audio_rate.max(1.0);
    let threshold = 10f32.powf(OVERLAY_ATTEN_DB / 20.0);
    let half = channel_half_width_hz(settings, rate, threshold);
    filter_curve_span_hz(settings.channel_bandwidth_hz(), half)
}

/// Inline channel-filter magnitude plot (0 dB at center, −60 dB floor).
pub fn paint_inline_response(
    ui: &mut Ui,
    settings: &CwChannelSettings,
    audio_rate: f32,
    height: f32,
) {
    let passband_hz = settings.channel_bandwidth_hz();
    let span_hz = response_span_hz(settings, audio_rate);
    let half_span = span_hz * 0.5;

    ui.horizontal(|ui| {
        ui.label(
            eframe::egui::RichText::new("Response")
                .small()
                .color(ACCENT),
        );
        ui.label(
            eframe::egui::RichText::new(format!(
                "±{half_span:.0} Hz · {passband_hz:.0} Hz BW"
            ))
            .small()
            .color(MUTED),
        );
    });
    ui.add_space(2.0);

    let (rect, _) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(200.0), height),
        Sense::hover(),
    );
    let curve = build_listen_filter_curves(&FilterCurveRequest {
        settings: settings.clone(),
        audio_rate,
        span_hz,
    });
    paint_magnitude_curve(
        &ui.painter_at(rect),
        rect,
        &curve,
        settings,
        audio_rate,
    );
    ui.add_space(4.0);
}

pub fn paint_magnitude_curve(
    painter: &Painter,
    rect: Rect,
    curve: &FilterCurve,
    settings: &CwChannelSettings,
    audio_rate: f32,
) {
    painter.rect_filled(rect, 4.0, SURFACE);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(40, 48, 64)),
        StrokeKind::Inside,
    );

    let margin_l = 28.0;
    let margin_r = 6.0;
    let margin_t = 4.0;
    let margin_b = 14.0;
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + margin_l, rect.top() + margin_t),
        Pos2::new(rect.right() - margin_r, rect.bottom() - margin_b),
    );
    if inner.width() < 8.0 || inner.height() < 8.0 {
        return;
    }

    let half_span = curve
        .offsets_hz
        .last()
        .copied()
        .unwrap_or(100.0)
        .abs()
        .max(10.0);
    let floor_db = -60.0f32;

    for step in 0..=3 {
        let db = -step as f32 * 20.0;
        let y = db_to_y(db, inner, floor_db);
        painter.line_segment(
            [Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(55, 65, 85, 80)),
        );
    }

    let threshold = 10f32.powf(OVERLAY_ATTEN_DB / 20.0);
    let half_hz = channel_half_width_hz(settings, audio_rate.max(1.0), threshold);
    let band_left = offset_to_x(-half_hz, half_span, inner);
    let band_right = offset_to_x(half_hz, half_span, inner);
    painter.rect_filled(
        Rect::from_min_max(
            Pos2::new(band_left, inner.top()),
            Pos2::new(band_right, inner.bottom()),
        ),
        0.0,
        Color32::from_rgba_unmultiplied(FILTER_EDGE.r(), FILTER_EDGE.g(), FILTER_EDGE.b(), 16),
    );
    for off in [-half_hz, half_hz] {
        let x = offset_to_x(off, half_span, inner);
        painter.line_segment(
            [Pos2::new(x, inner.top()), Pos2::new(x, inner.bottom())],
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(FILTER_EDGE.r(), FILTER_EDGE.g(), FILTER_EDGE.b(), 130),
            ),
        );
    }

    fill_under_curve(painter, inner, half_span, &curve.channel_only_db, &curve.offsets_hz, floor_db);
    draw_curve(
        painter,
        inner,
        half_span,
        &curve.channel_only_db,
        &curve.offsets_hz,
        floor_db,
        FILTER_EDGE,
        1.75,
    );

    let passband_hz = settings.channel_bandwidth_hz();
    let _ = fir_cutoff_hz(passband_hz, settings.passband_cutoff_frac);
    painter.text(
        Pos2::new(inner.left(), rect.bottom() - 2.0),
        eframe::egui::Align2::LEFT_BOTTOM,
        format!("−{half_span:.0}"),
        eframe::egui::FontId::monospace(8.0),
        MUTED,
    );
    painter.text(
        Pos2::new(inner.right(), rect.bottom() - 2.0),
        eframe::egui::Align2::RIGHT_BOTTOM,
        format!("+{half_span:.0}"),
        eframe::egui::FontId::monospace(8.0),
        MUTED,
    );
    painter.text(
        Pos2::new(inner.center().x, rect.bottom() - 2.0),
        eframe::egui::Align2::CENTER_BOTTOM,
        "Hz",
        eframe::egui::FontId::proportional(8.0),
        MUTED,
    );
}

fn fill_under_curve(
    painter: &Painter,
    inner: Rect,
    half_span: f32,
    db: &[f32],
    offsets: &[f32],
    floor_db: f32,
) {
    if db.len() < 2 || db.len() != offsets.len() {
        return;
    }
    let bottom = inner.bottom();
    let fill = Color32::from_rgba_unmultiplied(FILTER_EDGE.r(), FILTER_EDGE.g(), FILTER_EDGE.b(), 28);
    for i in 0..db.len() - 1 {
        let x0 = offset_to_x(offsets[i], half_span, inner);
        let x1 = offset_to_x(offsets[i + 1], half_span, inner);
        let y0 = db_to_y(db[i], inner, floor_db);
        let y1 = db_to_y(db[i + 1], inner, floor_db);
        let p0 = Pos2::new(x0, y0);
        let p1 = Pos2::new(x1, y1);
        let b0 = Pos2::new(x0, bottom);
        let b1 = Pos2::new(x1, bottom);
        painter.add(eframe::egui::Shape::convex_polygon(vec![p0, p1, b1], fill, Stroke::NONE));
        painter.add(eframe::egui::Shape::convex_polygon(vec![p0, b1, b0], fill, Stroke::NONE));
    }
}

fn draw_curve(
    painter: &Painter,
    inner: Rect,
    half_span: f32,
    db: &[f32],
    offsets: &[f32],
    floor_db: f32,
    color: Color32,
    width: f32,
) {
    if db.len() < 2 || db.len() != offsets.len() {
        return;
    }
    let mut prev: Option<Pos2> = None;
    for (&off, &val) in offsets.iter().zip(db.iter()) {
        let pt = Pos2::new(
            offset_to_x(off, half_span, inner),
            db_to_y(val, inner, floor_db),
        );
        if let Some(p0) = prev {
            painter.line_segment([p0, pt], Stroke::new(width, color));
        }
        prev = Some(pt);
    }
}

fn offset_to_x(offset_hz: f32, half_span: f32, inner: Rect) -> f32 {
    let t = ((offset_hz + half_span) / (2.0 * half_span)).clamp(0.0, 1.0);
    inner.left() + inner.width() * t
}

fn db_to_y(db: f32, inner: Rect, floor_db: f32) -> f32 {
    let t = ((0.0 - db) / (0.0 - floor_db).max(1.0)).clamp(0.0, 1.0);
    inner.top() + inner.height() * t
}
