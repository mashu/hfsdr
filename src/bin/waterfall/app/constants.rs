//! Layout presets, band plans, and waterfall texture cache keys.

use hfsdr::{CHANNEL_PASSBAND_MAX_HZ, CHANNEL_PASSBAND_MIN_HZ, CwSideband};

/// Minimum RX panel width (VFO digit wheels + section margins).
pub(crate) const LEFT_PANEL_MIN_W: f32 = 288.0;
pub(crate) const LEFT_PANEL_MAX_W: f32 = 440.0;
/// Minimum DSP panel width (AF scope, stage toggles, labeled sliders).
pub(crate) const RIGHT_PANEL_MIN_W: f32 = 252.0;
pub(crate) const RIGHT_PANEL_MAX_W: f32 = 420.0;
/// Resizable bottom panels (log console, spots history).
pub(crate) const BOTTOM_PANEL_MIN_H: f32 = 72.0;
pub(crate) const BOTTOM_PANEL_MAX_H: f32 = 520.0;

/// CW band plan: calling frequency + typical CW segment width for panadapter zoom.
pub(crate) struct CwBandPreset {
    pub(crate) label: &'static str,
    pub(crate) center_hz: f64,
    pub(crate) segment_hz: f32,
    /// Typical CW sideband on this band (IARU Region 1 band plan).
    pub(crate) default_sideband: CwSideband,
}

pub(crate) const CW_HF_BAND_PRESETS: [CwBandPreset; 10] = [
    CwBandPreset { label: "160m", center_hz: 1_810_000.0, segment_hz: 30_000.0, default_sideband: CwSideband::Lower },
    CwBandPreset { label: "80m", center_hz: 3_510_000.0, segment_hz: 80_000.0, default_sideband: CwSideband::Upper },
    CwBandPreset { label: "60m", center_hz: 5_354_000.0, segment_hz: 56_000.0, default_sideband: CwSideband::Lower },
    CwBandPreset { label: "40m", center_hz: 7_010_000.0, segment_hz: 40_000.0, default_sideband: CwSideband::Upper },
    CwBandPreset { label: "30m", center_hz: 10_110_000.0, segment_hz: 40_000.0, default_sideband: CwSideband::Lower },
    CwBandPreset { label: "20m", center_hz: 14_010_000.0, segment_hz: 70_000.0, default_sideband: CwSideband::Lower },
    CwBandPreset { label: "17m", center_hz: 18_080_000.0, segment_hz: 43_000.0, default_sideband: CwSideband::Lower },
    CwBandPreset { label: "15m", center_hz: 21_010_000.0, segment_hz: 70_000.0, default_sideband: CwSideband::Lower },
    CwBandPreset { label: "12m", center_hz: 24_900_000.0, segment_hz: 40_000.0, default_sideband: CwSideband::Lower },
    CwBandPreset { label: "10m", center_hz: 28_010_000.0, segment_hz: 70_000.0, default_sideband: CwSideband::Lower },
];

/// VHF and up — separate from HF so the band grid matches the band plan.
pub(crate) const CW_VHF_BAND_PRESETS: [CwBandPreset; 1] = [
    CwBandPreset { label: "6m", center_hz: 50_090_000.0, segment_hz: 100_000.0, default_sideband: CwSideband::Lower },
];

pub(crate) const DEFAULT_CENTER_HZ: f64 = 14_010_000.0;

/// CW sideband from the band plan for `center_hz`, or CW-L when off-band.
pub(crate) fn cw_sideband_for_center(center_hz: f64) -> CwSideband {
    CW_HF_BAND_PRESETS
        .iter()
        .chain(CW_VHF_BAND_PRESETS.iter())
        .find(|band| (center_hz - band.center_hz).abs() < 25_000.0)
        .map(|band| band.default_sideband)
        .unwrap_or(CwSideband::Lower)
}

pub(crate) const BFO_PRESETS: [(&str, f32); 5] =
    [("400", 400.0), ("450", 450.0), ("500", 500.0), ("600", 600.0), ("700", 700.0)];

pub(crate) const SMOOTH_ALPHA: f32 = 0.09;

pub(crate) const FILTER_PRESETS: [(&str, f32); 8] = [
    ("25", CHANNEL_PASSBAND_MIN_HZ),
    ("50", 50.0),
    ("100", 100.0),
    ("200", 200.0),
    ("250", 250.0),
    ("500", 500.0),
    ("1k", 1_000.0),
    ("2k", CHANNEL_PASSBAND_MAX_HZ),
];

pub(crate) const KIWI_IQ_RATE_PRESETS: &[(&str, u32)] = &[
    ("12 kHz (default)", 12_000),
    ("20.25 kHz (3-ch)", 20_250),
];

pub(crate) const KIWI_BW_PRESETS: &[(&str, u32)] = &[
    ("Full (max)", 0),
    ("±5 kHz", 5_000),
    ("±3 kHz", 3_000),
    ("±2.5 kHz", 2_500),
];

pub(crate) const KIWI_RESAMPLE_PRESETS: &[(&str, u32)] = &[
    ("None (native)", 0),
    ("12 kHz", 12_000),
    ("8 kHz", 8_000),
    ("6 kHz", 6_000),
    ("4.8 kHz", 4_800),
];

pub(crate) const KIWI_LO_PRESETS: &[(&str, f64)] = &[
    ("None", 0.0),
    ("9.75 MHz", 9_750.0),
    ("10.0 MHz", 10_000.0),
    ("10.45 MHz", 10_450.0),
    ("144 MHz", 144_000.0),
];

pub(crate) const KIWI_AR_OUT_PRESETS: &[(&str, u32)] = &[
    ("44.1 kHz", 44_100),
    ("48 kHz", 48_000),
    ("96 kHz", 96_000),
];

#[cfg(feature = "airspy")]
pub(crate) const AIRSPY_SAMPLE_RATE_PRESETS: &[(&str, u32)] = &[
    ("384 kHz (recommended)", 384_000),
    ("768 kHz", 768_000),
    ("192 kHz", 192_000),
    ("96 kHz", 96_000),
    ("48 kHz", 48_000),
    ("24 kHz", 24_000),
    ("12 kHz", 12_000),
];

#[cfg(feature = "rtlsdr")]
pub(crate) const RTLSDR_SAMPLE_RATE_PRESETS: &[(&str, u32)] = &[
    ("2.048 MHz (recommended)", 2_048_000),
    ("2.4 MHz", 2_400_000),
    ("1.92 MHz", 1_920_000),
    ("1.024 MHz", 1_024_000),
    ("960 kHz", 960_000),
    ("320 kHz", 320_000),
    ("250 kHz", 250_000),
];

#[cfg(feature = "rtlsdr")]
pub(crate) const RTLSDR_PROCESS_RATE_PRESETS: &[(&str, u32)] = &[
    ("Native (full rate)", 0),
    ("96 kHz", 96_000),
    ("48 kHz", 48_000),
    ("24 kHz", 24_000),
    ("12 kHz", 12_000),
];

#[cfg(feature = "soapy")]
pub(crate) const SOAPY_DRIVER_PRESETS: &[&str] = &[
    "rtlsdr",
    "airspyhf",
    "hackrf",
    "plutosdr",
    "lime",
    "uhd",
    "sdrplay",
];

#[cfg(feature = "soapy")]
pub(crate) const SOAPY_SAMPLE_RATE_PRESETS: &[(&str, u32)] = &[
    ("2.048 MHz", 2_048_000),
    ("1.92 MHz", 1_920_000),
    ("768 kHz", 768_000),
    ("384 kHz", 384_000),
    ("250 kHz", 250_000),
    ("192 kHz", 192_000),
    ("96 kHz", 96_000),
    ("48 kHz", 48_000),
];

#[cfg(feature = "soapy")]
pub(crate) const SOAPY_PROCESS_RATE_PRESETS: &[(&str, u32)] = &[
    ("Native (full rate)", 0),
    ("192 kHz", 192_000),
    ("96 kHz", 96_000),
    ("48 kHz", 48_000),
    ("24 kHz", 24_000),
    ("12 kHz", 12_000),
];

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct StorageKey {
    tex_width: u32,
    storage_span_hz: u32,
    row_rate_hz: u32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct ViewportKey {
    view_span_hz: u32,
    /// Pan in whole destination columns — see [`quantized_pan_steps`].
    pan_bits: i64,
    plot_width: u32,
}

impl ViewportKey {
    pub(crate) fn from_view(view_span_hz: f32, pan_offset_hz: f64, plot_width: usize) -> Self {
        Self {
            view_span_hz: view_span_hz.round() as u32,
            pan_bits: quantized_pan_steps(view_span_hz, pan_offset_hz, plot_width),
            plot_width: plot_width as u32,
        }
    }
}

/// Pan offset quantized to whole destination columns.
///
/// A pan smaller than one column cannot change a single waterfall pixel, so it
/// must not invalidate the viewport key — an exact f64 comparison here forces a
/// full 360-row recompose on every frame of a drag (10-17 ms at 1200-1920 px)
/// instead of the ~0.1 ms incremental append. Same reasoning as
/// [`stable_plot_width`], applied to the other axis.
pub(crate) fn quantized_pan_steps(
    view_span_hz: f32,
    pan_offset_hz: f64,
    plot_width: usize,
) -> i64 {
    let width = plot_width.max(1) as f64;
    let hz_per_column = (view_span_hz.max(0.0) as f64) / width;
    if !(hz_per_column > 0.0) || !pan_offset_hz.is_finite() {
        return 0;
    }
    (pan_offset_hz / hz_per_column).round() as i64
}

/// Quantize plot width so minor egui layout jitter does not rebuild the waterfall texture every frame.
pub(crate) fn stable_plot_width(raw: f32) -> usize {
    let w = raw.round().max(1.0) as usize;
    w.div_ceil(8) * 8
}

/// Max scroll credit (in rows) banked after a UI hitch — about two frames worth.
pub(crate) const WATERFALL_SCROLL_CREDIT_CAP_MULT: f32 = 2.0;

/// How many undisplayed FFT rows to keep before dropping oldest (stay near live).
pub(crate) fn waterfall_pending_cap(target_fps: u32, rows_per_frame: usize) -> usize {
    (target_fps.max(1) as usize * rows_per_frame.max(1) * 3).max(8)
}

/// Time-paced rows to paint this sync — decouples scroll from bursty engine delivery.
pub(crate) fn waterfall_scroll_rows_due(
    pending: usize,
    rows_per_frame: usize,
    target_fps: u32,
    dt_secs: f32,
    credit: f32,
) -> (usize, f32) {
    if pending == 0 {
        return (0, 0.0);
    }
    let per_frame = rows_per_frame.max(1);
    let rate = target_fps.max(1) as f32 * per_frame as f32;
    let max_credit = per_frame as f32 * WATERFALL_SCROLL_CREDIT_CAP_MULT;
    let mut credit = (credit + dt_secs.max(0.0) * rate).min(max_credit);
    let n = (credit.floor() as usize)
        .min(pending)
        .min(per_frame);
    if n > 0 {
        credit -= n as f32;
    }
    (n, credit)
}

#[cfg(feature = "airspy")]
pub(crate) const AIRSPY_PROCESS_RATE_PRESETS: &[(&str, u32)] = &[
    ("48 kHz (recommended)", 48_000),
    ("Native (full bandwidth)", 0),
    ("96 kHz", 96_000),
    ("192 kHz", 192_000),
];

#[cfg(feature = "qmx")]
pub(crate) const QMX_PROCESS_RATE_PRESETS: &[(&str, u32)] = &[
    ("24 kHz (recommended)", 24_000),
    ("Native (48 kHz)", 0),
    ("12 kHz", 12_000),
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stable_plot_width_quantizes_to_eight_pixels() {
        assert_eq!(stable_plot_width(1201.0), 1208);
        assert_eq!(stable_plot_width(1200.0), 1200);
    }

    /// Regression guard for the drag-pan cliff: a pan too small to move a pixel
    /// must keep the viewport key, or every drag frame forces a full recompose.
    #[test]
    fn sub_column_pan_preserves_viewport_key() {
        let span = 12_000.0f32;
        let width = 1200usize;
        let hz_per_column = f64::from(span) / width as f64;

        let base = ViewportKey::from_view(span, 0.0, width);
        let tenth = ViewportKey::from_view(span, hz_per_column * 0.1, width);
        let third = ViewportKey::from_view(span, hz_per_column * 0.3, width);
        assert_eq!(base, tenth, "a tenth-column pan cannot change a pixel");
        assert_eq!(base, third, "a third-column pan cannot change a pixel");

        let one = ViewportKey::from_view(span, hz_per_column * 1.0, width);
        let two = ViewportKey::from_view(span, hz_per_column * 2.0, width);
        assert_ne!(base, one, "a whole-column pan must invalidate");
        assert_ne!(one, two, "successive column pans stay distinct");
    }

    #[test]
    fn quantized_pan_steps_counts_columns_and_survives_degenerate_input() {
        let span = 9_600.0f32;
        let width = 960usize; // 10 Hz per column
        assert_eq!(quantized_pan_steps(span, 0.0, width), 0);
        assert_eq!(quantized_pan_steps(span, 10.0, width), 1);
        assert_eq!(quantized_pan_steps(span, -30.0, width), -3);
        assert_eq!(quantized_pan_steps(span, 4.0, width), 0, "rounds to nearest");
        // Degenerate inputs must not produce a NaN or panic key.
        assert_eq!(quantized_pan_steps(0.0, 100.0, width), 0);
        assert_eq!(quantized_pan_steps(span, f64::NAN, width), 0);
        assert_eq!(quantized_pan_steps(span, 10.0, 0), 0);
    }

    /// Quantifies the phase-2 win across drag speeds: how many frames of a drag
    /// keep the incremental append path instead of forcing a full recompose.
    #[test]
    fn drag_rebuild_rate_by_speed() {
        let span = 12_000.0f32;
        let width = 1200usize;
        let hz_per_column = f64::from(span) / width as f64;
        let frames = 60;

        for &columns_per_frame in &[0.05f64, 0.25, 0.5, 1.0, 3.0] {
            let mut rebuilds = 0usize;
            let mut prev = ViewportKey::from_view(span, 0.0, width);
            for f in 1..=frames {
                let pan = hz_per_column * columns_per_frame * f as f64;
                let key = ViewportKey::from_view(span, pan, width);
                if key != prev {
                    rebuilds += 1;
                    prev = key;
                }
            }
            println!(
                "drag {columns_per_frame:>4} col/frame -> {rebuilds:>2}/{frames} rebuilds                  ({:.0}% frames on the fast path)",
                100.0 * (frames - rebuilds) as f64 / frames as f64
            );
            // Rebuilds can never exceed the columns actually crossed.
            let crossed = (columns_per_frame * frames as f64).ceil() as usize;
            assert!(rebuilds <= crossed.min(frames));
        }

        // A slow, precise drag is the case this fix exists for.
        let mut rebuilds = 0usize;
        let mut prev = ViewportKey::from_view(span, 0.0, width);
        for f in 1..=frames {
            let key = ViewportKey::from_view(span, hz_per_column * 0.25 * f as f64, width);
            if key != prev {
                rebuilds += 1;
                prev = key;
            }
        }
        assert!(
            rebuilds <= frames / 3,
            "a quarter-column-per-frame drag should skip most rebuilds, got {rebuilds}/{frames}"
        );
    }

    #[test]
    fn waterfall_scroll_rows_due_paces_by_time() {
        assert_eq!(waterfall_scroll_rows_due(0, 1, 30, 0.033, 0.0), (0, 0.0));
        assert_eq!(waterfall_scroll_rows_due(10, 1, 30, 0.033, 0.0), (0, 0.99));
        assert_eq!(waterfall_scroll_rows_due(10, 1, 30, 0.034, 0.99), (1, 1.0));
        let (n, credit) = waterfall_scroll_rows_due(10, 4, 15, 0.066, 0.0);
        assert_eq!(n, 3);
        assert!((credit - 0.96).abs() < 0.001);
        let (n2, credit2) = waterfall_scroll_rows_due(10, 4, 15, 0.001, 0.96);
        assert_eq!(n2, 1);
        assert!((credit2 - 0.02).abs() < 0.001);
    }

    #[test]
    fn waterfall_pending_cap_scales_with_fps() {
        assert_eq!(waterfall_pending_cap(15, 1), 45);
        assert_eq!(waterfall_pending_cap(10, 2), 60);
    }

    #[test]
    fn band_plan_sideband_defaults() {
        use hfsdr::CwSideband;
        assert_eq!(cw_sideband_for_center(14_010_000.0), CwSideband::Lower);
        assert_eq!(cw_sideband_for_center(7_010_000.0), CwSideband::Upper);
        assert_eq!(cw_sideband_for_center(3_510_000.0), CwSideband::Upper);
        assert_eq!(cw_sideband_for_center(16_000_000.0), CwSideband::Lower);
    }
}
