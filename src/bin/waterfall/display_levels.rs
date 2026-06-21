//! Automatic ref/range dB from live FFT rows.

const MIN_RANGE_DB: f32 = 35.0;
const MAX_RANGE_DB: f32 = 85.0;
const PEAK_HEADROOM_DB: f32 = 8.0;
const NOISE_MARGIN_DB: f32 = 10.0;

/// Estimate `(ref_db, range_db)` from one fftshifted spectrum row.
///
/// `ref_db` is the top of the scale; `floor = ref_db - range_db` is the bottom.
pub fn estimate_levels(db_row: &[f32]) -> Option<(f32, f32)> {
    let mut samples: Vec<f32> = db_row.iter().copied().filter(|&d| d > -118.0).collect();
    if samples.len() < 64 {
        return None;
    }
    samples.sort_by(|a, b| a.total_cmp(b));

    let n = samples.len();
    let noise = samples[n * 10 / 100];
    let peak = samples[n - 1];
    if !noise.is_finite() || !peak.is_finite() {
        return None;
    }

    if peak - noise < 5.0 {
        let ref_db = noise + 18.0;
        let range_db = 50.0;
        return Some((ref_db, range_db));
    }

    let ref_db = peak + PEAK_HEADROOM_DB;
    let range_db = (ref_db - noise + NOISE_MARGIN_DB).clamp(MIN_RANGE_DB, MAX_RANGE_DB);
    Some((ref_db, range_db))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quiet_band_maps_to_dark_floor() {
        let row = vec![-82.0; 2048];
        let (ref_db, range_db) = estimate_levels(&row).expect("levels");
        let floor = ref_db - range_db;
        assert!(floor <= -82.0);
    }

    #[test]
    fn peak_gets_headroom_below_clip() {
        let mut row = vec![-78.0; 2048];
        row[1024] = -42.0;
        let (ref_db, _range_db) = estimate_levels(&row).expect("levels");
        assert!(ref_db >= -34.0);
        assert!(ref_db <= -30.0);
    }
}
