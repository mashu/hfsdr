//! Map full-span FFT rows to the frequency window shown in the panadapter.

/// Maximum bins in a composed panadapter row (wgpu `create_texture` width limit).
pub const MAX_PANADAPTER_BINS: usize = 8192;

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
) -> Vec<f32> {
    const FLOOR: f32 = -120.0;
    let data_span = data_span_hz.min(row_rate_hz.max(1.0));
    let extract_span = data_span.min(view_span_hz);
    let data = extract_view_window(row, row_rate_hz, extract_span, pan_offset_hz);
    if view_span_hz <= data_span + 1.0 {
        return cap_panadapter_bins(data.to_vec());
    }
    let ratio = view_span_hz / data_span;
    let out_len = ((data.len() as f32) * ratio).round() as usize;
    let out_len = out_len.max(data.len());
    let mut out = vec![FLOOR; out_len];
    let start = out_len.saturating_sub(data.len()) / 2;
    let end = start + data.len();
    if end <= out_len {
        out[start..end].copy_from_slice(data);
    }
    cap_panadapter_bins(out)
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
    pub pan_offset_hz: f64,
}

/// Map UI zoom/pan to FFT row coordinates (zoom-decimated rows are already centered on pan).
pub fn spectrum_view_mapping(
    iq_rate: f32,
    spectrum_rate: f32,
    spectrum_zoomed: bool,
    view_span_hz: f32,
    pan_offset_hz: f64,
) -> SpectrumViewMapping {
    if spectrum_zoomed && spectrum_rate > 0.0 {
        SpectrumViewMapping {
            row_rate_hz: spectrum_rate,
            view_span_hz,
            pan_offset_hz: 0.0,
        }
    } else {
        SpectrumViewMapping {
            row_rate_hz: iq_rate,
            view_span_hz,
            pan_offset_hz,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn zoomed_mapping_uses_spectrum_rate() {
        let m = spectrum_view_mapping(768_000.0, 48_000.0, true, 30_000.0, 12_000.0);
        assert_eq!(m.row_rate_hz, 48_000.0);
        assert_eq!(m.pan_offset_hz, 0.0);
    }

    #[test]
    fn full_span_mapping_keeps_pan() {
        let m = spectrum_view_mapping(12_000.0, 12_000.0, false, 12_000.0, 500.0);
        assert_eq!(m.row_rate_hz, 12_000.0);
        assert_eq!(m.pan_offset_hz, 500.0);
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
        let wide = compose_panadapter_row(&row, 12_000.0, 70_000.0, 12_000.0, 0.0);
        assert!(wide.len() > 1024);
        assert!(wide.len() <= MAX_PANADAPTER_BINS);
        assert!(wide[0] <= -119.0);
        let mid = &wide[wide.len() / 4..3 * wide.len() / 4];
        assert!(mid.iter().any(|&v| v > -100.0));
    }

    #[test]
    fn extreme_zoom_out_stays_within_gpu_limit() {
        let row: Vec<f32> = vec![-70.0; 16_384];
        let wide = compose_panadapter_row(&row, 12_000.0, 700_000.0, 12_000.0, 0.0);
        assert_eq!(wide.len(), MAX_PANADAPTER_BINS);
    }

    #[test]
    fn pan_offset_shifts_window() {
        let row: Vec<f32> = (0..1024).map(|i| i as f32).collect();
        let center = extract_view_window(&row, 12_000.0, 1_000.0, 0.0);
        let shifted = extract_view_window(&row, 12_000.0, 1_000.0, 500.0);
        assert_ne!(center[0], shifted[0]);
    }
}
