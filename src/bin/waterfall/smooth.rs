//! Display smoothing for spectrum traces (temporal + spatial).

const SPATIAL: [f32; 5] = [0.06, 0.14, 0.6, 0.14, 0.06];

/// Exponential moving average per bin.
pub fn ema_update(smoothed: &mut [f32], new_row: &[f32], alpha: f32) {
    let n = smoothed.len().min(new_row.len());
    for i in 0..n {
        let prev = smoothed[i];
        let fresh = new_row[i];
        smoothed[i] = if prev <= -119.0 {
            fresh
        } else {
            alpha * fresh + (1.0 - alpha) * prev
        };
    }
}

/// 5-point spatial smooth for display (reduces FFT bin noise).
pub fn spatial_smooth(row: &[f32]) -> Vec<f32> {
    let n = row.len();
    if n < 5 {
        return row.to_vec();
    }
    let mut out = row.to_vec();
    for i in 2..n - 2 {
        out[i] = SPATIAL[0] * row[i - 2]
            + SPATIAL[1] * row[i - 1]
            + SPATIAL[2] * row[i]
            + SPATIAL[3] * row[i + 1]
            + SPATIAL[4] * row[i + 2];
    }
    return out;
}
