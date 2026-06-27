//! Filter diagnostic panel — live spectrum vs channel-filtered (panel open only).

use eframe::egui::{
    self, Align2, Color32, FontId, Painter, Pos2, Rect, RichText, Sense, Stroke, Ui, Vec2,
};

use hfsdr::{
    build_listen_filter_curves, fir_cutoff_hz, offset_hz_to_view_t, FilterCurve, FilterCurveRequest,
    CwChannelSettings,
};

use crate::theme::{MUTED, OK, WARN};

const CURVE_POINTS: usize = 256;

/// Cached analytic channel response (settings-driven).
#[derive(Clone, Debug, Default)]
pub struct FilterDiagnosticState {
    theory: Option<FilterCurve>,
    theory_key: u64,
}

/// Live + analytic samples for the diagnostic plot.
#[derive(Clone, Debug)]
struct DiagnosticCurves {
    offsets_hz: Vec<f32>,
    signal_db: Vec<f32>,
    filtered_db: Vec<f32>,
    /// Analytic channel FIR/IIR magnitude (0 dB at passband peak).
    theory_channel_db: Vec<f32>,
    /// Analytic channel + manual notches magnitude.
    theory_active_db: Vec<f32>,
}

impl FilterDiagnosticState {
    fn theory(
        &mut self,
        settings: &CwChannelSettings,
        audio_rate: f32,
        span_hz: f32,
    ) -> &FilterCurve {
        let key = theory_cache_key(settings, audio_rate, span_hz);
        if self.theory_key != key || self.theory.is_none() {
            self.theory = Some(build_listen_filter_curves(&FilterCurveRequest {
                settings: settings.clone(),
                audio_rate,
                span_hz,
            }));
            self.theory_key = key;
        }
        self.theory.as_ref().expect("theory just built")
    }
}

fn theory_cache_key(settings: &CwChannelSettings, audio_rate: f32, span_hz: f32) -> u64 {
    let mut key = 0u64;
    key ^= settings.passband_hz.to_bits() as u64;
    key ^= (settings.window as u8 as u64) << 8;
    key ^= settings.kaiser_beta.to_bits() as u64;
    key ^= (settings.passband_flatten as u64) << 1;
    key ^= (settings.channel_filter as u8 as u64) << 2;
    key ^= (settings.iir_filter as u8 as u64) << 5;
    key ^= (settings.economy_filter as u64) << 3;
    key ^= (settings.diagnostic.channel_fir as u64) << 4;
    for (i, n) in settings.notches.iter().enumerate() {
        let slot = (i as u64).wrapping_mul(17);
        key ^= (n.enabled as u64) << (slot % 48);
        key ^= n.offset_hz.hz().to_bits() as u64;
        key ^= n.width_hz.to_bits() as u64;
    }
    key ^= audio_rate.to_bits() as u64;
    key ^= span_hz.to_bits() as u64;
    key
}

pub struct FilterDiagnosticView<'a> {
    pub settings: &'a CwChannelSettings,
    pub audio_rate: f32,
    pub span_hz: f32,
    pub channel_half_hz: f32,
    pub channel_bypass: bool,
    /// Composed scope trace (same mapping as the panadapter).
    pub trace_db: &'a [f32],
    pub trace_view_span_hz: f32,
    pub trace_pan_hz: f64,
    pub listen_offset_hz: f64,
    pub filter_shift_hz: f64,
    pub ref_db: f32,
    pub range_db: f32,
    pub streaming: bool,
}

pub fn show_filter_diagnostic_panel(
    ui: &mut Ui,
    state: &mut FilterDiagnosticState,
    view: &FilterDiagnosticView<'_>,
) {
    ui.label(
        RichText::new(
            "Live panadapter spectrum vs the same signal after the CW channel filter \
             (current BW, window, and architecture). Updates with the scope trace.",
        )
        .small()
        .color(MUTED),
    );
    ui.add_space(6.0);

    let cutoff = fir_cutoff_hz(view.settings.passband_hz);
    let window = window_label(view.settings);
    ui.horizontal(|ui| {
        ui.label(
            RichText::new(format!(
                "Main plot ±{:.0} Hz (−3 dB) · {:.0} Hz BW · {window}",
                view.channel_half_hz,
                view.settings.passband_hz,
            ))
            .small()
            .color(Color32::from_rgb(125, 211, 252)),
        );
        ui.label(RichText::new("·").small().color(MUTED));
        ui.label(
            RichText::new(format!("FIR cutoff {cutoff:.0} Hz"))
                .small()
                .color(MUTED),
        );
        if view.channel_bypass {
            ui.label(RichText::new("· channel FIR bypassed").small().color(WARN));
        }
    });
    ui.add_space(4.0);

    ui.horizontal(|ui| {
        legend_dot(ui, Color32::from_rgb(148, 163, 184), "Spectrum (unfiltered)");
        legend_dot(ui, OK, "After channel filter + notches");
        legend_dot(
            ui,
            Color32::from_rgb(125, 211, 252),
            "Filter magnitude (theory)",
        );
    });
    ui.add_space(6.0);

    let theory = state.theory(view.settings, view.audio_rate, view.span_hz);
    let live = build_live_curves(view, theory);
    let plot_h = 220.0;
    let (rect, _resp) = ui.allocate_exact_size(
        Vec2::new(ui.available_width().max(320.0), plot_h),
        Sense::hover(),
    );
    paint_filter_curve(
        &ui.painter_at(rect),
        rect,
        &live,
        view,
    );

    ui.add_space(4.0);
    if !view.streaming {
        ui.label(
            RichText::new("Connect and tune — comparison uses the live scope trace.")
                .small()
                .color(MUTED),
        );
    } else if live_peak(&live.signal_db) < view.ref_db - view.range_db + 6.0 {
        ui.label(
            RichText::new("Low signal in this span — widen view or tune a carrier.")
                .small()
                .color(MUTED),
        );
    } else {
        ui.label(
            RichText::new(
                "Hz offset from listen center — change BW / window in DSP panel; \
                 main plot −3 dB edges update with filter width.",
            )
            .small()
            .color(MUTED),
        );
    }
}

fn window_label(settings: &CwChannelSettings) -> &'static str {
    use hfsdr::{ChannelFilterKind, IirFilterKind, WindowKind};
    if settings.economy_filter || settings.effective_channel_filter() == ChannelFilterKind::Iir2Pole {
        return match settings.iir_filter {
            IirFilterKind::Chebyshev => "Chebyshev IIR",
            IirFilterKind::Butterworth => "Butterworth IIR",
        };
    }
    match settings.window {
        WindowKind::Gaussian => "Gaussian FIR",
        WindowKind::RaisedCosine => "RaisedCos FIR",
        WindowKind::Blackman => "Blackman FIR",
        WindowKind::Kaiser => "Kaiser FIR",
    }
}

fn live_peak(db: &[f32]) -> f32 {
    db.iter().copied().fold(f32::NEG_INFINITY, f32::max)
}

fn build_live_curves(view: &FilterDiagnosticView<'_>, theory: &FilterCurve) -> DiagnosticCurves {
    let half = (view.span_hz * 0.5).max(50.0);
    let mut offsets_hz = Vec::with_capacity(CURVE_POINTS);
    let mut signal_db = Vec::with_capacity(CURVE_POINTS);
    let mut filtered_db = Vec::with_capacity(CURVE_POINTS);
    let mut theory_channel_db = Vec::with_capacity(CURVE_POINTS);
    let mut theory_active_db = Vec::with_capacity(CURVE_POINTS);

    for i in 0..CURVE_POINTS {
        let t = i as f32 / (CURVE_POINTS - 1).max(1) as f32;
        let rel = -half + t * half * 2.0;
        offsets_hz.push(rel);

        let rx_offset = view.listen_offset_hz + rel as f64;
        let raw = sample_trace_db(
            view.trace_db,
            rx_offset,
            view.trace_view_span_hz,
            view.trace_pan_hz,
        );
        let filter_rel = view.filter_shift_hz as f32;
        let ch_db = interp_curve_db(
            &theory.offsets_hz,
            &theory.channel_only_db,
            rel - filter_rel,
        );
        let active_db_at = interp_curve_db(&theory.offsets_hz, &theory.active_db, rel);
        let ch_db_at_listen = interp_curve_db(
            &theory.offsets_hz,
            &theory.channel_only_db,
            rel,
        );
        let notch_lin = db_to_linear(active_db_at) / db_to_linear(ch_db_at_listen).max(1e-9);
        let combined_db = ch_db + linear_to_db(notch_lin);
        signal_db.push(raw);
        filtered_db.push(if view.channel_bypass {
            raw
        } else {
            raw + combined_db
        });
        theory_channel_db.push(
            view.ref_db
                + interp_curve_db(&theory.offsets_hz, &theory.channel_only_db, rel),
        );
        theory_active_db.push(
            view.ref_db + interp_curve_db(&theory.offsets_hz, &theory.active_db, rel),
        );
    }

    DiagnosticCurves {
        offsets_hz,
        signal_db,
        filtered_db,
        theory_channel_db,
        theory_active_db,
    }
}

fn linear_to_db(lin: f32) -> f32 {
    20.0 * lin.max(1e-9).log10()
}

fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

fn sample_trace_db(
    trace: &[f32],
    rx_offset_hz: f64,
    view_span_hz: f32,
    pan_hz: f64,
) -> f32 {
    if trace.is_empty() {
        return -120.0;
    }
    let t = offset_hz_to_view_t(rx_offset_hz, view_span_hz, pan_hz);
    let idx = t * (trace.len() - 1) as f64;
    let i0 = idx.floor() as usize;
    let i1 = (i0 + 1).min(trace.len() - 1);
    let frac = (idx - i0 as f64) as f32;
    trace[i0] * (1.0 - frac) + trace[i1] * frac
}

fn interp_curve_db(offsets: &[f32], values: &[f32], x: f32) -> f32 {
    if offsets.is_empty() || values.len() != offsets.len() {
        return 0.0;
    }
    if x <= offsets[0] {
        return values[0];
    }
    let last = offsets.len() - 1;
    if x >= offsets[last] {
        return values[last];
    }
    for i in 0..last {
        let x0 = offsets[i];
        let x1 = offsets[i + 1];
        if x0 <= x && x <= x1 {
            let t = (x - x0) / (x1 - x0).max(1e-6);
            return values[i] * (1.0 - t) + values[i + 1] * t;
        }
    }
    values[last]
}

fn legend_dot(ui: &mut Ui, color: Color32, label: &str) {
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(Vec2::splat(8.0), Sense::hover());
        ui.painter().circle_filled(r.center(), 4.0, color);
        ui.label(RichText::new(label).small().color(MUTED));
    });
    ui.add_space(8.0);
}

fn paint_filter_curve(painter: &Painter, rect: Rect, live: &DiagnosticCurves, view: &FilterDiagnosticView<'_>) {
    painter.rect_filled(rect, 6.0, Color32::from_rgb(12, 16, 24));
    painter.rect_stroke(
        rect,
        6.0,
        Stroke::new(1.0, Color32::from_rgb(40, 48, 64)),
        egui::StrokeKind::Inside,
    );

    let margin_l = 36.0;
    let margin_r = 8.0;
    let margin_t = 8.0;
    let margin_b = 22.0;
    let inner = Rect::from_min_max(
        Pos2::new(rect.left() + margin_l, rect.top() + margin_t),
        Pos2::new(rect.right() - margin_r, rect.bottom() - margin_b),
    );
    if inner.width() < 8.0 || inner.height() < 8.0 {
        return;
    }

    let half_span = live
        .offsets_hz
        .last()
        .copied()
        .unwrap_or(1000.0)
        .abs()
        .max(50.0);

    let ref_db = view.ref_db;
    let floor_db = ref_db - view.range_db.max(1.0);
    for step in 0..4 {
        let db = ref_db - view.range_db * step as f32 / 3.0;
        let y = db_to_y(db, inner, ref_db, view.range_db);
        painter.line_segment(
            [Pos2::new(inner.left(), y), Pos2::new(inner.right(), y)],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(60, 70, 90, 80)),
        );
        painter.text(
            Pos2::new(rect.left() + 4.0, y),
            Align2::LEFT_CENTER,
            format!("{db:.0}"),
            FontId::monospace(9.0),
            MUTED,
        );
    }
    let _ = floor_db;

    let filter_shift = view.filter_shift_hz as f32;
    for off in [
        filter_shift - view.channel_half_hz,
        filter_shift + view.channel_half_hz,
    ] {
        let x = offset_to_x(off, half_span, inner);
        painter.line_segment(
            [Pos2::new(x, inner.top()), Pos2::new(x, inner.bottom())],
            Stroke::new(
                1.5,
                Color32::from_rgba_unmultiplied(125, 211, 252, 180),
            ),
        );
    }

    let overlay = hfsdr::build_filter_overlay(view.settings, view.audio_rate);
    let listen_hz = view.listen_offset_hz as f32;
    for (slot, n) in view.settings.notches.iter().enumerate().filter(|(_, n)| n.enabled) {
        let half = overlay.notch_half_hz[slot];
        let center = n.offset_hz.hz() - listen_hz;
        let left = offset_to_x(center - half, half_span, inner);
        let right = offset_to_x(center + half, half_span, inner);
        let band = Rect::from_min_max(
            Pos2::new(left, inner.top()),
            Pos2::new(right, inner.bottom()),
        );
        painter.rect_filled(
            band,
            0.0,
            Color32::from_rgba_unmultiplied(192, 132, 252, 18),
        );
        let cx = offset_to_x(center, half_span, inner);
        painter.line_segment(
            [Pos2::new(cx, inner.top()), Pos2::new(cx, inner.bottom())],
            Stroke::new(1.0, Color32::from_rgba_unmultiplied(192, 132, 252, 120)),
        );
    }

    draw_dashed_curve_line(
        painter,
        inner,
        half_span,
        &live.theory_channel_db,
        &live.offsets_hz,
        ref_db,
        view.range_db,
        Color32::from_rgba_unmultiplied(125, 211, 252, 200),
        1.5,
    );
    if view.settings.notches.iter().any(|n| n.enabled) {
        draw_dashed_curve_line(
            painter,
            inner,
            half_span,
            &live.theory_active_db,
            &live.offsets_hz,
            ref_db,
            view.range_db,
            Color32::from_rgba_unmultiplied(192, 132, 252, 170),
            1.25,
        );
    }

    draw_curve_line(
        painter,
        inner,
        half_span,
        &live.signal_db,
        &live.offsets_hz,
        ref_db,
        view.range_db,
        Color32::from_rgba_unmultiplied(148, 163, 184, 220),
        1.75,
    );
    draw_curve_line(
        painter,
        inner,
        half_span,
        &live.filtered_db,
        &live.offsets_hz,
        ref_db,
        view.range_db,
        Color32::from_rgba_unmultiplied(OK.r(), OK.g(), OK.b(), 240),
        2.0,
    );

    painter.text(
        Pos2::new(inner.center().x, rect.bottom() - 4.0),
        Align2::CENTER_BOTTOM,
        "Hz from listen",
        FontId::proportional(9.0),
        MUTED,
    );
    painter.text(
        Pos2::new(inner.center().x, inner.top() - 2.0),
        Align2::CENTER_BOTTOM,
        "dB",
        FontId::proportional(9.0),
        MUTED,
    );
}

fn draw_dashed_curve_line(
    painter: &Painter,
    inner: Rect,
    half_span: f32,
    db: &[f32],
    offsets: &[f32],
    ref_db: f32,
    range_db: f32,
    color: Color32,
    width: f32,
) {
    if db.len() < 2 || db.len() != offsets.len() {
        return;
    }
    let dash = 5.0f32;
    let gap = 4.0f32;
    let mut prev: Option<Pos2> = None;
    let mut dash_left = dash;
    let mut drawing = true;
    for (&off, &val) in offsets.iter().zip(db.iter()) {
        let pt = Pos2::new(
            offset_to_x(off, half_span, inner),
            db_to_y(val, inner, ref_db, range_db),
        );
        if let Some(p0) = prev {
            let seg_len = p0.distance(pt);
            if seg_len > 1e-3 {
                let dir = (pt - p0) / seg_len;
                let mut traveled = 0.0;
                let mut a = p0;
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
        }
        prev = Some(pt);
    }
}

fn draw_curve_line(
    painter: &Painter,
    inner: Rect,
    half_span: f32,
    db: &[f32],
    offsets: &[f32],
    ref_db: f32,
    range_db: f32,
    color: Color32,
    width: f32,
) {
    if db.len() < 2 || db.len() != offsets.len() {
        return;
    }
    let mut prev: Option<Pos2> = None;
    for (&off, &val) in offsets.iter().zip(db.iter()) {
        let pt = Pos2::new(
            offset_to_x(off, half_span, inner),
            db_to_y(val, inner, ref_db, range_db),
        );
        if let Some(p0) = prev {
            painter.line_segment([p0, pt], Stroke::new(width, color));
        }
        prev = Some(pt);
    }
}

fn offset_to_x(offset_hz: f32, half_span: f32, inner: Rect) -> f32 {
    let t = ((offset_hz + half_span) / (2.0 * half_span)).clamp(0.0, 1.0);
    inner.left() + inner.width() * t
}

fn db_to_y(db: f32, inner: Rect, ref_db: f32, range_db: f32) -> f32 {
    let t = ((ref_db - db) / range_db.max(1.0)).clamp(0.0, 1.0);
    inner.top() + inner.height() * t
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_trace_interpolates_bins() {
        let trace = vec![-80.0, -60.0, -40.0];
        let mid = sample_trace_db(&trace, 0.0, 12_000.0, 0.0);
        assert!(mid > -70.0 && mid < -50.0);
    }

    #[test]
    fn filtered_curve_applies_channel_db() {
        let settings = CwChannelSettings::default();
        let theory = build_listen_filter_curves(&FilterCurveRequest {
            settings: settings.clone(),
            audio_rate: 12_000.0,
            span_hz: 1_000.0,
        });
        let view = FilterDiagnosticView {
            settings: &settings,
            audio_rate: 12_000.0,
            span_hz: 1_000.0,
            channel_half_hz: 80.0,
            channel_bypass: false,
            trace_db: &[-50.0; 64],
            trace_view_span_hz: 12_000.0,
            trace_pan_hz: 0.0,
            listen_offset_hz: 0.0,
            filter_shift_hz: 0.0,
            ref_db: -50.0,
            range_db: 80.0,
            streaming: true,
        };
        let live = build_live_curves(&view, &theory);
        let center = live.filtered_db[CURVE_POINTS / 2];
        let edge = live.filtered_db[CURVE_POINTS - 1];
        assert!(center > edge + 3.0);
        assert_eq!(live.theory_channel_db.len(), CURVE_POINTS);
        assert!(live.theory_channel_db[CURVE_POINTS / 2] > live.theory_channel_db[0] + 3.0);
    }
}
