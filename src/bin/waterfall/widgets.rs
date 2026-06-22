//! Interactive spectrum + waterfall rendering.

use eframe::egui::{
    Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Response, Sense, Shape, Stroke, Ui, Vec2,
};
use hfsdr::compose_panadapter_row;

use crate::interaction::{
    center_grab_px, edge_grab_px, filter_edges, format_freq_hz, format_offset_label,
    nice_freq_step_hz, offset_hz_to_x, NotchMarker, PlotAction, PlotInteraction, PlotViewState,
};

use crate::smooth::spatial_smooth;
use crate::theme::{ACCENT, CENTER_LINE, FILTER_EDGE, GRID, NOTCH_LINE, OK, TRACE, TRACE_GLOW, WARN};

/// A decoded-signal label floated above its spectral peak.
#[derive(Clone, Debug)]
pub struct SpotLabel {
    pub offset_hz: f32,
    pub text: String,
    pub cq: bool,
    pub snr_db: f32,
}

/// Shared rendering/interaction parameters for the RF plots.
///
/// Bundling these keeps the widget API small and is the natural seam for the
/// future node-graph compositor (one struct describes what a plot shows).
const FREQ_AXIS_HEIGHT: f32 = 18.0;
pub const SCOPE_HEIGHT: f32 = 200.0;

/// MHz/kHz strip between the scope and waterfall (same width / freq mapping as both).
pub fn show_freq_axis_bar(
    ui: &mut Ui,
    plot_width: f32,
    view_span_hz: f32,
    pan_offset_hz: f64,
    center_freq_hz: f64,
    hover_offset_hz: &mut Option<f64>,
) {
    let (response, painter) = ui.allocate_painter(
        Vec2::new(plot_width, FREQ_AXIS_HEIGHT),
        Sense::hover(),
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
        let x = offset_hz_to_x(offset, rect, view_span_hz, pan_offset_hz);
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
                Some(crate::interaction::x_to_offset_hz(pos.x, rect, view_span_hz, pan_offset_hz));
        }
    }
    let _ = response;
}

pub struct PlotParams<'a> {
    /// Visible panadapter width at zoom 1.0 (Kiwi IQ passband; equals IQ rate on wideband SDRs).
    pub view_bandwidth_hz: f32,
    /// Maximum zoom-out factor (CW band overview / full_span); 1.0 on wideband SDRs.
    pub max_zoom: f32,
    /// Visible span and pan aligned with composed spectrum/waterfall rows.
    pub view_span_hz: f32,
    pub pan_offset_hz: f64,
    /// Tuned carrier (Hz) — used for absolute MHz/kHz axis labels.
    pub center_freq_hz: f64,
    pub passband_hz: f32,
    pub passband_min_hz: f32,
    pub passband_max_hz: f32,
    pub filter_editable: bool,
    pub listen_center_hz: f64,
    pub tune_preview_offset_hz: f64,
    pub notches: &'a [NotchMarker],
    pub labels: &'a [SpotLabel],
    pub trace: &'a [f32],
    /// Full-span trace for the optional band overview minimap (IQ passband).
    pub overview_trace: &'a [f32],
    pub overview_span_hz: f32,
    pub show_overview: bool,
    pub ref_db: f32,
    pub range_db: f32,
    /// Scope trace height (frequency axis is a separate row below).
    pub height: f32,
    /// Shared plot column width — scope, axis, and waterfall must match.
    pub plot_width: f32,
}

pub struct SpectrumWidget;

impl SpectrumWidget {
    pub fn new() -> Self {
        Self
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        interaction: &mut PlotInteraction,
        view: &mut PlotViewState,
        p: &PlotParams,
        hover_out: &mut Option<f64>,
    ) -> (Response, Vec<PlotAction>) {
        let full_span = p.view_bandwidth_hz.max(1.0);
        let max_zoom = p.max_zoom.max(1.0);
        let view_span = p.view_span_hz;
        let pan = p.pan_offset_hz;
        let plot_w = p.plot_width.max(1.0);
        let (response, painter) = ui.allocate_painter(
            Vec2::new(plot_w, p.height),
            Sense::click_and_drag(),
        );
        let rect = response.rect;
        draw_plot_background(&painter, rect);

        if p.filter_editable {
            draw_filter_band(
                &painter,
                rect,
                view_span,
                pan,
                p.listen_center_hz,
                p.passband_hz,
                true,
            );
        }

        for notch in p.notches {
            draw_notch_marker(
                &painter,
                rect,
                view_span,
                pan,
                notch.slot,
                notch.offset_hz,
                notch.width_hz,
                true,
            );
        }

        draw_db_scale(&painter, rect, p.ref_db, p.range_db);
        draw_freq_vertical_grid(&painter, rect, view_span, pan);
        draw_trace(
            &painter,
            rect,
            p.trace,
            p.ref_db,
            p.range_db,
        );

        draw_center_line(&painter, rect, view_span, pan, p.tune_preview_offset_hz, true);

        if let Some(offset) = *hover_out {
            let x = offset_hz_to_x(offset, rect, view_span, pan);
            painter.line_segment(
                [Pos2::new(x, rect.top()), Pos2::new(x, rect.bottom())],
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(200, 200, 255, 100)),
            );
            painter.text(
                Pos2::new(x, rect.top() + 4.0),
                Align2::CENTER_TOP,
                format_offset_label(offset),
                FontId::proportional(11.0),
                ACCENT,
            );
        }

        draw_spot_labels(&painter, rect, view_span, pan, p.labels);

        let mut actions = interaction.handle(
            ui,
            rect,
            &response,
            view,
            full_span,
            max_zoom,
            view_span,
            pan,
            p.passband_hz,
            p.passband_min_hz,
            p.passband_max_hz,
            p.filter_editable,
            p.listen_center_hz,
            p.tune_preview_offset_hz,
            p.notches,
        );

        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            if rect.contains(pos) {
                *hover_out =
                    Some(crate::interaction::x_to_offset_hz(pos.x, rect, view_span, pan));
            }
        }

        if p.show_overview && !p.overview_trace.is_empty() {
            actions.extend(draw_band_overview(
                ui,
                &painter,
                rect,
                full_span,
                p.overview_span_hz,
                view_span,
                pan,
                p.overview_trace,
                p.ref_db,
                p.range_db,
            ));
        }

        (response, actions)
    }
}

pub struct WaterfallWidget;

impl WaterfallWidget {
    pub fn new() -> Self {
        Self
    }

    pub fn show(
        &mut self,
        ui: &mut Ui,
        interaction: &mut PlotInteraction,
        view: &mut PlotViewState,
        texture: &eframe::egui::TextureHandle,
        p: &PlotParams,
        hover_out: &mut Option<f64>,
    ) -> Vec<PlotAction> {
        let full_span = p.view_bandwidth_hz.max(1.0);
        let max_zoom = p.max_zoom.max(1.0);
        let view_span = p.view_span_hz;
        let pan = p.pan_offset_hz;
        let plot_w = p.plot_width.max(1.0);
        let size = Vec2::new(plot_w, ui.available_height());
        let (response, painter) = ui.allocate_painter(size, Sense::click_and_drag());
        let rect = response.rect;

        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(Pos2::ZERO, Pos2::new(1.0, 1.0)),
            Color32::WHITE,
        );

        painter.rect_stroke(
            rect,
            6.0,
            Stroke::new(1.0, Color32::from_rgb(40, 48, 64)),
            eframe::egui::StrokeKind::Outside,
        );

        if p.filter_editable {
            draw_filter_band(&painter, rect, view_span, pan, p.listen_center_hz, p.passband_hz, false);
        }

        for notch in p.notches {
            draw_notch_marker(
                &painter,
                rect,
                view_span,
                pan,
                notch.slot,
                notch.offset_hz,
                notch.width_hz,
                false,
            );
        }

        draw_center_line(&painter, rect, view_span, pan, p.tune_preview_offset_hz, false);

        draw_freq_vertical_grid(&painter, rect, view_span, pan);

        let actions = interaction.handle(
            ui,
            rect,
            &response,
            view,
            full_span,
            max_zoom,
            view_span,
            pan,
            p.passband_hz,
            p.passband_min_hz,
            p.passband_max_hz,
            p.filter_editable,
            p.listen_center_hz,
            p.tune_preview_offset_hz,
            p.notches,
        );

        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            if rect.contains(pos) {
                *hover_out =
                    Some(crate::interaction::x_to_offset_hz(pos.x, rect, view_span, pan));
            }
        }

        actions
    }
}

fn draw_center_line(
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

fn draw_plot_background(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 6.0, Color32::from_rgb(10, 12, 18));
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_rgb(40, 48, 64)),
        eframe::egui::StrokeKind::Outside,
    );
}

fn draw_db_scale(painter: &Painter, rect: Rect, ref_db: f32, range_db: f32) {
    let floor = ref_db - range_db;
    let label_x = rect.left() + 4.0;
    let tick_color = Color32::from_rgba_unmultiplied(120, 130, 150, 180);
    let text_color = Color32::from_rgb(130, 140, 160);
    for frac in [0.0_f32, 0.25, 0.5, 0.75, 1.0] {
        let y = rect.bottom() - rect.height() * frac;
        painter.line_segment(
            [Pos2::new(rect.left(), y), Pos2::new(rect.right(), y)],
            Stroke::new(1.0, GRID),
        );
        let db = floor + range_db * frac;
        painter.text(
            Pos2::new(label_x, y - 1.0),
            if frac >= 0.99 {
                Align2::LEFT_BOTTOM
            } else if frac <= 0.01 {
                Align2::LEFT_TOP
            } else {
                Align2::LEFT_CENTER
            },
            format!("{:.0}", db),
            FontId::proportional(10.0),
            text_color,
        );
    }
    painter.text(
        Pos2::new(label_x, rect.top() + 2.0),
        Align2::LEFT_TOP,
        "dB",
        FontId::proportional(9.0),
        tick_color,
    );
}

/// Vertical frequency grid lines (labels are on the shared axis bar).
fn draw_freq_vertical_grid(
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

fn draw_filter_band(
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

fn draw_spot_labels(
    painter: &Painter,
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
    labels: &[SpotLabel],
) {
    const CHAR_W: f32 = 6.5;
    const ROW_H: f32 = 13.0;
    const MIN_GAP: f32 = 3.0;
    const MAX_ROWS: u8 = 3;

    let mut placed: Vec<(f32, f32, u8)> = Vec::new();
    let mut sorted: Vec<&SpotLabel> = labels.iter().collect();
    sorted.sort_by(|a, b| {
        b.snr_db
            .total_cmp(&a.snr_db)
            .then_with(|| a.offset_hz.total_cmp(&b.offset_hz))
    });

    for label in sorted {
        let x = offset_hz_to_x(label.offset_hz as f64, rect, view_span_hz, pan_offset_hz);
        if x < rect.left() || x > rect.right() {
            continue;
        }
        let half_w = label.text.len() as f32 * CHAR_W * 0.5;
        let left = x - half_w;
        let right = x + half_w;

        let mut row = 0u8;
        'rows: while row < MAX_ROWS {
            let overlaps = placed.iter().any(|(pl, pr, r)| {
                *r == row && left < *pr + MIN_GAP && right > *pl - MIN_GAP
            });
            if !overlaps {
                break 'rows;
            }
            row += 1;
        }
        if row >= MAX_ROWS {
            continue;
        }
        placed.push((left, right, row));

        let y = rect.top() + 11.0 + row as f32 * ROW_H;
        let color = if label.cq { WARN } else { OK };
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.top() + 8.0 + row as f32 * ROW_H)],
            Stroke::new(1.5, color),
        );
        painter.text(
            Pos2::new(x, y),
            Align2::CENTER_TOP,
            &label.text,
            FontId::proportional(11.0),
            color,
        );
    }
}

fn draw_notch_marker(
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

fn draw_trace(painter: &Painter, rect: Rect, trace: &[f32], ref_db: f32, range_db: f32) {
    let floor = ref_db - range_db;
    let n = trace.len();
    if n < 2 || rect.width() < 1.0 || rect.height() < 1.0 {
        return;
    }

    // Composed rows are uniform in frequency across the view — index maps linearly
    // to X, matching draw_freq_grid / x_to_offset_hz on the same plot rect.
    let max_pts = ((rect.width() * 1.5).round() as usize).clamp(2, 2048);
    let mut line_pts = Vec::with_capacity(max_pts);
    if n <= max_pts {
        for (i, &db) in trace.iter().enumerate() {
            let x = rect.left() + rect.width() * i as f32 / (n as f32 - 1.0);
            let t = ((db - floor) / range_db).clamp(0.0, 1.0);
            let y = rect.bottom() - rect.height() * t;
            line_pts.push(Pos2::new(x, y));
        }
    } else {
        for out_i in 0..max_pts {
            let start = out_i * n / max_pts;
            let end = ((out_i + 1) * n / max_pts).max(start + 1).min(n);
            let peak = trace[start..end]
                .iter()
                .copied()
                .fold(f32::NEG_INFINITY, f32::max);
            let x = rect.left() + rect.width() * out_i as f32 / (max_pts as f32 - 1.0);
            let t = ((peak - floor) / range_db).clamp(0.0, 1.0);
            let y = rect.bottom() - rect.height() * t;
            line_pts.push(Pos2::new(x, y));
        }
    }

    fill_under_trace(painter, rect, &line_pts);
    painter.add(Shape::line(line_pts.clone(), Stroke::new(2.5, TRACE_GLOW)));
    painter.add(Shape::line(line_pts, Stroke::new(1.25, TRACE)));
}

fn fill_under_trace(painter: &Painter, rect: Rect, line_pts: &[Pos2]) {
    let fill = Color32::from_rgba_unmultiplied(56, 189, 248, 35);
    let bottom = rect.bottom();
    let mut mesh = Mesh::default();
    for p in line_pts {
        let top = p.y.min(bottom);
        if top >= bottom {
            continue;
        }
        let half_w = if line_pts.len() > 512 {
            0.75
        } else {
            1.0
        };
        mesh.add_colored_rect(
            Rect::from_min_max(Pos2::new(p.x - half_w, top), Pos2::new(p.x + half_w, bottom)),
            fill,
        );
    }
    if !mesh.is_empty() {
        painter.add(Shape::mesh(mesh));
    }
}

fn overview_offset_to_x(offset_hz: f64, rect: Rect, overview_span_hz: f32) -> f32 {
    let half = overview_span_hz as f64 / 2.0;
    let t = ((offset_hz + half) / overview_span_hz as f64).clamp(0.0, 1.0) as f32;
    rect.left() + t * rect.width()
}

fn draw_band_overview(
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

/// Update smoothed display trace from latest FFT row (in-place).
pub fn update_trace(
    latest: &[f32],
    smoothed: &mut Vec<f32>,
    composed: &mut Vec<f32>,
    view_key: &mut TraceViewKey,
    row_rate_hz: f32,
    view_span_hz: f32,
    data_span_hz: f32,
    center_offset_hz: f64,
    smooth_alpha: f32,
    latest_changed: bool,
) {
    let key = TraceViewKey::new(
        row_rate_hz,
        view_span_hz,
        data_span_hz,
        center_offset_hz,
        latest.len(),
    );
    if latest_changed || *view_key != key {
        *composed = compose_panadapter_row(
            latest,
            row_rate_hz,
            view_span_hz,
            data_span_hz,
            center_offset_hz,
        );
        *view_key = key;
    }
    if smoothed.len() != composed.len() {
        smoothed.resize(composed.len(), -120.0);
    }
    crate::smooth::ema_update(smoothed, composed, smooth_alpha);
    if smoothed.len() <= 1024 {
        let filtered = spatial_smooth(smoothed);
        smoothed.copy_from_slice(&filtered);
    }
}

/// View parameters that force recomposing the panadapter row for the trace.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct TraceViewKey {
    row_rate_bits: u32,
    view_span_bits: u32,
    data_span_bits: u32,
    pan_bits: u64,
    latest_len: usize,
}

impl TraceViewKey {
    pub fn new(
        row_rate_hz: f32,
        view_span_hz: f32,
        data_span_hz: f32,
        center_offset_hz: f64,
        latest_len: usize,
    ) -> Self {
        Self {
            row_rate_bits: row_rate_hz.to_bits(),
            view_span_bits: view_span_hz.to_bits(),
            data_span_bits: data_span_hz.to_bits(),
            pan_bits: center_offset_hz.to_bits(),
            latest_len,
        }
    }
}
