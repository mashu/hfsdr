use eframe::egui::{
    Align2, Color32, FontId, Mesh, Painter, Pos2, Rect, Shape, Stroke,
};
use hfsdr::compose_panadapter_row;

use crate::theme::{GRID, TRACE, TRACE_GLOW};

use super::smooth::{self, spatial_smooth};

/// Update smoothed display trace from latest FFT row (in-place).
pub fn update_trace(
    latest: &[f32],
    smoothed: &mut Vec<f32>,
    composed: &mut Vec<f32>,
    view_key: &mut TraceViewKey,
    row_rate_hz: f32,
    view_span_hz: f32,
    data_span_hz: f32,
    compose_pan_offset_hz: f64,
    allow_band_padding: bool,
    smooth_alpha: f32,
    latest_changed: bool,
) {
    let key = TraceViewKey::new(
        row_rate_hz,
        view_span_hz,
        data_span_hz,
        compose_pan_offset_hz,
        latest.len(),
    );
    if latest_changed || *view_key != key {
        *composed = compose_panadapter_row(
            latest,
            row_rate_hz,
            view_span_hz,
            data_span_hz,
            compose_pan_offset_hz,
            allow_band_padding,
        );
        *view_key = key;
    }
    if smoothed.len() != composed.len() {
        smoothed.resize(composed.len(), -120.0);
    }
    smooth::ema_update(smoothed, composed, smooth_alpha);
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

pub(crate) fn draw_plot_background(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, 6.0, Color32::from_rgb(10, 12, 18));
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_rgb(40, 48, 64)),
        eframe::egui::StrokeKind::Outside,
    );
}

pub(crate) fn draw_db_scale(painter: &Painter, rect: Rect, ref_db: f32, range_db: f32) {
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

pub(crate) fn draw_trace(painter: &Painter, rect: Rect, trace: &[f32], ref_db: f32, range_db: f32) {
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
