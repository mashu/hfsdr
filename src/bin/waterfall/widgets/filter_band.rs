use eframe::egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Stroke, Vec2,
};

use crate::interaction::{edge_grab_px, filter_edges, offset_hz_to_x};
use crate::theme::{FILTER_EDGE, NOTCH_LINE};

pub(crate) fn draw_filter_band(
    painter: &Painter,
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
    listen_center_hz: f64,
    passband_hz: f32,
    fill: bool,
) {
    let (mut left, mut right) =
        filter_edges(rect, view_span_hz, pan_offset_hz, listen_center_hz, passband_hz);
    left = left.clamp(rect.left(), rect.right());
    right = right.clamp(rect.left(), rect.right());
    if right <= left {
        return;
    }

    if fill {
        let band = Rect::from_min_max(Pos2::new(left, rect.top()), Pos2::new(right, rect.bottom()));
        painter.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(56, 189, 248, 28));
    }

    painter.line_segment(
        [Pos2::new(left, rect.top()), Pos2::new(left, rect.bottom())],
        Stroke::new(1.5, FILTER_EDGE),
    );
    painter.line_segment(
        [Pos2::new(right, rect.top()), Pos2::new(right, rect.bottom())],
        Stroke::new(1.5, FILTER_EDGE),
    );

    if fill {
        let grab_w = edge_grab_px();
        for x in [left, right] {
            let handle = Rect::from_center_size(
                Pos2::new(x, rect.center().y),
                Vec2::new(grab_w, rect.height() * 0.35),
            );
            painter.rect_filled(handle, 3.0, Color32::from_rgba_unmultiplied(125, 211, 252, 60));
        }
        painter.text(
            Pos2::new((left + right) * 0.5, rect.bottom() - 4.0),
            Align2::CENTER_BOTTOM,
            "drag band = RIT · edges = BW",
            FontId::proportional(9.0),
            Color32::from_rgba_unmultiplied(125, 211, 252, 140),
        );
    }
}

pub(crate) fn draw_notch_marker(
    painter: &Painter,
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
    slot: usize,
    notch_offset_hz: f32,
    notch_width_hz: f32,
    show_handles: bool,
) {
    let half = notch_width_hz as f64 / 2.0;
    let center = notch_offset_hz as f64;
    let left = offset_hz_to_x(center - half, rect, view_span_hz, pan_offset_hz);
    let right = offset_hz_to_x(center + half, rect, view_span_hz, pan_offset_hz);
    let center_x = offset_hz_to_x(center, rect, view_span_hz, pan_offset_hz);

    let band = Rect::from_min_max(
        Pos2::new(left.clamp(rect.left(), rect.right()), rect.top()),
        Pos2::new(right.clamp(rect.left(), rect.right()), rect.bottom()),
    );
    if band.width() > 1.0 {
        painter.rect_filled(band, 0.0, Color32::from_rgba_unmultiplied(192, 132, 252, 22));
    }

    let stroke = Stroke::new(1.5, NOTCH_LINE);
    painter.line_segment(
        [Pos2::new(center_x, rect.top()), Pos2::new(center_x, rect.bottom())],
        stroke,
    );
    for x in [left, right] {
        if rect.left() <= x && x <= rect.right() {
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(192, 132, 252, 160)),
            );
            if show_handles {
                let handle = Rect::from_center_size(
                    Pos2::new(x, rect.center().y),
                    Vec2::new(edge_grab_px(), rect.height() * 0.28),
                );
                painter.rect_filled(
                    handle,
                    3.0,
                    Color32::from_rgba_unmultiplied(192, 132, 252, 55),
                );
            }
        }
    }

    painter.text(
        Pos2::new(center_x, rect.top() + 14.0),
        Align2::CENTER_TOP,
        format!("#{}", slot + 1),
        FontId::proportional(10.0),
        NOTCH_LINE,
    );
}
