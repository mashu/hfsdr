//! Peak picking and noise-floor estimation over fftshifted spectrum rows.
//!
//! Shared by the in-band skimmer (find every signal) and the zero-beat / pitch
//! lock features (find the one signal near the cursor). Rows are fftshifted dB
//! values: bin `n/2` is DC, bin `i` maps to `(i - n/2) * sample_rate / n`.

/// A detected spectral peak.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Peak {
    /// Frequency offset from tuned center, Hz.
    pub offset_hz: f32,
    /// Peak power above the local noise floor, dB.
    pub snr_db: f32,
    /// FFT bin index.
    pub bin: usize,
}

/// Map an fftshifted bin index to a frequency offset from center.
pub fn bin_to_offset_hz(bin: usize, len: usize, sample_rate: f32) -> f32 {
    (bin as f32 - len as f32 / 2.0) * sample_rate / len as f32
}

/// Map a frequency offset from center to the nearest fftshifted bin index.
pub fn offset_hz_to_bin(offset_hz: f32, len: usize, sample_rate: f32) -> usize {
    let bin = (offset_hz / sample_rate * len as f32 + len as f32 / 2.0).round();
    bin.clamp(0.0, len as f32 - 1.0) as usize
}

/// Robust noise-floor estimate: a low percentile of the row (excludes peaks).
pub fn noise_floor_db(row: &[f32]) -> f32 {
    if row.is_empty() {
        return -120.0;
    }
    let mut scratch: Vec<f32> = row.to_vec();
    scratch.sort_by(f32::total_cmp);
    let idx = (scratch.len() as f32 * 0.25) as usize;
    scratch[idx.min(scratch.len() - 1)]
}

/// Find local maxima at least `min_snr_db` above the noise floor.
///
/// `min_separation_bins` suppresses duplicate detections on a single signal.
pub fn detect_peaks(
    row: &[f32],
    sample_rate: f32,
    min_snr_db: f32,
    min_separation_bins: usize,
) -> Vec<Peak> {
    let n = row.len();
    if n < 3 {
        return Vec::new();
    }
    let floor = noise_floor_db(row);
    let sep = min_separation_bins.max(1);

    let mut peaks: Vec<Peak> = Vec::new();
    for i in 1..n - 1 {
        let v = row[i];
        if v <= row[i - 1] || v < row[i + 1] {
            continue;
        }
        let snr = v - floor;
        if snr < min_snr_db {
            continue;
        }
        if let Some(last) = peaks.last_mut() {
            if i - last.bin < sep {
                if snr > last.snr_db {
                    *last = Peak {
                        offset_hz: bin_to_offset_hz(i, n, sample_rate),
                        snr_db: snr,
                        bin: i,
                    };
                }
                continue;
            }
        }
        peaks.push(Peak {
            offset_hz: bin_to_offset_hz(i, n, sample_rate),
            snr_db: snr,
            bin: i,
        });
    }
    peaks
}

/// Strongest bin offset within `±window_hz` of `around_hz` — used for zero-beat
/// and pitch-lock. Returns `None` if nothing rises above the local floor.
pub fn strongest_offset_hz(
    row: &[f32],
    sample_rate: f32,
    around_hz: f32,
    window_hz: f32,
) -> Option<f32> {
    let n = row.len();
    if n < 3 || sample_rate <= 0.0 {
        return None;
    }
    let center = offset_hz_to_bin(around_hz, n, sample_rate) as i32;
    let half = ((window_hz / sample_rate) * n as f32).round().max(1.0) as i32;
    let lo = (center - half).clamp(0, n as i32 - 1);
    let hi = (center + half).clamp(0, n as i32 - 1);
    if hi <= lo {
        return None;
    }

    let floor = noise_floor_db(row);
    let mut best_bin = lo as usize;
    let mut best_val = f32::NEG_INFINITY;
    for b in lo..=hi {
        let v = row[b as usize];
        if v > best_val {
            best_val = v;
            best_bin = b as usize;
        }
    }
    if best_val - floor < 6.0 {
        return None;
    }
    Some(bin_to_offset_hz(best_bin, n, sample_rate))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bin_offset_roundtrip() {
        let n = 2048;
        let sr = 12_000.0;
        let bin = offset_hz_to_bin(700.0, n, sr);
        let back = bin_to_offset_hz(bin, n, sr);
        assert!((back - 700.0).abs() < sr / n as f32);
    }

    #[test]
    fn finds_strongest_near_target() {
        let n = 2048;
        let sr = 12_000.0;
        let mut row = vec![-100.0f32; n];
        let target_bin = offset_hz_to_bin(300.0, n, sr);
        row[target_bin] = -20.0;
        let off = strongest_offset_hz(&row, sr, 280.0, 200.0).expect("peak found");
        assert!((off - 300.0).abs() < 20.0);
    }

    #[test]
    fn detect_peaks_ignores_floor() {
        let n = 1024;
        let sr = 12_000.0;
        let mut row = vec![-100.0f32; n];
        row[400] = -30.0;
        row[600] = -25.0;
        let peaks = detect_peaks(&row, sr, 20.0, 4);
        assert_eq!(peaks.len(), 2);
    }
}
