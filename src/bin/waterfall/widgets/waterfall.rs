use eframe::egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Stroke,
};

use crate::interaction::PlotFreqMapping;

use super::filter_band::{draw_filter_band, draw_notch_marker};
use super::freq_axis::draw_center_line;
use super::freq_axis::draw_freq_vertical_grid;
use super::PlotParams;

pub(crate) fn draw_waterfall_layer(painter: &Painter, rect: Rect, freq_map: PlotFreqMapping, p: &PlotParams) {
    let view_span = freq_map.view_span_hz;
    let pan = freq_map.pan_offset_hz;

    if let Some(tex) = p.waterfall_display {
        painter.image(
            tex.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );
    } else {
        painter.rect_filled(rect, 6.0, Color32::from_rgb(10, 12, 18));
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "Waiting for IQ data…",
            FontId::proportional(13.0),
            Color32::from_rgb(120, 130, 150),
        );
    }

    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_rgb(40, 48, 64)),
        eframe::egui::StrokeKind::Outside,
    );

    if p.filter_editable {
        draw_filter_band(
            painter,
            rect,
            view_span,
            pan,
            p.listen_center_hz,
            p.channel_half_hz,
            false,
        );
    }

    for notch in p.notches {
        draw_notch_marker(
            painter,
            rect,
            view_span,
            pan,
            notch.slot,
            notch.offset_hz.hz(),
            notch.display_half_hz,
            false,
        );
    }

    draw_center_line(
        painter,
        rect,
        view_span,
        pan,
        p.tune_preview_offset_hz,
        false,
    );
    draw_freq_vertical_grid(painter, rect, view_span, pan);
}
