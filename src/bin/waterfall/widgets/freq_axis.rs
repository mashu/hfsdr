use eframe::egui::{
    Align2, Color32, FontId, Painter, Pos2, Rect, Sense, Stroke, Ui, Vec2,
};

use crate::interaction::{
    format_freq_hz, format_offset_label, nice_freq_step_hz, offset_hz_to_x, x_to_offset_hz,
    center_grab_px, PlotFreqMapping,
};
use crate::theme::{CENTER_LINE, GRID};

const FREQ_AXIS_HEIGHT: f32 = 18.0;

/// MHz/kHz strip between the scope and waterfall (same width / freq mapping as both).
pub fn show_freq_axis_bar(
    ui: &mut Ui,
    plot_width: f32,
    view_span_hz: f32,
    pan_offset_hz: f64,
    center_freq_hz: f64,
    hover_offset_hz: &mut Option<f64>,
) -> Rect {
    let (response, painter) = ui.allocate_painter(
        Vec2::new(plot_width, FREQ_AXIS_HEIGHT),
        Sense::empty(),
    );
    let rect = response.rect;
    draw_freq_axis_bar(
        &painter,
        rect,
        view_span_hz,
        pan_offset_hz,
        center_freq_hz,
    );
    if let Some(offset) = *hover_offset_hz {
        let map = PlotFreqMapping::new(view_span_hz, pan_offset_hz, view_span_hz);
        let x = map.offset_to_x(offset, rect);
        if x >= rect.left() && x <= rect.right() {
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 200, 255, 90)),
            );
        }
    }
    if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
        if rect.contains(pos) {
            *hover_offset_hz =
                Some(x_to_offset_hz(pos.x, rect, view_span_hz, pan_offset_hz));
        }
    }
    let _ = response;
    rect
}

/// Vertical frequency grid lines (labels are on the shared axis bar).
pub(crate) fn draw_freq_vertical_grid(
    painter: &Painter,
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
) {
    let step = nice_freq_step_hz(view_span_hz) as f64;
    let left_hz = pan_offset_hz - view_span_hz as f64 / 2.0;
    let right_hz = pan_offset_hz + view_span_hz as f64 / 2.0;
    let mut tick_hz = (left_hz / step).ceil() * step;

    while tick_hz <= right_hz + step * 0.01 {
        let x = offset_hz_to_x(tick_hz, rect, view_span_hz, pan_offset_hz);
        if x >= rect.left() && x <= rect.right() {
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.0, GRID),
            );
        }
        tick_hz += step;
    }
}

/// MHz/kHz labels between the scope and waterfall.
fn draw_freq_axis_bar(
    painter: &Painter,
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
    center_freq_hz: f64,
) {
    painter.rect_filled(rect, 0.0, Color32::from_rgb(10, 12, 18));
    painter.line_segment(
        [Pos2::new(rect.left(), rect.top()), Pos2::new(rect.right(), rect.top())],
        Stroke::new(1.0, Color32::from_rgb(40, 48, 64)),
    );
    painter.line_segment(
        [Pos2::new(rect.left(), rect.bottom()), Pos2::new(rect.right(), rect.bottom())],
        Stroke::new(1.0, Color32::from_rgb(40, 48, 64)),
    );

    let step = nice_freq_step_hz(view_span_hz) as f64;
    let left_hz = pan_offset_hz - view_span_hz as f64 / 2.0;
    let right_hz = pan_offset_hz + view_span_hz as f64 / 2.0;
    let mut tick_hz = (left_hz / step).ceil() * step;
    let text_color = Color32::from_rgb(140, 150, 170);
    let unit_color = Color32::from_rgb(100, 110, 130);

    while tick_hz <= right_hz + step * 0.01 {
        let x = offset_hz_to_x(tick_hz, rect, view_span_hz, pan_offset_hz);
        if x >= rect.left() && x <= rect.right() {
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.top() + 4.0)],
                Stroke::new(1.0, GRID),
            );
            let abs_hz = center_freq_hz + tick_hz;
            let label = if center_freq_hz > 0.0 {
                format_freq_hz(abs_hz)
            } else {
                format_offset_label(tick_hz)
            };
            painter.text(
                Pos2::new(x, rect.bottom() - 2.0),
                Align2::CENTER_BOTTOM,
                label,
                FontId::proportional(10.0),
                text_color,
            );
        }
        tick_hz += step;
    }

    if center_freq_hz > 1_000_000.0 {
        painter.text(
            Pos2::new(rect.right() - 4.0, rect.bottom() - 2.0),
            Align2::RIGHT_BOTTOM,
            "MHz",
            FontId::proportional(9.0),
            unit_color,
        );
    }

    let rx_x = offset_hz_to_x(0.0, rect, view_span_hz, pan_offset_hz);
    if rx_x > rect.left() + 8.0 && rx_x < rect.right() - 8.0 {
        painter.line_segment(
            [
                Pos2::new(rx_x, rect.top()),
                Pos2::new(rx_x, rect.top() + 6.0),
            ],
            Stroke::new(1.5, Color32::from_rgba_unmultiplied(248, 113, 113, 160)),
        );
        if center_freq_hz > 0.0 {
            painter.text(
                Pos2::new(rx_x, rect.top() + 1.0),
                Align2::CENTER_TOP,
                "RX",
                FontId::proportional(8.0),
                Color32::from_rgba_unmultiplied(248, 113, 113, 200),
            );
        }
    }
}

pub(crate) fn draw_center_line(
    painter: &Painter,
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
    preview_offset_hz: f64,
    show_handle: bool,
) {
    let center_x = offset_hz_to_x(preview_offset_hz, rect, view_span_hz, pan_offset_hz);
    let stroke = if preview_offset_hz.abs() > f64::EPSILON {
        Stroke::new(2.0, CENTER_LINE)
    } else {
        Stroke::new(1.5, CENTER_LINE)
    };
    painter.line_segment(
        [Pos2::new(center_x, rect.top()), Pos2::new(center_x, rect.bottom())],
        stroke,
    );

    if show_handle {
        let grab = center_grab_px();
        let handle = Rect::from_center_size(
            Pos2::new(center_x, rect.center().y),
            Vec2::new(grab, rect.height() * 0.42),
        );
        painter.rect_filled(handle, 4.0, Color32::from_rgba_unmultiplied(248, 113, 113, 45));
        painter.rect_stroke(
            handle,
            4.0,
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(248, 113, 113, 140)),
            eframe::egui::StrokeKind::Inside,
        );
    }
}
