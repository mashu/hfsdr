//! Compact status-bar indicators.

use eframe::egui::{self, Color32, Response, Ui};

use crate::theme::{OK, WARN};

/// IQ ring / pump utilization (green = consuming at rate, red = starved).
pub fn iq_buffer_gauge(ui: &mut Ui, fill: f32, buffer_secs: f32) -> Response {
    let fill = fill.clamp(0.0, 1.0);
    let size = egui::vec2(52.0, 11.0);
    let (rect, response) = ui.allocate_exact_size(size, egui::Sense::hover());
    let painter = ui.painter_at(rect);
    let rounding = 2.0;
    painter.rect_filled(rect, rounding, Color32::from_rgb(30, 36, 48));
    if fill > 0.02 {
        let mut fill_rect = rect;
        fill_rect.set_width(rect.width() * fill);
        painter.rect_filled(fill_rect, rounding, buffer_color(fill));
    }
    response.on_hover_text(format!(
        "IQ utilization {:.0}%\n\
         ~{:.2}s queued in ring · pump vs expected rate\n\
         High = samples flowing and consumed · Low / empty = stall or underrun",
        fill * 100.0,
        buffer_secs
    ))
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
