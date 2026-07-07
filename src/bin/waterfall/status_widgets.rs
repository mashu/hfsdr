//! Compact status-bar indicators.

use eframe::egui::{self, Color32, FontId, Response, Sense, Stroke, Ui, Vec2, WidgetInfo, WidgetType};

use crate::status_icons::{tool_icon_chip, StatusIcon};
use crate::theme::{chip_hovered, ACCENT, MUTED, OK, WARN};

/// Engine / pipeline chip — opens the interactive DSP flow diagram.
pub fn engine_pipeline_chip(ui: &mut Ui, panel_open: bool, streaming: bool) -> Response {
    let accent = if panel_open {
        ACCENT
    } else if streaming {
        OK
    } else {
        MUTED
    };
    tool_icon_chip(
        ui,
        panel_open,
        false,
        accent,
        StatusIcon::Engine,
        "Engine",
        "Receive pipeline — source, DSP stages, spectrum, skimmer, sinks\n\
         Click for draggable flow diagram with live stage status",
    )
}

/// Sidetone envelope chip — opens before/after keying shape preview.
pub fn envelope_diagnostic_chip(ui: &mut Ui, panel_open: bool, envelope_active: bool) -> Response {
    let accent = if panel_open {
        ACCENT
    } else if envelope_active {
        Color32::from_rgb(251, 191, 36)
    } else {
        MUTED
    };
    tool_icon_chip(
        ui,
        panel_open,
        false,
        accent,
        StatusIcon::Envelope,
        "Envelope",
        "Sidetone envelope preview — hard BFO edges vs shaped keying\n\
         Compare rise, fall, and edge shape from CW demod settings",
    )
}

/// Filter diagnostic chip — opens magnitude response curves (notches + channel FIR).
pub fn filter_diagnostic_chip(ui: &mut Ui, panel_open: bool, filters_active: bool) -> Response {
    let accent = if panel_open {
        ACCENT
    } else if filters_active {
        Color32::from_rgb(125, 211, 252)
    } else {
        MUTED
    };
    tool_icon_chip(
        ui,
        panel_open,
        false,
        accent,
        StatusIcon::Filters,
        "Filters",
        "Filter magnitude response — true channel FIR + manual notch curves\n\
         Compare active path vs bypass; GUI plot overlays are control hints only",
    )
}

/// IQ ring buffer — framed, labeled control; click opens record / playback panel.
pub fn iq_buffer_control(ui: &mut Ui, fill: f32, buffer_secs: f32, panel_open: bool) -> Response {
    let fill = fill.clamp(0.0, 1.0);
    let size = Vec2::new(52.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = chip_hovered(ui, rect, &response);
    let painter = ui.painter_at(rect);
    let rounding = 4.0;

    let accent = if panel_open || hovered { ACCENT } else { MUTED };
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

    let inner = rect.shrink2(Vec2::new(4.0, 4.0));
    let label_w = 12.0;
    let bar_rect = egui::Rect::from_min_max(
        egui::pos2(inner.left() + label_w, inner.center().y - 4.0),
        egui::pos2(inner.right(), inner.center().y + 4.0),
    );

    painter.text(
        egui::pos2(inner.left(), inner.center().y),
        egui::Align2::LEFT_CENTER,
        "IQ",
        FontId::monospace(8.5),
        if hovered || panel_open { ACCENT } else { MUTED },
    );

    painter.rect_filled(bar_rect, 2.0, Color32::from_rgb(18, 22, 30));
    if fill > 0.02 {
        let mut fill_rect = bar_rect;
        fill_rect.set_width(bar_rect.width() * fill);
        painter.rect_filled(fill_rect, 2.0, buffer_color(fill));
    }

    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "IQ"));
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
    let color = if recording { WARN } else { MUTED };
    let size = Vec2::new(24.0, 20.0);
    let enabled = recording || can_record;
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let hovered = chip_hovered(ui, rect, &response);
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

    let dot_color = if !enabled {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 120)
    } else {
        color
    };
    let c = rect.center();
    let rad = 5.0;
    painter.circle_stroke(c, rad, Stroke::new(1.2, dot_color));
    if recording {
        painter.circle_filled(c, rad * 0.55, dot_color);
    }

    let tip = if recording {
        format!(
            "REC {elapsed_secs:.1}s — stop IQ recording\n\
             Toggle off then on again to start a new timestamped .hiq.gz"
        )
    } else if can_record {
        "REC — start IQ recording\n\
         Saves gzip .hiq.gz with timestamp · toggle off/on for next file"
            .to_string()
    } else {
        "REC — connect (or stream) to record IQ".to_string()
    };
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "REC"));
    response.on_hover_text(tip)
}

/// One-click replay of the IQ file selected in the I/O panel.
pub fn iq_playback_chip(ui: &mut Ui, playing: bool, has_file: bool) -> Response {
    let size = Vec2::new(24.0, 20.0);
    let enabled = has_file;
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let hovered = chip_hovered(ui, rect, &response);
    let painter = ui.painter_at(rect);
    let rounding = 4.0;
    let accent = if playing { OK } else { ACCENT };
    let stroke_color = if !enabled {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 60)
    } else if playing {
        Color32::from_rgba_unmultiplied(OK.r(), OK.g(), OK.b(), if hovered { 220 } else { 160 })
    } else if hovered {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 220)
    } else {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 130)
    };
    let bg = if !enabled {
        Color32::from_rgb(30, 36, 48)
    } else if playing {
        Color32::from_rgba_unmultiplied(OK.r(), OK.g(), OK.b(), if hovered { 50 } else { 36 })
    } else if hovered {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 28)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    painter.rect(rect, rounding, bg, Stroke::new(1.0, stroke_color), egui::StrokeKind::Inside);
    let c = rect.center();
    let s = 5.0;
    let tri_color = if !enabled {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 120)
    } else {
        accent
    };
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x - s * 0.4, c.y - s),
            egui::pos2(c.x + s * 0.7, c.y),
            egui::pos2(c.x - s * 0.4, c.y + s),
        ],
        tri_color,
        Stroke::NONE,
    ));
    let tip = if !enabled {
        "Play — choose an IQ file in the I/O panel first"
    } else if playing {
        "Play — replay selected IQ file from the start"
    } else {
        "Play — play selected IQ file"
    };
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, "Play"));
    response.on_hover_text(tip)
}

/// Receiver alias beside the connection badge — click opens connection settings.
pub fn connection_alias_chip(ui: &mut Ui, alias: &str, compact: bool) -> Response {
    let text = truncate_middle(alias, if compact { 18 } else { 28 });
    let size = Vec2::new(if compact { 88.0 } else { 128.0 }, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = chip_hovered(ui, rect, &response);
    let painter = ui.painter_at(rect);
    let rounding = 4.0;
    let border = if hovered {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 180)
    } else {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 90)
    };
    let bg = if hovered {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 20)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    painter.rect(rect, rounding, bg, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        text,
        FontId::monospace(10.5),
        if hovered { ACCENT } else { MUTED },
    );
    response.on_hover_text(format!("{alias}\nClick for connection settings"))
}

/// Fixed-width cursor frequency readout — accent when hovering the plot, muted placeholder otherwise.
pub fn cursor_freq_slot(ui: &mut Ui, label: &str, active: bool, compact: bool) -> Response {
    let size = Vec2::new(if compact { 108.0 } else { 156.0 }, 14.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::hover());
    let color = if active {
        ACCENT
    } else {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 100)
    };
    ui.painter_at(rect).text(
        rect.left_center(),
        egui::Align2::LEFT_CENTER,
        label,
        FontId::monospace(10.5),
        color,
    );
    if active {
        response.on_hover_text("Mouse position on spectrum / waterfall")
    } else {
        response.on_hover_text("Hover spectrum or waterfall to read cursor frequency")
    }
}

/// One-click connect to the last configured receiver (beside the OFFLINE badge).
pub fn quick_connect_chip(ui: &mut Ui, enabled: bool) -> Response {
    let size = Vec2::new(22.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(
        size,
        if enabled { Sense::click() } else { Sense::hover() },
    );
    let hovered = chip_hovered(ui, rect, &response);
    let painter = ui.painter_at(rect);
    let rounding = 4.0;
    let stroke_color = if !enabled {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 60)
    } else if hovered {
        Color32::from_rgba_unmultiplied(OK.r(), OK.g(), OK.b(), 220)
    } else {
        Color32::from_rgba_unmultiplied(OK.r(), OK.g(), OK.b(), 130)
    };
    let bg = if !enabled {
        Color32::from_rgb(30, 36, 48)
    } else if hovered {
        Color32::from_rgba_unmultiplied(OK.r(), OK.g(), OK.b(), 42)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    painter.rect(rect, rounding, bg, Stroke::new(1.0, stroke_color), egui::StrokeKind::Inside);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "⚡",
        FontId::proportional(11.0),
        if !enabled {
            Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 120)
        } else if hovered {
            OK
        } else {
            MUTED
        },
    );
    response
}

/// One-click disconnect beside the connection badge.
pub fn disconnect_chip(ui: &mut Ui) -> Response {
    let size = Vec2::new(22.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = chip_hovered(ui, rect, &response);
    let painter = ui.painter_at(rect);
    let rounding = 4.0;
    let stroke_color = if hovered {
        Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), 220)
    } else {
        Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), 130)
    };
    let bg = if hovered {
        Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), 42)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    painter.rect(rect, rounding, bg, Stroke::new(1.0, stroke_color), egui::StrokeKind::Inside);
    painter.text(
        rect.center(),
        egui::Align2::CENTER_CENTER,
        "✕",
        FontId::proportional(11.0),
        if hovered { WARN } else { MUTED },
    );
    response.on_hover_text("Disconnect")
}

fn truncate_middle(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let keep = max_chars.saturating_sub(1) / 2;
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().rev().take(keep).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{head}…{tail}")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_middle_short_string_unchanged() {
        assert_eq!(truncate_middle("hello", 10), "hello");
    }

    #[test]
    fn truncate_middle_long_string_uses_ellipsis() {
        let s = truncate_middle("abcdefghijklmnopqrstuvwxyz", 10);
        assert!(s.contains('…'));
        assert!(s.chars().count() <= 10);
    }

    #[test]
    fn buffer_color_interpolates_low_to_high() {
        let low = buffer_color(0.0);
        let mid = buffer_color(0.5);
        let high = buffer_color(1.0);
        assert_ne!(low, high);
        assert_ne!(low, mid);
    }

    #[test]
    fn status_chips_render_in_ui() {
        use eframe::egui::Vec2;
        use egui_kittest::Harness;

        let mut harness = Harness::builder()
            .with_size(Vec2::new(640.0, 40.0))
            .build_ui_state(|ui, ()| {
                ui.horizontal(|ui| {
                    let _ = engine_pipeline_chip(ui, false, true);
                    let _ = filter_diagnostic_chip(ui, false, true);
                    let _ = envelope_diagnostic_chip(ui, false, true);
                    let _ = iq_buffer_control(ui, 0.55, 1.2, false);
                    let _ = iq_record_toggle(ui, false, true, 0.0);
                    let _ = iq_playback_chip(ui, false, true);
                    let _ = connection_alias_chip(ui, "rx.test:8073", false);
                    let _ = cursor_freq_slot(ui, "14010.000 kHz", true, false);
                    let _ = quick_connect_chip(ui, true);
                    let _ = disconnect_chip(ui);
                });
            }, ());
        harness.run_steps(2);
    }
}
