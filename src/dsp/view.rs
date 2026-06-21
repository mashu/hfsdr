//! Map full-span FFT rows to the frequency window shown in the panadapter.

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

/// Full-span centered view (pan offset zero).
pub fn extract_passband_view(row: &[f32], sample_rate: f32, span_hz: f32) -> &[f32] {
    extract_view_window(row, sample_rate, span_hz, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn narrow_span_is_subset_of_full_row() {
        let row: Vec<f32> = (0..1024).map(|i| i as f32).collect();
        let view = extract_passband_view(&row, 12_000.0, 500.0);
        assert!(view.len() < row.len());
        assert!(view.len() > 10);
    }

    #[test]
    fn pan_offset_shifts_window() {
        let row: Vec<f32> = (0..1024).map(|i| i as f32).collect();
        let center = extract_view_window(&row, 12_000.0, 1_000.0, 0.0);
        let shifted = extract_view_window(&row, 12_000.0, 1_000.0, 500.0);
        assert_ne!(center[0], shifted[0]);
    }
}
