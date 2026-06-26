//! Analytic magnitude curves for listen-chain IQ filters (UI diagnostics).
//!
//! Main-plot overlays use [`OVERLAY_ATTEN_DB`] half-widths from this module (recomputed
//! when filter width / shape settings change). The diagnostic panel sweeps the full curve.

use std::f32::consts::TAU;

use super::super::biquad::Biquad;
use super::filter_plan::{
    clamp_passband_hz, passband_cutoff_hz, DEFAULT_CHANNEL_PASSBAND_HZ, CHANNEL_PASSBAND_MAX_HZ,
};
use super::fir::{design_lowpass_with, LowpassDesign};
use super::settings::{ChannelFilterKind, CwChannelSettings, MAX_NOTCHES};

/// Main-plot band / notch edge marker — attenuation relative to passband peak.
pub const OVERLAY_ATTEN_DB: f32 = -3.0;

/// Points along the diagnostic curve (offset Hz from listen center).
pub const FILTER_CURVE_POINTS: usize = 256;

/// Cached overlay geometry for the panadapter (rebuilt when filter settings change).
#[derive(Clone, Debug)]
pub struct FilterOverlay {
    /// Channel FIR/IIR half-width at [`OVERLAY_ATTEN_DB`] from listen center (Hz).
    pub channel_half_hz: f32,
    /// Per-notch display half-width at [`OVERLAY_ATTEN_DB`] (0 when disabled).
    pub notch_half_hz: [f32; MAX_NOTCHES],
}

impl Default for FilterOverlay {
    fn default() -> Self {
        Self {
            channel_half_hz: DEFAULT_CHANNEL_PASSBAND_HZ * 0.5,
            notch_half_hz: [0.0; MAX_NOTCHES],
        }
    }
}

/// Magnitude response samples for overlay in the filter diagnostic panel.
#[derive(Clone, Debug)]
pub struct FilterCurve {
    pub offsets_hz: Vec<f32>,
    /// Combined manual notches + channel FIR (when not bypassed).
    pub active_db: Vec<f32>,
    /// Flat 0 dB reference — listen path with channel FIR and notches bypassed.
    pub bypass_db: Vec<f32>,
    /// Channel FIR alone (notches off) for skirt inspection.
    pub channel_only_db: Vec<f32>,
}

/// Inputs for [`build_listen_filter_curves`].
#[derive(Clone, Debug)]
pub struct FilterCurveRequest {
    pub settings: CwChannelSettings,
    pub audio_rate: f32,
    pub span_hz: f32,
}

/// Cache key for [`build_filter_overlay`] — invalidates when filter design changes.
pub fn filter_overlay_cache_key(settings: &CwChannelSettings, audio_rate: f32) -> u64 {
    let mut key = 0u64;
    key ^= settings.passband_hz.to_bits() as u64;
    key ^= (settings.window as u8 as u64) << 8;
    key ^= settings.kaiser_beta.to_bits() as u64;
    key ^= (settings.passband_flatten as u64) << 1;
    key ^= (settings.channel_filter as u8 as u64) << 2;
    key ^= (settings.economy_filter as u64) << 3;
    key ^= (settings.diagnostic.channel_fir as u64) << 4;
    for (i, n) in settings.notches.iter().enumerate() {
        let slot = (i as u64).wrapping_mul(17);
        key ^= (n.enabled as u64) << (slot % 48);
        key ^= n.offset_hz.hz().to_bits() as u64;
        key ^= n.width_hz.to_bits() as u64;
    }
    key ^= audio_rate.to_bits() as u64;
    key
}

/// Build main-plot overlay edges from the current listen-chain filter design.
pub fn build_filter_overlay(settings: &CwChannelSettings, audio_rate: f32) -> FilterOverlay {
    let rate = audio_rate.max(1.0);
    let threshold = db_to_linear(OVERLAY_ATTEN_DB);
    let channel_half_hz = if settings.diagnostic.channel_fir {
        settings.channel_bandwidth_hz() * 0.5
    } else {
        channel_half_width_hz(settings, rate, threshold)
    };
    let mut notch_half_hz = [0.0f32; MAX_NOTCHES];
    for (slot, n) in settings.notches.iter().enumerate() {
        if n.enabled {
            notch_half_hz[slot] = notch_display_half_hz(n.width_hz, rate, threshold);
        }
    }
    FilterOverlay {
        channel_half_hz,
        notch_half_hz,
    }
}

/// Map a dragged channel edge (half-width in Hz) back to GUI passband width.
pub fn passband_hz_for_channel_half(
    target_half_hz: f32,
    settings: &CwChannelSettings,
    audio_rate: f32,
    passband_min_hz: f32,
    passband_max_hz: f32,
) -> f32 {
    let target = target_half_hz.max(1.0);
    let rate = audio_rate.max(1.0);
    let threshold = db_to_linear(OVERLAY_ATTEN_DB);
    let mut lo = passband_min_hz;
    let mut hi = passband_max_hz;
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        let mut probe = settings.clone();
        probe.passband_hz = mid;
        let half = channel_half_width_hz(&probe, rate, threshold);
        if half < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    clamp_passband_hz((lo + hi) * 0.5)
}

/// Map a dragged notch edge (display half-width in Hz) back to notch `width_hz`.
pub fn notch_width_for_display_half(
    target_half_hz: f32,
    audio_rate: f32,
    width_min_hz: f32,
    width_max_hz: f32,
) -> f32 {
    let target = target_half_hz.max(1.0);
    let rate = audio_rate.max(1.0);
    let threshold = db_to_linear(OVERLAY_ATTEN_DB);
    let mut lo = width_min_hz;
    let mut hi = width_max_hz;
    for _ in 0..24 {
        let mid = (lo + hi) * 0.5;
        let half = notch_display_half_hz(mid, rate, threshold);
        if half < target {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    ((lo + hi) * 0.5).clamp(width_min_hz, width_max_hz)
}

pub fn channel_half_width_hz(
    settings: &CwChannelSettings,
    audio_rate: f32,
    threshold_lin: f32,
) -> f32 {
    let bandwidth = settings.channel_bandwidth_hz();
    let max_search = (bandwidth * 1.5).max(CHANNEL_PASSBAND_MAX_HZ * 0.5);
    if settings.effective_channel_filter() == ChannelFilterKind::Iir2Pole {
        let mut bq = Biquad::new();
        bq.set_lowpass(audio_rate, (bandwidth * 0.5).max(10.0), 0.707);
        half_width_where(|f| bq.magnitude_linear(audio_rate, f), max_search, threshold_lin)
    } else {
        let design = LowpassDesign {
            window: settings.window,
            kaiser_beta: settings.kaiser_beta,
            passband_flatten: settings.passband_flatten,
        };
        let taps = design_lowpass_with(audio_rate, bandwidth, design)
            .taps()
            .to_vec();
        half_width_where(
            |f| fir_magnitude_linear(&taps, audio_rate, f),
            max_search,
            threshold_lin,
        )
    }
}

pub fn notch_display_half_hz(width_hz: f32, audio_rate: f32, threshold_lin: f32) -> f32 {
    let alpha = notch_dc_alpha(audio_rate, width_hz);
    let max_search = (width_hz * 4.0).clamp(30.0, 800.0);
    notch_half_width_where(
        |f| dc_block_magnitude_linear(alpha, f, audio_rate),
        max_search,
        threshold_lin,
    )
}

/// Half-width where a **notch-shaped** response (minimum at 0 Hz) reaches `threshold_lin`.
fn notch_half_width_where(mag_at: impl Fn(f32) -> f32, max_hz: f32, threshold_lin: f32) -> f32 {
    if mag_at(max_hz) < threshold_lin {
        return max_hz;
    }
    let mut lo = 0.0f32;
    let mut hi = max_hz;
    for _ in 0..28 {
        let mid = (lo + hi) * 0.5;
        if mag_at(mid) < threshold_lin {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    hi
}

fn half_width_where(mag_at: impl Fn(f32) -> f32, max_hz: f32, threshold_lin: f32) -> f32 {
    if mag_at(0.0) < threshold_lin {
        return 0.0;
    }
    if mag_at(max_hz) >= threshold_lin {
        return max_hz;
    }
    let mut lo = 0.0f32;
    let mut hi = max_hz;
    for _ in 0..28 {
        let mid = (lo + hi) * 0.5;
        if mag_at(mid) >= threshold_lin {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    lo
}

fn db_to_linear(db: f32) -> f32 {
    10f32.powf(db / 20.0)
}

pub fn build_listen_filter_curves(req: &FilterCurveRequest) -> FilterCurve {
    let points = FILTER_CURVE_POINTS;
    let half = (req.span_hz * 0.5).max(50.0);
    let rate = req.audio_rate.max(1.0);
    let settings = &req.settings;
    let bandwidth = settings.channel_bandwidth_hz();

    let design = LowpassDesign {
        window: settings.window,
        kaiser_beta: settings.kaiser_beta,
        passband_flatten: settings.passband_flatten,
    };
    let taps = design_lowpass_with(rate, bandwidth, design)
        .taps()
        .to_vec();

    let mut iir = Biquad::new();
    let use_iir = settings.effective_channel_filter() == ChannelFilterKind::Iir2Pole;
    if use_iir {
        iir.set_lowpass(rate, (bandwidth * 0.5).max(10.0), 0.707);
    }

    let channel_bypass = settings.diagnostic.channel_fir;
    let listen_hz = settings.listen_offset_hz.hz();
    let enabled_notches: Vec<_> = settings
        .notches
        .iter()
        .filter(|n| n.enabled)
        .collect();

    let mut offsets_hz = Vec::with_capacity(points);
    let mut active_db = Vec::with_capacity(points);
    let mut bypass_db = Vec::with_capacity(points);
    let mut channel_only_db = Vec::with_capacity(points);

    for i in 0..points {
        let t = i as f32 / (points - 1).max(1) as f32;
        let offset = -half + t * half * 2.0;
        offsets_hz.push(offset);

        let mut notch_mag = 1.0f32;
        for n in &enabled_notches {
            let tone_hz = listen_hz + offset;
            notch_mag *= notch_magnitude_linear(rate, n.width_hz, tone_hz, n.offset_hz.hz());
        }

        let ch_mag = if channel_bypass {
            1.0
        } else if use_iir {
            iir.magnitude_linear(rate, offset.abs())
        } else {
            fir_magnitude_linear(&taps, rate, offset.abs())
        };

        active_db.push(linear_to_db(notch_mag * ch_mag));
        bypass_db.push(0.0);
        channel_only_db.push(linear_to_db(ch_mag));
    }

    FilterCurve {
        offsets_hz,
        active_db,
        bypass_db,
        channel_only_db,
    }
}

/// Legacy: nominal GUI half-width (±passband/2). Prefer [`FilterOverlay::channel_half_hz`].
pub fn gui_passband_edge_hz(passband_hz: f32) -> f32 {
    passband_hz * 0.5
}

pub fn fir_cutoff_hz(passband_hz: f32) -> f32 {
    passband_cutoff_hz(passband_hz)
}

fn linear_to_db(lin: f32) -> f32 {
    20.0 * lin.max(1e-9).log10()
}

fn fir_magnitude_linear(taps: &[f32], sample_rate: f32, freq_hz: f32) -> f32 {
    let w = TAU * freq_hz / sample_rate;
    let mut re = 0.0f32;
    let mut im = 0.0f32;
    for (k, &h) in taps.iter().enumerate() {
        let phase = -w * k as f32;
        re += h * phase.cos();
        im += h * phase.sin();
    }
    (re * re + im * im).sqrt()
}

fn notch_dc_alpha(sample_rate: f32, width_hz: f32) -> f32 {
    let w = width_hz.clamp(10.0, 500.0);
    (1.0 - 2.0 * TAU * w / sample_rate).clamp(0.5, 0.9999)
}

/// Steerable IQ notch: mix to DC, leaky DC blocker, mix back.
fn notch_magnitude_linear(
    sample_rate: f32,
    width_hz: f32,
    tone_offset_hz: f32,
    notch_offset_hz: f32,
) -> f32 {
    let mixed = tone_offset_hz - notch_offset_hz;
    dc_block_magnitude_linear(notch_dc_alpha(sample_rate, width_hz), mixed, sample_rate)
}

/// |H(e^jω)| for `y = x - state`, `state = α·state + (1-α)·x`.
fn dc_block_magnitude_linear(alpha: f32, freq_hz: f32, sample_rate: f32) -> f32 {
    let w = TAU * freq_hz / sample_rate;
    let c = w.cos();
    let s = w.sin();
    let num_r = alpha * (1.0 - c);
    let num_i = alpha * s;
    let den_r = 1.0 - alpha * c;
    let den_i = alpha * s;
    let den_sq = den_r * den_r + den_i * den_i;
    if den_sq < 1e-20 {
        return 1.0;
    }
    let hr = (num_r * den_r + num_i * den_i) / den_sq;
    let hi = (num_i * den_r - num_r * den_i) / den_sq;
    (hr * hr + hi * hi).sqrt()
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::super::filter_plan::CHANNEL_PASSBAND_MIN_HZ;
    use super::super::super::freq_offset::ChannelOffsetHz;
    use super::super::notch::IqNotch;
    use super::super::settings::NotchSpec;
    use crate::source::Complex32;
    use std::f32::consts::TAU;

    #[test]
    fn fir_passband_near_unity() {
        let rate = 12_000.0;
        let taps = design_lowpass_with(rate, 200.0, LowpassDesign::default())
            .taps()
            .to_vec();
        let mag = fir_magnitude_linear(&taps, rate, 0.0);
        assert!((mag - 1.0).abs() < 0.05, "dc gain {mag}");
    }

    #[test]
    fn notch_analytic_matches_simulation() {
        let rate = 12_000.0;
        let offset = 350.0;
        let width = 80.0;
        let analytic = notch_magnitude_linear(rate, width, offset, offset);
        let mut notch = IqNotch::new();
        notch.sync(rate, width);
        let warm = rate as usize * 2;
        let measure = rate as usize;
        let mut in_pwr = 0.0f32;
        let mut out_pwr = 0.0f32;
        for i in 0..warm + measure {
            let t = i as f32 / rate;
            let phase = TAU * offset * t;
            let s = Complex32::new(phase.cos(), phase.sin());
            let o = notch.process(s, offset, rate);
            if i >= warm {
                in_pwr += s.norm_sqr();
                out_pwr += o.norm_sqr();
            }
        }
        let sim = (out_pwr / in_pwr.max(1e-12)).sqrt();
        assert!(
            (analytic - sim).abs() < 0.15,
            "analytic {analytic} vs sim {sim}"
        );
    }

    #[test]
    fn curve_active_below_bypass_off_frequency() {
        let mut settings = CwChannelSettings::default();
        settings.passband_hz = 200.0;
        settings.notches[0] = NotchSpec {
            enabled: true,
            offset_hz: ChannelOffsetHz::new(300.0),
            width_hz: 80.0,
        };
        let curve = build_listen_filter_curves(&FilterCurveRequest {
            settings,
            audio_rate: 12_000.0,
            span_hz: 2_000.0,
        });
        let center = curve.active_db[curve.active_db.len() / 2];
        let notch_bin = curve
            .offsets_hz
            .iter()
            .position(|&o| (o - 300.0).abs() < 20.0)
            .unwrap();
        assert!(curve.active_db[notch_bin] < center - 6.0);
        assert!((curve.bypass_db[notch_bin] - 0.0).abs() < 0.01);
    }

    #[test]
    fn overlay_half_narrower_than_gui_edge_for_default_bw() {
        let settings = CwChannelSettings::default();
        let overlay = build_filter_overlay(&settings, 12_000.0);
        assert!(overlay.channel_half_hz < gui_passband_edge_hz(settings.passband_hz));
        assert!(overlay.channel_half_hz > fir_cutoff_hz(settings.passband_hz) * 0.5);
    }

    #[test]
    fn passband_inverse_matches_overlay_half() {
        let settings = CwChannelSettings::default();
        let rate = 12_000.0;
        let overlay = build_filter_overlay(&settings, rate);
        let solved = passband_hz_for_channel_half(
            overlay.channel_half_hz,
            &settings,
            rate,
            CHANNEL_PASSBAND_MIN_HZ,
            CHANNEL_PASSBAND_MAX_HZ,
        );
        let check = build_filter_overlay(
            &CwChannelSettings {
                passband_hz: solved,
                ..settings.clone()
            },
            rate,
        );
        assert!((check.channel_half_hz - overlay.channel_half_hz).abs() < 2.0);
    }

    #[test]
    fn notch_display_half_is_nonzero_for_typical_width() {
        let rate = 12_000.0;
        let half = notch_display_half_hz(80.0, rate, db_to_linear(OVERLAY_ATTEN_DB));
        assert!(
            half > 20.0,
            "notch overlay half-width should span a visible band, got {half} Hz"
        );
    }

    #[test]
    fn notch_width_inverse_matches_display_half() {
        let rate = 12_000.0;
        let width = 80.0;
        let half = notch_display_half_hz(width, rate, db_to_linear(OVERLAY_ATTEN_DB));
        let solved = notch_width_for_display_half(half, rate, 10.0, 500.0);
        let half2 = notch_display_half_hz(solved, rate, db_to_linear(OVERLAY_ATTEN_DB));
        assert!((half2 - half).abs() < 2.0);
    }

    #[test]
    fn gui_edge_wider_than_fir_cutoff() {
        let pb = 200.0;
        assert!(gui_passband_edge_hz(pb) > fir_cutoff_hz(pb));
    }
}
