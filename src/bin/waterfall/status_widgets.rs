//! Compact status-bar indicators.

use eframe::egui::{self, Color32, FontId, Response, Sense, Stroke, Ui, Vec2};

use crate::theme::{ACCENT, MUTED, OK, WARN};

/// IQ ring buffer — framed, labeled control; click opens record / playback panel.
pub fn iq_buffer_control(ui: &mut Ui, fill: f32, buffer_secs: f32, panel_open: bool) -> Response {
    let fill = fill.clamp(0.0, 1.0);
    let size = Vec2::new(92.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = response.hovered();
    let painter = ui.painter_at(rect);
    let rounding = 4.0;

    let accent = if panel_open { ACCENT } else if hovered { ACCENT } else { MUTED };
    let border = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), if hovered || panel_open { 200 } else { 110 });
    let bg = if hovered || panel_open {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 28)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    painter.rect(
        rect,
        rounding,
        bg,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );

    let inner = rect.shrink2(Vec2::new(6.0, 4.0));
    let label_w = 14.0;
    let chevron_w = 10.0;
    let bar_rect = egui::Rect::from_min_max(
        egui::pos2(inner.left() + label_w, inner.center().y - 4.5),
        egui::pos2(inner.right() - chevron_w, inner.center().y + 4.5),
    );

    painter.text(
        egui::pos2(inner.left(), inner.center().y),
        egui::Align2::LEFT_CENTER,
        "IQ",
        FontId::proportional(11.0),
        if hovered || panel_open { ACCENT } else { MUTED },
    );

    painter.rect_filled(bar_rect, 2.0, Color32::from_rgb(18, 22, 30));
    if fill > 0.02 {
        let mut fill_rect = bar_rect;
        fill_rect.set_width(bar_rect.width() * fill);
        painter.rect_filled(fill_rect, 2.0, buffer_color(fill));
    }

    painter.text(
        egui::pos2(inner.right(), inner.center().y),
        egui::Align2::RIGHT_CENTER,
        "▾",
        FontId::proportional(10.0),
        accent,
    );

    response.on_hover_text(format!(
        "IQ utilization {:.0}%\n\
         ~{:.2}s queued in ring · pump vs expected rate\n\
         High = samples flowing and consumed · Low / empty = stall or underrun\n\
         Click to open record / playback",
        fill * 100.0,
        buffer_secs
    ))
}

/// One-click record toggle — off starts a new timestamped capture, on stops.
pub fn iq_record_toggle(
    ui: &mut Ui,
    recording: bool,
    can_record: bool,
    elapsed_secs: f32,
) -> Response {
    let label = if recording {
        format!("REC {elapsed_secs:.0}s")
    } else {
        "REC".to_string()
    };
    let color = if recording { WARN } else { MUTED };
    let size = Vec2::new(if recording { 72.0 } else { 36.0 }, 20.0);
    let enabled = recording || can_record;
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let hovered = response.hovered();
    let painter = ui.painter_at(rect);
    let rounding = 4.0;
    let stroke_color = if !enabled {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 60)
    } else if recording {
        Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), if hovered { 220 } else { 160 })
    } else if hovered {
        Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), 180)
    } else {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 110)
    };
    let bg = if recording {
        Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), if hovered { 50 } else { 36 })
    } else if hovered && can_record {
        Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), 24)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    painter.rect(rect, rounding, bg, Stroke::new(1.0, stroke_color), egui::StrokeKind::Inside);

    let text_color = if !enabled {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 120)
    } else {
        color
    };
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        label,
        FontId::proportional(11.0),
        text_color,
    );

    if recording {
        response.on_hover_text(
            "Stop IQ recording\n\
             Toggle off then on again to start a new timestamped .hiq.gz",
        )
    } else if can_record {
        response.on_hover_text(
            "Start IQ recording\n\
             Saves gzip .hiq.gz with timestamp · toggle off/on for next file",
        )
    } else {
        response.on_hover_text("Connect (or stream) to record IQ")
    }
}

fn buffer_color(fill: f32) -> Color32 {
    let low = Color32::from_rgb(248, 113, 113);
    let mid = WARN;
    let high = OK;
    if fill < 0.5 {
        lerp_color(low, mid, fill / 0.5)
    } else {
        lerp_color(mid, high, (fill - 0.5) / 0.5)
    }
}

fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let t = t.clamp(0.0, 1.0);
    Color32::from_rgb(
        (a.r() as f32 + (b.r() as f32 - a.r() as f32) * t) as u8,
        (a.g() as f32 + (b.g() as f32 - a.g() as f32) * t) as u8,
        (a.b() as f32 + (b.b() as f32 - a.b() as f32) * t) as u8,
    )
}
