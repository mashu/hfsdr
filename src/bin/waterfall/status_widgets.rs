//! Compact status-bar indicators.

use eframe::egui::{self, Color32, FontId, Response, Sense, Stroke, Ui, Vec2};

use crate::theme::{chip_hovered, ACCENT, MUTED, OK, WARN};

/// Engine / pipeline chip — opens the interactive DSP flow diagram.
pub fn engine_pipeline_chip(ui: &mut Ui, panel_open: bool, streaming: bool) -> Response {
    let size = Vec2::new(72.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = chip_hovered(ui, rect, &response);
    let painter = ui.painter_at(rect);
    let rounding = 4.0;
    let accent = if panel_open || hovered {
        ACCENT
    } else if streaming {
        OK
    } else {
        MUTED
    };
    let border = Color32::from_rgba_unmultiplied(
        accent.r(),
        accent.g(),
        accent.b(),
        if hovered || panel_open { 200 } else { 110 },
    );
    let bg = if hovered || panel_open {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 28)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    painter.rect(rect, rounding, bg, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    painter.text(
        rect.center() - Vec2::new(6.0, 0.0),
        egui::Align2::CENTER_CENTER,
        "Engine",
        FontId::proportional(11.0),
        accent,
    );
    painter.text(
        egui::pos2(rect.right() - 6.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "▾",
        FontId::proportional(10.0),
        accent,
    );
    response.on_hover_text(
        "Receive pipeline — source, DSP stages, spectrum, skimmer, sinks\n\
         Click for draggable flow diagram with live stage status",
    )
}

/// Sidetone envelope chip — opens before/after keying shape preview.
pub fn envelope_diagnostic_chip(ui: &mut Ui, panel_open: bool, envelope_active: bool) -> Response {
    let size = Vec2::new(72.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = chip_hovered(ui, rect, &response);
    let painter = ui.painter_at(rect);
    let rounding = 4.0;
    let accent = if panel_open || hovered {
        ACCENT
    } else if envelope_active {
        Color32::from_rgb(251, 191, 36)
    } else {
        MUTED
    };
    let border = Color32::from_rgba_unmultiplied(
        accent.r(),
        accent.g(),
        accent.b(),
        if hovered || panel_open { 200 } else { 110 },
    );
    let bg = if hovered || panel_open {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 28)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    painter.rect(rect, rounding, bg, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    painter.text(
        rect.center() - Vec2::new(6.0, 0.0),
        egui::Align2::CENTER_CENTER,
        "Envelope",
        FontId::proportional(11.0),
        accent,
    );
    painter.text(
        egui::pos2(rect.right() - 6.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "▾",
        FontId::proportional(10.0),
        accent,
    );
    response.on_hover_text(
        "Sidetone envelope preview — hard BFO edges vs shaped keying\n\
         Compare rise, fall, and edge shape from CW demod settings",
    )
}

/// Filter diagnostic chip — opens magnitude response curves (notches + channel FIR).
pub fn filter_diagnostic_chip(ui: &mut Ui, panel_open: bool, filters_active: bool) -> Response {
    let size = Vec2::new(58.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = chip_hovered(ui, rect, &response);
    let painter = ui.painter_at(rect);
    let rounding = 4.0;
    let accent = if panel_open || hovered {
        ACCENT
    } else if filters_active {
        Color32::from_rgb(125, 211, 252)
    } else {
        MUTED
    };
    let border = Color32::from_rgba_unmultiplied(
        accent.r(),
        accent.g(),
        accent.b(),
        if hovered || panel_open { 200 } else { 110 },
    );
    let bg = if hovered || panel_open {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 28)
    } else {
        Color32::from_rgb(30, 36, 48)
    };
    painter.rect(rect, rounding, bg, Stroke::new(1.0, border), egui::StrokeKind::Inside);
    painter.text(
        rect.center() - Vec2::new(4.0, 0.0),
        egui::Align2::CENTER_CENTER,
        "Filters",
        FontId::proportional(11.0),
        accent,
    );
    painter.text(
        egui::pos2(rect.right() - 6.0, rect.center().y),
        egui::Align2::RIGHT_CENTER,
        "▾",
        FontId::proportional(10.0),
        accent,
    );
    response.on_hover_text(
        "Filter magnitude response — true channel FIR + manual notch curves\n\
         Compare active path vs bypass; GUI plot overlays are control hints only",
    )
}

/// IQ ring buffer — framed, labeled control; click opens record / playback panel.
pub fn iq_buffer_control(ui: &mut Ui, fill: f32, buffer_secs: f32, panel_open: bool) -> Response {
    let fill = fill.clamp(0.0, 1.0);
    let size = Vec2::new(92.0, 20.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    let hovered = chip_hovered(ui, rect, &response);
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

/// One-click replay of the IQ file selected in the I/O panel.
pub fn iq_playback_chip(ui: &mut Ui, playing: bool, has_file: bool) -> Response {
    let size = Vec2::new(28.0, 20.0);
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
    painter.text(
        rect.center() + Vec2::new(1.0, 0.0),
        egui::Align2::CENTER_CENTER,
        "▶",
        FontId::proportional(10.0),
        if !enabled {
            Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 120)
        } else {
            accent
        },
    );
    if !enabled {
        response.on_hover_text("Choose an IQ file in the I/O panel first")
    } else if playing {
        response.on_hover_text("Replay selected IQ file from the start")
    } else {
        response.on_hover_text("Play selected IQ file")
    }
}

/// Receiver alias beside the connection badge — click opens connection settings.
pub fn connection_alias_chip(ui: &mut Ui, alias: &str) -> Response {
    let text = truncate_middle(alias, 28);
    let size = Vec2::new(128.0, 20.0);
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
pub fn cursor_freq_slot(ui: &mut Ui, label: &str, active: bool) -> Response {
    let size = Vec2::new(156.0, 14.0);
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
                    let _ = connection_alias_chip(ui, "rx.test:8073");
                    let _ = cursor_freq_slot(ui, "14010.000 kHz", true);
                    let _ = quick_connect_chip(ui, true);
                    let _ = disconnect_chip(ui);
                });
            }, ());
        harness.run_steps(2);
    }
}
