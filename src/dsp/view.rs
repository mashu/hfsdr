//! Map full-span FFT rows to the passband shown in the panadapter.

/// Extract the centered frequency slice of an fftshifted dB row for display.
///
/// `span_hz` is the total width shown (e.g. Kiwi passband or full sample rate).
pub fn extract_passband_view(row: &[f32], sample_rate: f32, span_hz: f32) -> &[f32] {
    let n = row.len();
    if n < 2 || sample_rate <= 0.0 || span_hz <= 0.0 {
        return row;
    }
    let center = n / 2;
    let half_bins = ((span_hz / 2.0) * n as f32 / sample_rate).round() as usize;
    let half_bins = half_bins.clamp(1, center);
    let start = center - half_bins;
    let end = center + half_bins;
    &row[start..end]
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
}
