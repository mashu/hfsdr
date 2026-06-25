use eframe::egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};

use crate::interaction::PlotAction;
use crate::theme::ACCENT;

use super::trace::draw_trace;

fn overview_offset_to_x(offset_hz: f64, rect: Rect, overview_span_hz: f32) -> f32 {
    let half = overview_span_hz as f64 / 2.0;
    let t = ((offset_hz + half) / overview_span_hz as f64).clamp(0.0, 1.0) as f32;
    rect.left() + t * rect.width()
}

pub(crate) fn draw_band_overview(
    ui: &mut Ui,
    painter: &Painter,
    plot_rect: Rect,
    sample_rate: f32,
    overview_span_hz: f32,
    view_span_hz: f32,
    pan_offset_hz: f64,
    overview_trace: &[f32],
    ref_db: f32,
    range_db: f32,
) -> Vec<PlotAction> {
    let mut actions = Vec::new();
    let size = Vec2::new(156.0, 46.0);
    let mini_rect = Rect::from_min_size(
        Pos2::new(plot_rect.right() - size.x - 8.0, plot_rect.top() + 8.0),
        size,
    );
    let response = ui.allocate_rect(mini_rect, Sense::click());

    painter.rect_filled(
        mini_rect,
        4.0,
        Color32::from_rgba_unmultiplied(10, 12, 18, 220),
    );
    painter.rect_stroke(
        mini_rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgb(55, 65, 85)),
        eframe::egui::StrokeKind::Inside,
    );
    painter.text(
        Pos2::new(mini_rect.left() + 6.0, mini_rect.top() + 2.0),
        Align2::LEFT_TOP,
        "Band overview",
        FontId::proportional(9.0),
        Color32::from_rgb(140, 150, 170),
    );

    let trace_rect = Rect::from_min_max(
        Pos2::new(mini_rect.left() + 4.0, mini_rect.top() + 14.0),
        Pos2::new(mini_rect.right() - 4.0, mini_rect.bottom() - 4.0),
    );

    let iq_half = sample_rate as f64 / 2.0;
    let iq_left = overview_offset_to_x(-iq_half, trace_rect, overview_span_hz);
    let iq_right = overview_offset_to_x(iq_half, trace_rect, overview_span_hz);
    let iq_rect = Rect::from_min_max(
        Pos2::new(iq_left, trace_rect.top()),
        Pos2::new(iq_right, trace_rect.bottom()),
    );
    if iq_rect.width() > 2.0 {
        painter.rect_filled(
            iq_rect,
            0.0,
            Color32::from_rgba_unmultiplied(56, 189, 248, 12),
        );
        draw_trace(painter, iq_rect, overview_trace, ref_db, range_db);
    }

    let view_left = pan_offset_hz - view_span_hz as f64 / 2.0;
    let view_right = pan_offset_hz + view_span_hz as f64 / 2.0;
    let vp_left = overview_offset_to_x(view_left, trace_rect, overview_span_hz);
    let vp_right = overview_offset_to_x(view_right, trace_rect, overview_span_hz);
    let viewport = Rect::from_min_max(
        Pos2::new(vp_left, trace_rect.top()),
        Pos2::new(vp_right, trace_rect.bottom()),
    );
    painter.rect_stroke(
        viewport,
        2.0,
        Stroke::new(1.5, ACCENT),
        eframe::egui::StrokeKind::Inside,
    );

    let rx_x = overview_offset_to_x(0.0, trace_rect, overview_span_hz);
    painter.line_segment(
        [
            Pos2::new(rx_x, trace_rect.top()),
            Pos2::new(rx_x, trace_rect.bottom()),
        ],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(248, 113, 113, 140)),
    );

    if response.clicked() {
        if let Some(pos) = response.interact_pointer_pos() {
            if trace_rect.contains(pos) {
                let t = ((pos.x - trace_rect.left()) / trace_rect.width()).clamp(0.0, 1.0) as f64;
                let half = overview_span_hz as f64 / 2.0;
                let pan = -half + t * overview_span_hz as f64;
                actions.push(PlotAction::SetViewPanHz(pan));
            }
        }
    }

    actions
}
