//! AF oscilloscope and RF-gain tuning hints (FTX-1 / classic superhet style).
//!
//! Bipolar audio trace around zero: barely lifting = too little front-end gain;
//! pinned to the rails = AGC riding noise continuously.

use eframe::egui::{self, Align2, Color32, FontId, Pos2, Sense, Shape, Stroke, Ui, Vec2};

use crate::theme::{ACCENT, MUTED, OK, WARN};

pub const SCOPE_LEN: usize = 320;
/// Classic “half scale” target (~−6 dB of full swing).
pub const HALF_SCALE: f32 = 0.45;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AudioLevelHint {
    Idle,
    TooQuiet,
    SweetSpot,
    TooHot,
}

pub fn classify_level(peak: f32, agc_enabled: bool, agc_gain: f32, streaming: bool) -> AudioLevelHint {
    if !streaming {
        return AudioLevelHint::Idle;
    }
    if peak < 1e-5 {
        return AudioLevelHint::Idle;
    }
    let agc_starved = agc_enabled && agc_gain > 14.0;
    let agc_saturated = agc_enabled && agc_gain < 0.12;
    if peak < 0.07 || agc_starved {
        return AudioLevelHint::TooQuiet;
    }
    if peak > 0.88 || agc_saturated {
        return AudioLevelHint::TooHot;
    }
    if peak >= 0.10 && peak <= 0.70 && !agc_starved && !agc_saturated {
        return AudioLevelHint::SweetSpot;
    }
    if peak > 0.70 {
        AudioLevelHint::TooHot
    } else {
        AudioLevelHint::SweetSpot
    }
}

fn hint_label(h: AudioLevelHint) -> &'static str {
    match h {
        AudioLevelHint::Idle => "Waiting for audio…",
        AudioLevelHint::TooQuiet => "Too quiet — raise RF gain / preamp or lower attenuator",
        AudioLevelHint::SweetSpot => "Good dynamic range — trace near half scale, AGC not pinned",
        AudioLevelHint::TooHot => "Too hot — lower RF gain or enable attenuator; AGC riding noise",
    }
}

fn hint_color(h: AudioLevelHint) -> Color32 {
    match h {
        AudioLevelHint::Idle => MUTED,
        AudioLevelHint::TooQuiet => WARN,
        AudioLevelHint::SweetSpot => OK,
        AudioLevelHint::TooHot => Color32::from_rgb(255, 120, 100),
    }
}

pub struct AfScopeParams<'a> {
    pub samples: &'a [f32],
    pub peak: f32,
    pub rms: f32,
    pub agc_gain: f32,
    pub agc_envelope: f32,
    pub agc_enabled: bool,
    pub agc_target: f32,
    pub iq_headroom: f32,
    pub hint: AudioLevelHint,
}

pub fn show_af_tuning_panel(ui: &mut Ui, p: &AfScopeParams<'_>) {
    ui.label(
        egui::RichText::new("AF scope — tune RF gain")
            .small()
            .color(MUTED),
    );
    section_hint(
        ui,
        "Aim for the trace near the ±half-scale lines without hitting the rails. \
         Too little RF gain: trace barely leaves zero and IQ AGC gain stays high. \
         Too much: trace clips and AGC works continuously on band noise.",
    );
    show_af_scope(ui, p);
    ui.horizontal(|ui| {
        ui.label(
            egui::RichText::new(format!(
                "peak {:.3} · rms {:.3}",
                p.peak, p.rms
            ))
            .monospace()
            .small()
            .color(MUTED),
        );
        if p.agc_enabled {
            ui.label(
                egui::RichText::new(format!(
                    "IQ AGC {:.1}× · env {:.3} (tgt {:.2})",
                    p.agc_gain, p.agc_envelope, p.agc_target
                ))
                .monospace()
                .small()
                .color(MUTED),
            );
        }
        ui.label(
            egui::RichText::new(format!("IQ buf {:.0}%", p.iq_headroom * 100.0))
                .monospace()
                .small()
                .color(MUTED),
        );
    });
    ui.label(
        egui::RichText::new(hint_label(p.hint))
            .small()
            .color(hint_color(p.hint)),
    );
}

pub fn show_af_scope(ui: &mut Ui, p: &AfScopeParams<'_>) {
    let h = 72.0;
    let (rect, _resp) = ui.allocate_exact_size(Vec2::new(ui.available_width().max(200.0), h), Sense::hover());
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 6.0, Color32::from_rgb(10, 14, 20));
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_rgb(40, 50, 68)),
        egui::StrokeKind::Inside,
    );

    let mid_y = rect.center().y;
    let half_h = rect.height() * 0.42;
    let full_scale = 1.0f32;

    painter.line_segment(
        [Pos2::new(rect.left() + 4.0, mid_y), Pos2::new(rect.right() - 4.0, mid_y)],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 90)),
    );

    for sign in [-1.0f32, 1.0] {
        let y = mid_y - sign * HALF_SCALE / full_scale * half_h;
        let mut pts = vec![];
        let x0 = rect.left() + 6.0;
        let x1 = rect.right() - 6.0;
        let dash = 6.0;
        let mut x = x0;
        while x < x1 {
            let x_end = (x + dash).min(x1);
            pts.push(Pos2::new(x, y));
            pts.push(Pos2::new(x_end, y));
            x += dash * 2.0;
        }
        painter.add(Shape::line(
            pts,
            Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 100),
            ),
        ));
    }

    let clip_y_top = rect.top() + 4.0;
    let clip_y_bot = rect.bottom() - 4.0;
    painter.line_segment(
        [
            Pos2::new(rect.left() + 2.0, clip_y_top),
            Pos2::new(rect.right() - 2.0, clip_y_top),
        ],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 100, 90, 80)),
    );
    painter.line_segment(
        [
            Pos2::new(rect.left() + 2.0, clip_y_bot),
            Pos2::new(rect.right() - 2.0, clip_y_bot),
        ],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 100, 90, 80)),
    );

    if p.samples.len() >= 2 {
        let n = p.samples.len();
        let dx = (rect.width() - 8.0) / (n - 1) as f32;
        let mut pts = Vec::with_capacity(n);
        let mut clipped = false;
        for (i, &s) in p.samples.iter().enumerate() {
            let x = rect.left() + 4.0 + i as f32 * dx;
            let y = mid_y - (s / full_scale).clamp(-1.2, 1.2) * half_h;
            if y <= clip_y_top + 1.0 || y >= clip_y_bot - 1.0 {
                clipped = true;
            }
            pts.push(Pos2::new(x, y));
        }
        let trace_color = if clipped {
            Color32::from_rgb(255, 140, 110)
        } else {
            ACCENT
        };
        painter.add(Shape::line(pts, Stroke::new(1.5, trace_color)));
    } else {
        painter.text(
            rect.center(),
            Align2::CENTER_CENTER,
            "No audio yet",
            FontId::monospace(11.0),
            MUTED,
        );
    }

    painter.text(
        Pos2::new(rect.left() + 6.0, rect.top() + 4.0),
        Align2::LEFT_TOP,
        "±half",
        FontId::monospace(9.0),
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 140),
    );
}

fn section_hint(ui: &mut Ui, text: &str) {
    ui.label(egui::RichText::new(text).small().italics().color(MUTED));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_when_peak_low() {
        assert_eq!(
            classify_level(0.02, true, 20.0, true),
            AudioLevelHint::TooQuiet
        );
    }

    #[test]
    fn hot_when_clipping() {
        assert_eq!(
            classify_level(0.95, true, 0.05, true),
            AudioLevelHint::TooHot
        );
    }

    #[test]
    fn sweet_in_mid_range() {
        assert_eq!(
            classify_level(0.35, true, 2.0, true),
            AudioLevelHint::SweetSpot
        );
    }
}
