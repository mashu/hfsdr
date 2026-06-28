//! Sidetone envelope diagnostic — keyed demo waveform before vs after shaping.

use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2,
};

use hfsdr::{SidetoneEnvelope, SidetoneEnvelopeSettings, SidetoneEnvelopeShape};

use crate::theme::{ACCENT, MUTED, OK, WARN};

const DEMO_POINTS: usize = 480;
const TONE_HZ: f32 = 650.0;
const KEY_LEVEL: f32 = 0.25;
const IDLE_LEVEL: f32 = 0.001;

/// Live settings for the envelope comparison plot.
pub struct EnvelopeDiagnosticView<'a> {
    pub settings: &'a SidetoneEnvelopeSettings,
    pub audio_rate: f32,
    pub streaming: bool,
}

#[derive(Clone, Debug)]
struct DemoCurves {
    times_ms: Vec<f32>,
    before: Vec<f32>,
    after: Vec<f32>,
    gain: Vec<f32>,
}

pub fn show_envelope_diagnostic_panel(ui: &mut Ui, view: &EnvelopeDiagnosticView<'_>) {
    let settings = view.settings.clamped();
    ui.label(
        RichText::new(
            "Keyed CW demo — hard BFO edges vs the same bursts after sidetone envelope shaping. \
             Adjust rise, fall, and edge shape in CW demod → Sidetone envelope.",
        )
        .small()
        .color(MUTED),
    );
    ui.add_space(6.0);

    ui.horizontal(|ui| {
        let status = if settings.enabled {
            format!(
                "On · rise {:.1} ms · fall {:.1} ms · {}",
                settings.rise_ms,
                settings.fall_ms,
                shape_label(settings.shape),
            )
        } else {
            "Off — pass-through (raw BFO edges)".to_string()
        };
        ui.label(
            RichText::new(status)
                .small()
                .color(if settings.enabled {
                    ACCENT
                } else {
                    WARN
                }),
        );
    });
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        legend_dot(ui, Color32::from_rgb(148, 163, 184), "Before shaping");
        legend_dot(ui, OK, "After shaping");
        legend_dot(
            ui,
            Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 200),
            "Gain envelope",
        );
    });
    ui.add_space(6.0);

    let curves = build_demo_curves(&settings, view.audio_rate);
    let plot_h = 200.0;
    let (rect, _resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(320.0), plot_h),
        Sense::hover(),
    );
    paint_envelope_waveform(&ui.painter_at(rect), rect, &curves);

    ui.add_space(4.0);
    if !settings.enabled {
        ui.label(
            RichText::new("Enable envelope in CW demod to soften key-up/key-down clicks.")
                .small()
                .color(MUTED),
        );
    } else if view.streaming {
        ui.label(
            RichText::new(
                "Live demod audio uses this shaper — faster rise/fall or exponential edges \
                 sound sharper on fast keying.",
            )
            .small()
            .color(MUTED),
        );
    } else {
        ui.label(
            RichText::new(
                "Connect and copy CW to hear the same rise/fall on live signals.",
            )
            .small()
            .color(MUTED),
        );
    }
}

fn shape_label(shape: SidetoneEnvelopeShape) -> &'static str {
    match shape {
        SidetoneEnvelopeShape::Cosine => "cosine",
        SidetoneEnvelopeShape::Linear => "linear",
        SidetoneEnvelopeShape::Exponential => "exponential",
    }
}

/// Instant key gate for demo segments (start_ms inclusive, end_ms exclusive, keyed).
fn keyed_at(t_ms: f32) -> bool {
    const SEGMENTS: &[(f32, f32)] = &[
        (70.0, 130.0),
        (170.0, 350.0),
        (390.0, 470.0),
    ];
    SEGMENTS.iter().any(|&(a, b)| t_ms >= a && t_ms < b)
}

fn build_demo_curves(settings: &SidetoneEnvelopeSettings, audio_rate: f32) -> DemoCurves {
    let rate = audio_rate.max(1.0);
    let demo_ms = 520.0;
    let mut times_ms = Vec::with_capacity(DEMO_POINTS);
    let mut before = Vec::with_capacity(DEMO_POINTS);
    let mut after = Vec::with_capacity(DEMO_POINTS);
    let mut gain = Vec::with_capacity(DEMO_POINTS);

    let mut env = SidetoneEnvelope::new();
    let warmup = (rate * 0.05).round() as usize;
    for i in 0..warmup {
        let t = i as f32 / rate;
        let keyed = keyed_at(t * 1000.0);
        let level = if keyed { KEY_LEVEL } else { IDLE_LEVEL };
        let audio = if keyed {
            (std::f32::consts::TAU * TONE_HZ * t).sin() * 0.35
        } else {
            0.0
        };
        let _ = env.process(audio, level, rate, settings);
    }

    for i in 0..DEMO_POINTS {
        let t_ms = demo_ms * i as f32 / (DEMO_POINTS - 1).max(1) as f32;
        let t = t_ms / 1000.0;
        let keyed = keyed_at(t_ms);
        let level = if keyed { KEY_LEVEL } else { IDLE_LEVEL };
        let raw = if keyed {
            (std::f32::consts::TAU * TONE_HZ * t).sin() * 0.35
        } else {
            0.0
        };
        let shaped = env.process(raw, level, rate, settings);
        times_ms.push(t_ms);
        before.push(raw);
        after.push(shaped);
        gain.push(if settings.enabled {
            env.gain()
        } else if keyed {
            1.0
        } else {
            0.0
        });
    }

    DemoCurves {
        times_ms,
        before,
        after,
        gain,
    }
}

fn legend_dot(ui: &mut Ui, color: Color32, label: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, color);
        ui.label(RichText::new(label).small().color(MUTED));
    });
    ui.add_space(8.0);
}

fn paint_envelope_waveform(painter: &Painter, rect: Rect, curves: &DemoCurves) {
    painter.rect_filled(rect, 6.0, Color32::from_rgb(12, 16, 24));
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_rgb(40, 48, 64)),
        egui::StrokeKind::Inside,
    );

    let margin_l = 36.0;
    let margin_r = 8.0;
    let margin_t = 10.0;
    let margin_b = 22.0;
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + margin_l, rect.top() + margin_t),
        Pos2::new(rect.right() - margin_r, rect.bottom() - margin_b),
    );
    if inner.width() < 8.0 || inner.height() < 8.0 {
        return;
    }

    let t_max = curves
        .times_ms
        .last()
        .copied()
        .unwrap_or(500.0)
        .max(50.0);
    let amp = 0.42f32;

    painter.line_segment(
        [
            Pos2::new(inner.left(), inner.center().y),
            Pos2::new(inner.right(), inner.center().y),
        ],
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 70, 90, 100)),
    );

    for frac in [0.25, 0.5, 0.75] {
        let y = inner.top() + inner.height() * frac;
        painter.line_segment(
            [Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 70, 90, 50)),
        );
    }

    let gain_y = inner.bottom() - inner.height() * 0.18;
    draw_dashed_hline(painter, inner.left(), inner.right(), gain_y, ACCENT.gamma_multiply(0.55));

    draw_waveform(
        painter,
        inner,
        t_max,
        amp,
        &curves.times_ms,
        &curves.gain,
        gain_y,
        0.0,
        amp,
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 180),
        1.25,
        true,
    );
    draw_waveform(
        painter,
        inner,
        t_max,
        amp,
        &curves.times_ms,
        &curves.before,
        inner.center().y,
        -amp,
        amp,
        Color32::from_rgba_unmultiplied(148, 163, 184, 220),
        1.5,
        false,
    );
    draw_waveform(
        painter,
        inner,
        t_max,
        amp,
        &curves.times_ms,
        &curves.after,
        inner.center().y,
        -amp,
        amp,
        Color32::from_rgba_unmultiplied(OK.r(), OK.g(), OK.b(), 240),
        2.0,
        false,
    );

    painter.text(
        Pos2::new(inner.center().x, rect.bottom() - 4.0),
        Align2::CENTER_BOTTOM,
        "ms",
        FontId::proportional(9.0),
        MUTED,
    );
    painter.text(
        Pos2::new(rect.left() + 4.0, inner.top() + 2.0),
        Align2::LEFT_TOP,
        "+",
        FontId::proportional(9.0),
        MUTED,
    );
    painter.text(
        Pos2::new(rect.left() + 4.0, inner.bottom() - 2.0),
        Align2::LEFT_BOTTOM,
        "−",
        FontId::proportional(9.0),
        MUTED,
    );
    painter.text(
        Pos2::new(inner.right() - 2.0, gain_y - 2.0),
        Align2::RIGHT_BOTTOM,
        "gain",
        FontId::proportional(8.5),
        MUTED,
    );
}

fn draw_dashed_hline(painter: &Painter, x0: f32, x1: f32, y: f32, color: Color32) {
    let dash = 4.0;
    let gap = 3.0;
    let mut x = x0;
    let mut draw = true;
    while x < x1 {
        let end = (x + if draw { dash } else { gap }).min(x1);
        if draw {
            painter.line_segment([Pos2::new(x, y), Pos2::new(end, y)], Stroke::new(1.0, color));
        }
        x = end;
        draw = !draw;
    }
}

fn draw_waveform(
    painter: &Painter,
    inner: Rect,
    t_max: f32,
    _amp_scale: f32,
    times_ms: &[f32],
    values: &[f32],
    center_y: f32,
    vmin: f32,
    vmax: f32,
    color: Color32,
    width: f32,
    dashed: bool,
) {
    if values.len() < 2 || values.len() != times_ms.len() {
        return;
    }
    let half_h = (inner.height() * 0.38).max(4.0);
    let mut prev: Option<Pos2> = None;
    for (&t, &v) in times_ms.iter().zip(values.iter()) {
        let x = inner.left() + inner.width() * (t / t_max).clamp(0.0, 1.0);
        let norm = if (vmax - vmin).abs() > 1e-6 {
            ((v - vmin) / (vmax - vmin)).clamp(0.0, 1.0)
        } else {
            0.5
        };
        let y = center_y - (norm * 2.0 - 1.0) * half_h;
        let pt = Pos2::new(x, y);
        if let Some(p0) = prev {
            if dashed {
                draw_dashed_segment(painter, p0, pt, color, width);
            } else {
                painter.line_segment([p0, pt], Stroke::new(width, color));
            }
        }
        prev = Some(pt);
    }
}

fn draw_dashed_segment(painter: &Painter, p0: Pos2, p1: Pos2, color: Color32, width: f32) {
    let seg_len = p0.distance(p1);
    if seg_len < 1e-3 {
        return;
    }
    let dir = (p1 - p0) / seg_len;
    let dash = 4.0;
    let gap = 3.0;
    let mut traveled = 0.0;
    let mut a = p0;
    let mut drawing = true;
    let mut dash_left: f32 = dash;
    while traveled < seg_len {
        let remain = seg_len - traveled;
        let step = dash_left.min(remain);
        let b = a + dir * step;
        if drawing {
            painter.line_segment([a, b], Stroke::new(width, color));
        }
        traveled += step;
        a = b;
        if drawing {
            dash_left -= step;
            if dash_left <= 0.0 {
                drawing = false;
                dash_left = gap;
            }
        } else {
            dash_left -= step;
            if dash_left <= 0.0 {
                drawing = true;
                dash_left = dash;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn disabled_curves_match() {
        let settings = SidetoneEnvelopeSettings {
            enabled: false,
            ..SidetoneEnvelopeSettings::default()
        };
        let curves = build_demo_curves(&settings, 12_000.0);
        for (&b, &a) in curves.before.iter().zip(curves.after.iter()) {
            assert!((b - a).abs() < 1e-6, "b={b} a={a}");
        }
    }

    #[test]
    fn enabled_softens_first_keyed_sample() {
        let settings = SidetoneEnvelopeSettings::default();
        let curves = build_demo_curves(&settings, 12_000.0);
        let idx = curves
            .times_ms
            .iter()
            .position(|&t| t >= 70.0)
            .expect("key segment");
        let raw = curves.before[idx].abs();
        let shaped = curves.after[idx].abs();
        assert!(raw > 0.05);
        assert!(shaped < raw * 0.5, "raw={raw} shaped={shaped}");
    }

    #[test]
    fn gain_reaches_unity_during_long_key() {
        let settings = SidetoneEnvelopeSettings::default();
        let curves = build_demo_curves(&settings, 12_000.0);
        let peak_gain = curves.gain.iter().copied().fold(0.0f32, f32::max);
        assert!(peak_gain > 0.85, "peak_gain={peak_gain}");
    }
}
