//! Automatic ref/range dB from live FFT rows.

/// dB above noise floor for the top of the waterfall scale (Ref dB).
pub const REF_ABOVE_NOISE_DB: f32 = 17.0;
/// Default dynamic range: floor ≈ noise (pitch-black band noise).
pub const DEFAULT_RANGE_DB: f32 = 17.0;
/// Signals must exceed the noise estimate by this much before they tint the waterfall.
const FLOOR_ABOVE_NOISE_DB: f32 = 8.0;
/// Headroom above the 99th-percentile bin (ignores single-bin spikes).
const P99_HEADROOM_DB: f32 = 4.0;
const MIN_RANGE_DB: f32 = 12.0;
const MAX_RANGE_DB: f32 = 38.0;
/// Upper edge of the noise blob — above the thermal floor, below weak carriers.
const NOISE_PERCENTILE: f32 = 0.35;
/// Strong-signal reference: high percentile, not the max bin.
const PEAK_PERCENTILE: f32 = 0.99;
/// When averaging rows, bias noise upward so quiet frames do not expose grain.
const ROW_NOISE_AGG_PERCENTILE: f32 = 0.65;

fn percentile(sorted: &[f32], pct: f32) -> f32 {
    if sorted.is_empty() {
        return -120.0;
    }
    let pct = pct.clamp(0.0, 1.0);
    let idx = ((sorted.len() - 1) as f32 * pct).round() as usize;
    sorted[idx.min(sorted.len() - 1)]
}

fn row_percentiles(db_row: &[f32]) -> Option<(f32, f32)> {
    let mut samples: Vec<f32> = db_row.iter().copied().filter(|&d| d > -118.0).collect();
    if samples.len() < 256 {
        return None;
    }
    samples.sort_by(|a, b| a.total_cmp(b));
    let noise = percentile(&samples, NOISE_PERCENTILE);
    let p99 = percentile(&samples, PEAK_PERCENTILE);
    if noise.is_finite() && p99.is_finite() {
        Some((noise, p99))
    } else {
        None
    }
}

fn levels_from_noise_peak(noise: f32, p99: f32) -> (f32, f32) {
    let floor = noise + FLOOR_ABOVE_NOISE_DB;
    let ideal_ref = (p99 + P99_HEADROOM_DB).max(noise + REF_ABOVE_NOISE_DB);
    let range_db = (ideal_ref - floor).clamp(MIN_RANGE_DB, MAX_RANGE_DB);
    let mut ref_db = floor + range_db;
    // Never clip strong signals at the top when the band needs more headroom.
    if ref_db < ideal_ref {
        ref_db = ideal_ref;
    }
    let range_db = ref_db - floor;
    (ref_db, range_db)
}

/// Estimate `(ref_db, range_db)` from one fftshifted spectrum row.
///
/// `ref_db` is the top of the scale; `floor = ref_db - range_db` is the bottom.
/// Uses robust percentiles so one spurious FFT bin cannot saturate the display.
pub fn estimate_levels(db_row: &[f32]) -> Option<(f32, f32)> {
    let (noise, p99) = row_percentiles(db_row)?;
    Some(levels_from_noise_peak(noise, p99))
}

/// Estimate levels from several recent waterfall rows (median noise / peak).
pub fn estimate_levels_from_rows(rows: &[&[f32]]) -> Option<(f32, f32)> {
    let mut noises = Vec::with_capacity(rows.len());
    let mut peaks = Vec::with_capacity(rows.len());
    for row in rows {
        if let Some((noise, p99)) = row_percentiles(row) {
            noises.push(noise);
            peaks.push(p99);
        }
    }
    if noises.is_empty() {
        return None;
    }
    noises.sort_by(|a, b| a.total_cmp(b));
    peaks.sort_by(|a, b| a.total_cmp(b));
    let noise = percentile(&noises, ROW_NOISE_AGG_PERCENTILE);
    let p99 = percentile(&peaks, 0.75);
    Some(levels_from_noise_peak(noise, p99))
}

/// Exponential smooth toward a new estimate (for continuous auto-track).
pub fn smooth_levels(
    current: (f32, f32),
    target: (f32, f32),
    alpha: f32,
) -> (f32, f32) {
    let a = alpha.clamp(0.02, 1.0);
    (
        current.0 * (1.0 - a) + target.0 * a,
        current.1 * (1.0 - a) + target.1 * a,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_band_maps_to_dark_floor() {
        let row = vec![-82.0; 2048];
        let (ref_db, range_db) = estimate_levels(&row).expect("levels");
        let floor = ref_db - range_db;
        assert!((floor - (-82.0 + FLOOR_ABOVE_NOISE_DB)).abs() < 1.0);
        assert!(ref_db >= -82.0 + REF_ABOVE_NOISE_DB - 3.0);
        assert!(range_db >= MIN_RANGE_DB);
    }

    #[test]
    fn single_bin_spike_does_not_saturate() {
        let mut row = vec![-82.0; 2048];
        row[1024] = -10.0;
        let (ref_db, range_db) = estimate_levels(&row).expect("levels");
        let floor = ref_db - range_db;
        assert!(ref_db < -10.0, "ref {ref_db} should ignore lone spike");
        assert!((floor - (-82.0 + FLOOR_ABOVE_NOISE_DB)).abs() < 2.0);
        assert!(range_db <= MAX_RANGE_DB);
    }

    #[test]
    fn peak_gets_headroom_below_clip() {
        let mut row = vec![-78.0; 2048];
        for i in 1010..=1038 {
            row[i] = -42.0;
        }
        let (ref_db, _range_db) = estimate_levels(&row).expect("levels");
        assert!(ref_db >= -38.0);
        assert!(ref_db <= -34.0);
    }

    #[test]
    fn smooth_levels_moves_gradually() {
        let (r, g) = smooth_levels((-65.0, 17.0), (-50.0, 30.0), 0.1);
        assert!(r > -65.0 && r < -50.0);
        assert!(g > 17.0 && g < 30.0);
    }

    #[test]
    fn strong_peak_extends_ref_instead_of_clipping() {
        let mut row = vec![-78.0; 2048];
        for i in 1000..=1050 {
            row[i] = -28.0;
        }
        let (ref_db, range_db) = estimate_levels(&row).expect("levels");
        assert!(ref_db <= -24.0, "ref {ref_db}");
        let floor = ref_db - range_db;
        let t = ((-28.0 - floor) / range_db).clamp(0.0, 1.0);
        assert!(t < 1.0, "strong blob should not clip, t={t}");
    }

    #[test]
    fn multi_row_median_ignores_one_hot_frame() {
        let quiet = vec![-82.0; 2048];
        let mut hot = vec![-82.0; 2048];
        hot[1024] = -5.0;
        let rows = [quiet.as_slice(), quiet.as_slice(), hot.as_slice()];
        let (ref_db, _) = estimate_levels_from_rows(&rows).expect("levels");
        let (single, _) = estimate_levels(&quiet).expect("quiet");
        assert!((ref_db - single).abs() < 3.0, "ref {ref_db} vs quiet {single}");
    }

    #[test]
    fn fft_scalloping_stays_dark() {
        use crate::colormap::db_to_colour;

        let mut row = vec![-82.0; 2048];
        for (i, v) in row.iter_mut().enumerate() {
            // Typical FFT bin-to-bin variation and weak sidelobes.
            *v = -82.0 + ((i * 17 % 11) as f32) * 0.55;
            if i % 97 == 0 {
                *v += 2.5;
            }
        }
        let (ref_db, range_db) = estimate_levels(&row).expect("levels");
        let floor = ref_db - range_db;
        let mut bright = 0usize;
        for &db in &row {
            let t = ((db - floor) / range_db).clamp(0.0, 1.0);
            if t > 0.08 {
                bright += 1;
            }
        }
        assert!(
            bright < row.len() / 20,
            "floor {floor} left {bright}/{} bins visibly above black",
            row.len()
        );
        let mid = db_to_colour(-80.0, ref_db, range_db);
        assert!(mid.r() < 40 && mid.g() < 40, "mid noise should stay dark");
    }
}
