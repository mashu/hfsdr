//! Compact icon buttons for the status-bar toolbar.

use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, Response, Sense, Stroke, Ui, Vec2,
    WidgetInfo, WidgetType,
};

use crate::theme::{chip_hovered, ACCENT, MUTED};

/// Panel / tool icons drawn as line art (no external font dependency).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum StatusIcon {
    Rx,
    Dsp,
    Meter,
    Scope,
    Simple,
    Log,
    Spots,
    Help,
    Fullscreen,
    Engine,
    Filters,
    Envelope,
}

const TOGGLE_SIZE: Vec2 = Vec2::new(24.0, 22.0);
const CHIP_SIZE: Vec2 = Vec2::new(24.0, 20.0);

/// Compact panel visibility toggle — icon only, with accessibility label + tooltip.
pub fn panel_icon_toggle(ui: &mut Ui, on: &mut bool, label: &str, tooltip: &str, icon: StatusIcon) -> bool {
    let resp = icon_button(ui, TOGGLE_SIZE, *on, icon, label);
    let mut changed = false;
    if resp.clicked() {
        *on = !*on;
        changed = true;
    }
    resp.on_hover_text(format!("{label}\n{tooltip}"));
    changed
}

/// Compact action button (help, fullscreen) — not a toggle.
pub fn panel_icon_button(ui: &mut Ui, label: &str, tooltip: &str, icon: StatusIcon) -> Response {
    let resp = icon_button(ui, TOGGLE_SIZE, false, icon, label);
    resp.on_hover_text(format!("{label}\n{tooltip}"))
}

/// Diagnostic / tool chip — icon with optional accent override.
pub fn tool_icon_chip(
    ui: &mut Ui,
    active: bool,
    _hovered_accent: bool,
    accent: Color32,
    icon: StatusIcon,
    label: &str,
    tooltip: &str,
) -> Response {
    let (rect, response) = ui.allocate_exact_size(CHIP_SIZE, Sense::click());
    let hovered = chip_hovered(ui, rect, &response);
    let color = if active || hovered {
        accent
    } else {
        MUTED
    };
    paint_chip_frame(ui, rect, color, active || hovered);
    paint_icon(&ui.painter_at(rect), rect, icon, color);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    response.on_hover_text(tooltip)
}

fn icon_button(ui: &mut Ui, size: Vec2, on: bool, icon: StatusIcon, label: &str) -> Response {
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = chip_hovered(ui, rect, &response);
    let accent = if on { ACCENT } else { MUTED };
    paint_chip_frame(ui, rect, accent, on || hovered);
    paint_icon(&ui.painter_at(rect), rect, icon, accent);
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    response
}

fn paint_chip_frame(ui: &mut Ui, rect: Rect, accent: Color32, highlighted: bool) {
    let bg = if highlighted {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 34)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    let border = Color32::from_rgba_unmultiplied(
        accent.r(),
        accent.g(),
        accent.b(),
        if highlighted { 150 } else { 70 },
    );
    ui.painter_at(rect).rect(
        rect,
        4.0,
        bg,
        Stroke::new(1.0, border),
        egui::StrokeKind::Inside,
    );
}

fn paint_icon(painter: &Painter, rect: Rect, icon: StatusIcon, color: Color32) {
    let inner = rect.shrink(5.0);
    let stroke = Stroke::new(1.4, color);
    match icon {
        StatusIcon::Rx => draw_rx(painter, inner, stroke),
        StatusIcon::Dsp => draw_wave(painter, inner, stroke),
        StatusIcon::Meter => draw_meter(painter, inner, stroke),
        StatusIcon::Scope => draw_scope(painter, inner, stroke),
        StatusIcon::Simple => draw_simple(painter, inner, stroke, color),
        StatusIcon::Log => draw_log(painter, inner, stroke),
        StatusIcon::Spots => draw_spots(painter, inner, color),
        StatusIcon::Help => draw_help(painter, rect, color),
        StatusIcon::Fullscreen => draw_fullscreen(painter, inner, stroke),
        StatusIcon::Engine => draw_engine(painter, inner, stroke),
        StatusIcon::Filters => draw_filters(painter, inner, stroke),
        StatusIcon::Envelope => draw_envelope(painter, inner, stroke),
    }
}

fn draw_rx(painter: &Painter, r: Rect, stroke: Stroke) {
    let cx = r.center().x;
    painter.line_segment([Pos2::new(cx, r.bottom()), Pos2::new(cx, r.top() + 2.0)], stroke);
    painter.line_segment(
        [Pos2::new(cx - 4.0, r.top() + 2.0), Pos2::new(cx + 4.0, r.top() + 2.0)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(cx - 3.0, r.top() + 2.0), Pos2::new(cx - 5.0, r.top() - 1.0)],
        stroke,
    );
    painter.line_segment(
        [Pos2::new(cx + 3.0, r.top() + 2.0), Pos2::new(cx + 5.0, r.top() - 1.0)],
        stroke,
    );
}

fn draw_wave(painter: &Painter, r: Rect, stroke: Stroke) {
    let mid = r.center().y;
    let amp = r.height() * 0.35;
    let left = r.left();
    let w = r.width();
    let pts: Vec<Pos2> = (0..=16)
        .map(|i| {
            let t = i as f32 / 16.0;
            let x = left + t * w;
            let y = mid - amp * (t * std::f32::consts::TAU * 2.0).sin();
            Pos2::new(x, y)
        })
        .collect();
    for pair in pts.windows(2) {
        painter.line_segment([pair[0], pair[1]], stroke);
    }
}

fn draw_meter(painter: &Painter, r: Rect, stroke: Stroke) {
    let bar_w = 2.5;
    let gap = 2.5;
    let base = r.bottom();
    let heights = [0.45, 0.85, 0.65];
    let total_w = heights.len() as f32 * bar_w + (heights.len() - 1) as f32 * gap;
    let mut x = r.center().x - total_w / 2.0;
    for h in heights {
        let h_px = r.height() * h;
        painter.rect_stroke(
            Rect::from_min_max(Pos2::new(x, base - h_px), Pos2::new(x + bar_w, base)),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
        x += bar_w + gap;
    }
}

fn draw_scope(painter: &Painter, r: Rect, stroke: Stroke) {
    painter.rect_stroke(r, 2.0, stroke, egui::StrokeKind::Inside);
    let inner = r.shrink(2.0);
    let mid = inner.center().y;
    let amp = inner.height() * 0.3;
    let left = inner.left();
    let w = inner.width();
    let pts: Vec<Pos2> = (0..=10)
        .map(|i| {
            let t = i as f32 / 10.0;
            let x = left + t * w;
            let y = mid - amp * (t * std::f32::consts::PI * 3.0).sin();
            Pos2::new(x, y)
        })
        .collect();
    for pair in pts.windows(2) {
        painter.line_segment([pair[0], pair[1]], stroke);
    }
}

fn draw_simple(painter: &Painter, r: Rect, stroke: Stroke, fill: Color32) {
    let c = r.center();
    let rad = r.width().min(r.height()) * 0.38;
    painter.circle_stroke(c, rad, stroke);
    painter.circle_filled(c, rad * 0.28, fill);
}

fn draw_log(painter: &Painter, r: Rect, stroke: Stroke) {
    let inset = 1.0;
    for (i, frac) in [0.25, 0.5, 0.75].iter().enumerate() {
        let y = egui::lerp(r.top() + inset..=r.bottom() - inset, *frac);
        let width_frac = match i {
            0 => 0.9,
            1 => 0.7,
            _ => 0.85,
        };
        let x0 = r.left() + inset;
        let x1 = r.left() + inset + r.width() * width_frac;
        painter.line_segment([Pos2::new(x0, y), Pos2::new(x1, y)], stroke);
    }
}

fn draw_spots(painter: &Painter, r: Rect, color: Color32) {
    let dot_r = 1.6;
    let cols = 3;
    let rows = 2;
    let gap_x = r.width() / (cols as f32 + 1.0);
    let gap_y = r.height() / (rows as f32 + 1.0);
    for row in 0..rows {
        for col in 0..cols {
            let x = r.left() + gap_x * (col as f32 + 1.0);
            let y = r.top() + gap_y * (row as f32 + 1.0);
            painter.circle_filled(Pos2::new(x, y), dot_r, color);
        }
    }
}

fn draw_help(painter: &Painter, rect: Rect, color: Color32) {
    painter.text(
        rect.center(),
        Align2::CENTER_CENTER,
        "?",
        FontId::proportional(13.0),
        color,
    );
}

fn draw_fullscreen(painter: &Painter, r: Rect, stroke: Stroke) {
    let len = 3.5;
    let inset = 1.0;
    let corners = [
        (r.left() + inset, r.top() + inset, 1.0, 1.0),
        (r.right() - inset, r.top() + inset, -1.0, 1.0),
        (r.left() + inset, r.bottom() - inset, 1.0, -1.0),
        (r.right() - inset, r.bottom() - inset, -1.0, -1.0),
    ];
    for (x, y, dx, dy) in corners {
        painter.line_segment([Pos2::new(x, y), Pos2::new(x + dx * len, y)], stroke);
        painter.line_segment([Pos2::new(x, y), Pos2::new(x, y + dy * len)], stroke);
    }
}

fn draw_engine(painter: &Painter, r: Rect, stroke: Stroke) {
    let box_w = r.width() * 0.28;
    let box_h = r.height() * 0.45;
    let y = r.center().y - box_h / 2.0;
    let x0 = r.left();
    let x1 = r.left() + box_w;
    let x2 = r.right() - box_w;
    let x3 = r.right();
    for (x_start, _x_end) in [(x0, x1), (x2, x3)] {
        let rect = Rect::from_min_size(Pos2::new(x_start, y), Vec2::new(box_w, box_h));
        painter.rect_stroke(rect, 2.0, stroke, egui::StrokeKind::Inside);
    }
    let mid_y = r.center().y;
    painter.line_segment([Pos2::new(x1, mid_y), Pos2::new(x2, mid_y)], stroke);
}

fn draw_filters(painter: &Painter, r: Rect, stroke: Stroke) {
    let top = r.top() + 1.0;
    let bot = r.bottom() - 1.0;
    let mid = r.center().x;
    let pts = [
        Pos2::new(r.left(), top),
        Pos2::new(mid, bot),
        Pos2::new(r.right(), top),
    ];
    for pair in pts.windows(2) {
        painter.line_segment([pair[0], pair[1]], stroke);
    }
}

fn draw_envelope(painter: &Painter, r: Rect, stroke: Stroke) {
    let base = r.bottom() - 1.0;
    let top = r.top() + 2.0;
    let left = r.left();
    let right = r.right();
    let pts = [
        Pos2::new(left, base),
        Pos2::new(left + r.width() * 0.15, top),
        Pos2::new(right - r.width() * 0.15, top),
        Pos2::new(right, base),
    ];
    for pair in pts.windows(2) {
        painter.line_segment([pair[0], pair[1]], stroke);
    }
}

/// Width reserved for the right-hand icon toolbar (icons + separators + help/fullscreen).
pub fn toolbar_reserved_width(simple_mode: bool) -> f32 {
    let toggles = if simple_mode { 5 } else { 7 };
  // toggles + sep + help + fullscreen + spacing
    toggles as f32 * (TOGGLE_SIZE.x + 4.0) + 56.0
}

#[cfg(test)]
mod tests {
    use super::*;
    use egui_kittest::Harness;

    #[test]
    fn icon_toggles_render_without_panic() {
        let mut on = true;
        let mut harness = Harness::builder()
            .with_size(Vec2::new(320.0, 40.0))
            .build_ui_state(|ui, ()| {
                ui.horizontal(|ui| {
                    let _ = panel_icon_toggle(ui, &mut on, "RX", "VFO panel", StatusIcon::Rx);
                    let _ = tool_icon_chip(
                        ui,
                        false,
                        false,
                        ACCENT,
                        StatusIcon::Engine,
                        "Engine",
                        "Pipeline",
                    );
                });
            }, ());
        harness.run_steps(2);
    }
}
