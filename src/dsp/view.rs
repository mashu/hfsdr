//! Map full-span FFT rows to the frequency window shown in the panadapter.

/// Maximum bins in a composed panadapter row (wgpu `create_texture` width limit).
pub const MAX_PANADAPTER_BINS: usize = 8192;
/// Band-overview rows: IQ is a narrow core in a wide padded span — fewer bins are enough.
pub const WIDE_PANADAPTER_BINS: usize = 2048;

/// Target output length for a panadapter row (before padding layout).
pub fn panadapter_output_bins(data_len: usize, view_span_hz: f32, data_span_hz: f32) -> usize {
    if data_len == 0 {
        return 1;
    }
    let data_span = data_span_hz.max(1.0);
    let view_span = view_span_hz.max(1.0);
    if view_span > data_span + 1.0 {
        // Band overview wider than the IQ core (Kiwi padded layout).
        let ratio = view_span / data_span;
        let padded = (data_len as f32 * ratio).round() as usize;
        return padded.max(data_len).min(WIDE_PANADAPTER_BINS);
    }
    if view_span >= data_span - 1.0 {
        // Full native passband visible.
        return data_len.min(MAX_PANADAPTER_BINS);
    }
    // Zoomed in: scale bin count to visible span (never pad a slice to full FFT width).
    let ratio = view_span / data_span;
    let scaled = (data_len as f32 * ratio).round() as usize;
    scaled.clamp(1, data_len).min(MAX_PANADAPTER_BINS)
}

/// Peak-hold downsample for spectrum / waterfall rows.
pub fn downsample_row_peak(src: &[f32], out_len: usize) -> Vec<f32> {
    if out_len == 0 {
        return Vec::new();
    }
    if src.len() <= out_len {
        return src.to_vec();
    }
    let mut out = Vec::with_capacity(out_len);
    for i in 0..out_len {
        let start = i * src.len() / out_len;
        let end = ((i + 1) * src.len() / out_len).max(start + 1).min(src.len());
        let peak = src[start..end]
            .iter()
            .copied()
            .fold(f32::NEG_INFINITY, f32::max);
        out.push(peak);
    }
    out
}

fn cap_panadapter_bins(row: Vec<f32>) -> Vec<f32> {
    if row.len() > MAX_PANADAPTER_BINS {
        downsample_row_peak(&row, MAX_PANADAPTER_BINS)
    } else {
        row
    }
}

/// Pad or peak-downsample a row to an exact bin count (texture rows must match).
pub fn fit_panadapter_row_width(row: Vec<f32>, target: usize) -> Vec<f32> {
    const FLOOR: f32 = -120.0;
    let target = target.max(1);
    if row.len() == target {
        return row;
    }
    if row.len() > target {
        return downsample_row_peak(&row, target);
    }
    let mut out = vec![FLOOR; target];
    let start = (target - row.len()) / 2;
    out[start..start + row.len()].copy_from_slice(&row);
    out
}

/// Extract a frequency slice of an fftshifted dB row for display.
///
/// `span_hz` is the visible width; `center_offset_hz` shifts the window relative to tuned center.
pub fn extract_view_window(
    row: &[f32],
    sample_rate: f32,
    span_hz: f32,
    center_offset_hz: f64,
) -> &[f32] {
    let n = row.len();
    if n < 2 || sample_rate <= 0.0 || span_hz <= 0.0 {
        return row;
    }
    let center = n / 2;
    let half_bins = ((span_hz / 2.0) * n as f32 / sample_rate).round() as usize;
    let half_bins = half_bins.clamp(1, center);
    let offset_bins = (center_offset_hz / sample_rate as f64 * n as f64).round() as i32;

    let mut start = center as i32 - half_bins as i32 + offset_bins;
    let mut end = start + 2 * half_bins as i32;
    if start < 0 {
        start = 0;
        end = (2 * half_bins as i32).min(n as i32);
    }
    if end > n as i32 {
        end = n as i32;
        start = end - 2 * half_bins as i32;
        if start < 0 {
            start = 0;
        }
    }
    let start = start as usize;
    let end = end as usize;
    if end <= start + 1 {
        return &row[center.saturating_sub(1)..center + 1];
    }
    &row[start..end]
}

/// Build a display row for `view_span_hz`, padding with the noise floor when the view is
/// wider than the available IQ (`data_span_hz`). Keeps spectrum/waterfall aligned on Kiwi
/// when zoomed out to the CW band segment.
pub fn compose_panadapter_row(
    row: &[f32],
    row_rate_hz: f32,
    view_span_hz: f32,
    data_span_hz: f32,
    pan_offset_hz: f64,
    allow_band_padding: bool,
) -> Vec<f32> {
    const FLOOR: f32 = -120.0;
    let data_span = data_span_hz.min(row_rate_hz.max(1.0));
    let view_span = if allow_band_padding {
        view_span_hz
    } else {
        view_span_hz.min(data_span)
    };
    let extract_span = data_span.min(view_span);
    let data = extract_view_window(row, row_rate_hz, extract_span, pan_offset_hz);
    let target = if allow_band_padding {
        panadapter_output_bins(row.len(), view_span, data_span_hz)
    } else {
        panadapter_output_bins(row.len(), view_span, data_span)
    };
    let composed = if !allow_band_padding || view_span <= data_span + 1.0 {
        cap_panadapter_bins(data.to_vec())
    } else {
        let ratio = view_span / data_span;
        let out_len = target;
        let data_width = ((out_len as f32 / ratio).round() as usize)
            .clamp(1, data.len())
            .min(out_len);
        let core = if data.len() > data_width {
            downsample_row_peak(data, data_width)
        } else {
            data.to_vec()
        };
        let mut out = vec![FLOOR; out_len];
        let start = out_len.saturating_sub(core.len()) / 2;
        let end = start + core.len();
        if end <= out_len {
            out[start..end].copy_from_slice(&core);
        }
        out
    };
    fit_panadapter_row_width(composed, target)
}

/// Full-span centered view (pan offset zero).
pub fn extract_passband_view(row: &[f32], sample_rate: f32, span_hz: f32) -> &[f32] {
    extract_view_window(row, sample_rate, span_hz, 0.0)
}

/// Row rate / pan used when drawing a spectrum or waterfall row.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpectrumViewMapping {
    pub row_rate_hz: f32,
    pub view_span_hz: f32,
    /// Pan for frequency axis labels and mouse hit-testing (always matches plot state).
    pub pan_offset_hz: f64,
    /// Pan passed to [`compose_panadapter_row`] (zero when FFT rows are mix-down centered).
    pub compose_pan_offset_hz: f64,
    /// Unpadded IQ data width (native passband).
    pub data_span_hz: f32,
    /// When true, views wider than `data_span_hz` pad with noise floor (Kiwi band overview).
    pub allow_band_padding: bool,
}

/// Frequency span stored in the waterfall texture (full IQ or Kiwi band overview).
pub fn waterfall_storage_span_hz(
    data_span_hz: f32,
    allow_band_padding: bool,
    band_overview_span_hz: f32,
) -> f32 {
    if allow_band_padding {
        band_overview_span_hz.max(data_span_hz)
    } else {
        data_span_hz
    }
}

/// View mapping for composing waterfall rows at fixed storage resolution (pan = 0).
pub fn waterfall_storage_mapping(
    iq_passband_hz: f32,
    plot_span_hz: f32,
    spectrum_rate_hz: f32,
    band_overview_span_hz: f32,
    allow_band_padding: bool,
) -> SpectrumViewMapping {
    let row_rate = if spectrum_rate_hz > 0.0 {
        spectrum_rate_hz
    } else {
        plot_span_hz.max(iq_passband_hz)
    };
    let storage_span = if allow_band_padding {
        waterfall_storage_span_hz(iq_passband_hz, true, band_overview_span_hz)
    } else {
        // Wideband: stored rows span the FFT bandwidth, not the wider device IQ rate.
        row_rate
    };
    SpectrumViewMapping {
        row_rate_hz: row_rate,
        view_span_hz: storage_span,
        pan_offset_hz: 0.0,
        compose_pan_offset_hz: 0.0,
        data_span_hz: if allow_band_padding {
            iq_passband_hz
        } else {
            row_rate
        },
        allow_band_padding,
    }
}

/// Linearly resample a composed panadapter row to an exact pixel width (matches trace X mapping).
pub fn stretch_row_to_width(src: &[f32], width: usize) -> Vec<f32> {
    const FLOOR: f32 = -120.0;
    let width = width.max(1);
    if src.is_empty() {
        return vec![FLOOR; width];
    }
    if src.len() == 1 {
        return vec![src[0]; width];
    }
    let denom = width.saturating_sub(1).max(1) as f32;
    let last = src.len() - 1;
    (0..width)
        .map(|x| {
            let t = x as f32 / denom;
            let fidx = t * last as f32;
            let i0 = fidx.floor() as usize;
            let i1 = (i0 + 1).min(last);
            let frac = fidx - i0 as f32;
            src[i0] * (1.0 - frac) + src[i1] * frac
        })
        .collect()
}

/// Map the visible frequency window to texture UV coordinates along the stored row.
///
/// Texture u=0 and u=1 span `storage_span_hz` centered on the tuned carrier.
pub fn waterfall_texture_u_range(
    storage_span_hz: f32,
    view_span_hz: f32,
    pan_offset_hz: f64,
) -> (f32, f32) {
    let storage = storage_span_hz.max(1.0);
    let half = storage as f64 / 2.0;
    let left = pan_offset_hz - view_span_hz as f64 / 2.0;
    let right = pan_offset_hz + view_span_hz as f64 / 2.0;
    let u0 = ((left + half) / storage as f64).clamp(0.0, 1.0) as f32;
    let u1 = ((right + half) / storage as f64).clamp(0.0, 1.0) as f32;
    (u0, u1.max(u0 + 1e-6))
}

/// Offset (Hz relative to tuned center) for a normalized view coordinate t in [0, 1].
pub fn view_t_to_offset_hz(t: f64, view_span_hz: f32, pan_offset_hz: f64) -> f64 {
    pan_offset_hz + (t - 0.5) * view_span_hz as f64
}

/// Normalized view coordinate t in [0, 1] for an offset (Hz relative to tuned center).
pub fn offset_hz_to_view_t(offset_hz: f64, view_span_hz: f32, pan_offset_hz: f64) -> f64 {
    let span = view_span_hz.max(1.0) as f64;
    ((offset_hz - pan_offset_hz) / span + 0.5).clamp(0.0, 1.0)
}

/// Normalized storage coordinate u in [0, 1] for an offset (Hz relative to tuned center).
pub fn offset_hz_to_storage_u(offset_hz: f64, storage_span_hz: f32) -> f64 {
    let storage = storage_span_hz.max(1.0) as f64;
    let half = storage / 2.0;
    ((offset_hz + half) / storage).clamp(0.0, 1.0)
}

/// Map UI zoom/pan to FFT row coordinates.
pub fn spectrum_view_mapping(
    iq_passband_hz: f32,
    spectrum_rate_hz: f32,
    spectrum_zoomed: bool,
    view_span_hz: f32,
    pan_offset_hz: f64,
    allow_band_padding: bool,
) -> SpectrumViewMapping {
    let fft_rate = if spectrum_rate_hz > 0.0 {
        spectrum_rate_hz
    } else {
        iq_passband_hz
    };
    let row_rate_hz = if spectrum_zoomed && spectrum_rate_hz > 0.0 {
        spectrum_rate_hz
    } else {
        fft_rate
    };
    SpectrumViewMapping {
        row_rate_hz,
        view_span_hz,
        pan_offset_hz,
        compose_pan_offset_hz: if spectrum_zoomed { 0.0 } else { pan_offset_hz },
        data_span_hz: if allow_band_padding {
            iq_passband_hz
        } else {
            fft_rate
        },
        allow_band_padding,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoomed_mapping_uses_spectrum_rate() {
        let m = spectrum_view_mapping(768_000.0, 48_000.0, true, 30_000.0, 12_000.0, false);
        assert_eq!(m.row_rate_hz, 48_000.0);
        assert_eq!(m.pan_offset_hz, 12_000.0);
        assert_eq!(m.compose_pan_offset_hz, 0.0);
    }

    #[test]
    fn full_span_mapping_keeps_pan() {
        let m = spectrum_view_mapping(12_000.0, 12_000.0, false, 12_000.0, 500.0, true);
        assert_eq!(m.row_rate_hz, 12_000.0);
        assert_eq!(m.pan_offset_hz, 500.0);
        assert_eq!(m.compose_pan_offset_hz, 500.0);
    }

    #[test]
    fn waterfall_uv_full_span_covers_texture() {
        let (u0, u1) = waterfall_texture_u_range(384_000.0, 384_000.0, 0.0);
        assert!((u0 - 0.0).abs() < 1e-6);
        assert!((u1 - 1.0).abs() < 1e-6);
    }

    #[test]
    fn waterfall_uv_zoom_in_is_center_crop() {
        let (u0, u1) = waterfall_texture_u_range(100_000.0, 10_000.0, 0.0);
        assert!((u0 - 0.45).abs() < 0.01);
        assert!((u1 - 0.55).abs() < 0.01);
    }

    #[test]
    fn view_t_offset_roundtrip() {
        let span = 10_000.0;
        let pan = 500.0;
        for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
            let offset = view_t_to_offset_hz(t, span, pan);
            let back = offset_hz_to_view_t(offset, span, pan);
            assert!((back - t).abs() < 1e-9, "t {t} -> {offset} -> {back}");
        }
    }

    #[test]
    fn zoomed_view_scales_output_bins() {
        let bins = panadapter_output_bins(4096, 38_400.0, 384_000.0);
        assert!(
            (380..=420).contains(&bins),
            "expected ~410 bins for 10% zoom, got {bins}"
        );
        let row: Vec<f32> = (0..4096).map(|i| i as f32).collect();
        let composed = compose_panadapter_row(&row, 384_000.0, 38_400.0, 384_000.0, 0.0, false);
        assert!(
            composed.len() >= 380 && composed.len() <= 420,
            "composed len {}",
            composed.len()
        );
    }

    #[test]
    fn zoomed_compose_aligns_edges_with_view_span() {
        let n = 4096;
        let rate = 384_000.0;
        let view_span = 38_400.0;
        let row: Vec<f32> = (0..n).map(|i| i as f32).collect();
        let composed = compose_panadapter_row(&row, rate, view_span, rate, 0.0, false);
        let m = composed.len();
        assert!(m >= 2);
        // Left edge of composed row ≈ pan - view_span/2; right edge ≈ pan + view_span/2.
        let left = extract_view_window(&row, rate, view_span, 0.0);
        assert_eq!(composed[0], left[0]);
        assert_eq!(composed[m - 1], left[left.len() - 1]);
    }

    #[test]
    fn waterfall_storage_uses_full_passband_on_wideband() {
        let m = waterfall_storage_mapping(384_000.0, 384_000.0, 384_000.0, 70_000.0, false);
        assert_eq!(m.view_span_hz, 384_000.0);
        assert_eq!(m.row_rate_hz, 384_000.0);
        assert_eq!(m.compose_pan_offset_hz, 0.0);
    }

    #[test]
    fn waterfall_storage_matches_decimated_spectrum_rate() {
        let m = waterfall_storage_mapping(384_000.0, 48_000.0, 48_000.0, 70_000.0, false);
        assert_eq!(m.view_span_hz, 48_000.0);
        assert_eq!(m.row_rate_hz, 48_000.0);
        assert_eq!(m.data_span_hz, 48_000.0);
    }

    #[test]
    fn decimated_storage_resample_finds_tone_offset() {
        let rate = 48_000.0;
        let storage_span = rate;
        let view_span = 4_800.0;
        let tone_offset = 500.0f64;
        let n = 4096usize;
        let peak_bin = (tone_offset / rate as f64 * n as f64 + n as f64 / 2.0).round() as usize;
        let mut row = vec![-120.0f32; n];
        row[peak_bin] = -40.0;
        if peak_bin > 0 {
            row[peak_bin - 1] = -55.0;
        }
        if peak_bin + 1 < n {
            row[peak_bin + 1] = -55.0;
        }

        let storage = waterfall_storage_mapping(384_000.0, rate, rate, 70_000.0, false);
        let composed = compose_panadapter_row(
            &row,
            storage.row_rate_hz,
            storage.view_span_hz,
            storage.data_span_hz,
            0.0,
            false,
        );
        let view = compose_panadapter_row(
            &row,
            rate,
            view_span,
            rate,
            0.0,
            false,
        );

        let plot_w = 800usize;
        let t_tone = ((tone_offset / view_span as f64) + 0.5).clamp(0.0, 1.0);
        let click_x = (t_tone * (plot_w - 1) as f64).round() as usize;
        let click_offset = view_t_to_offset_hz(click_x as f64 / (plot_w - 1) as f64, view_span, 0.0);
        let u = offset_hz_to_storage_u(click_offset, storage_span);
        let src_x = (u * (composed.len() - 1) as f64).round() as usize;
        let view_x = (t_tone * (view.len() - 1) as f64).round() as usize;

        let storage_db = composed[src_x.min(composed.len() - 1)];
        let view_db = view[view_x.min(view.len() - 1)];
        assert!(
            storage_db > -80.0 && view_db > -80.0,
            "tone should be visible at storage u={u:.3} and view t={t_tone:.3}, got {storage_db}/{view_db} dB"
        );
        assert!(
            (click_offset - tone_offset).abs() < rate as f64 / composed.len() as f64 * 4.0,
            "click offset {click_offset} should be near tone {tone_offset}"
        );
    }

    #[test]
    fn narrow_span_is_subset_of_full_row() {
        let row: Vec<f32> = (0..1024).map(|i| i as f32).collect();
        let view = extract_passband_view(&row, 12_000.0, 500.0);
        assert!(view.len() < row.len());
        assert!(view.len() > 10);
    }

    #[test]
    fn wide_view_pads_iq_row() {
        let row: Vec<f32> = (0..1024).map(|i| -80.0 + (i as f32) * 0.01).collect();
        let wide = compose_panadapter_row(&row, 12_000.0, 70_000.0, 12_000.0, 0.0, true);
        assert!(wide.len() > 1024);
        assert!(wide.len() <= MAX_PANADAPTER_BINS);
        assert!(wide[0] <= -119.0);
        let mid = &wide[wide.len() / 4..3 * wide.len() / 4];
        assert!(mid.iter().any(|&v| v > -100.0));
    }

    #[test]
    fn no_padding_when_disabled() {
        let row: Vec<f32> = (0..1024).map(|i| -70.0 + (i as f32) * 0.01).collect();
        let wide = compose_panadapter_row(&row, 384_000.0, 384_000.0, 384_000.0, 0.0, false);
        assert!(wide.len() <= 8192);
        assert!(wide.iter().all(|&v| v > -119.0));
    }

    #[test]
    fn extreme_zoom_out_stays_within_gpu_limit() {
        let row: Vec<f32> = vec![-70.0; 16_384];
        let wide = compose_panadapter_row(&row, 12_000.0, 700_000.0, 12_000.0, 0.0, true);
        assert_eq!(wide.len(), WIDE_PANADAPTER_BINS);
    }

    #[test]
    fn wide_overview_avoids_full_pad_allocation() {
        let row: Vec<f32> = vec![-70.0; 2048];
        let wide = compose_panadapter_row(&row, 12_000.0, 70_000.0, 12_000.0, 0.0, true);
        assert_eq!(wide.len(), WIDE_PANADAPTER_BINS);
        let active = wide.iter().filter(|&&v| v > -119.0).count();
        assert!(active < WIDE_PANADAPTER_BINS / 2);
    }

    #[test]
    fn fit_width_normalizes_mismatched_rows() {
        let narrow = vec![-70.0; 2042];
        let wide = vec![-70.0; 2048];
        let a = compose_panadapter_row(&narrow, 12_000.0, 12_000.0, 12_000.0, 0.0, true);
        let b = compose_panadapter_row(&wide, 12_000.0, 12_000.0, 12_000.0, 0.0, true);
        let target = panadapter_output_bins(2048, 12_000.0, 12_000.0);
        assert_eq!(fit_panadapter_row_width(a, target).len(), target);
        assert_eq!(fit_panadapter_row_width(b, target).len(), target);
    }

    #[test]
    fn pan_offset_shifts_window() {
        let row: Vec<f32> = (0..1024).map(|i| i as f32).collect();
        let center = extract_view_window(&row, 12_000.0, 1_000.0, 0.0);
        let shifted = extract_view_window(&row, 12_000.0, 1_000.0, 500.0);
        assert_ne!(center[0], shifted[0]);
    }
}
