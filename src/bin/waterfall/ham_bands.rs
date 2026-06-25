//! Amateur band allocation limits for RX frequency lock.

struct HamBandSpan {
    min_hz: f64,
    max_hz: f64,
}

/// ITU-style amateur allocations (HF + 6 m). Gaps between bands are out of bounds.
const HAM_BANDS: [HamBandSpan; 11] = [
    HamBandSpan { min_hz: 1_800_000.0, max_hz: 2_000_000.0 },
    HamBandSpan { min_hz: 3_500_000.0, max_hz: 4_000_000.0 },
    HamBandSpan { min_hz: 5_351_500.0, max_hz: 5_366_500.0 },
    HamBandSpan { min_hz: 7_000_000.0, max_hz: 7_200_000.0 },
    HamBandSpan { min_hz: 10_100_000.0, max_hz: 10_150_000.0 },
    HamBandSpan { min_hz: 14_000_000.0, max_hz: 14_350_000.0 },
    HamBandSpan { min_hz: 18_068_000.0, max_hz: 18_168_000.0 },
    HamBandSpan { min_hz: 21_000_000.0, max_hz: 21_450_000.0 },
    HamBandSpan { min_hz: 24_890_000.0, max_hz: 24_990_000.0 },
    HamBandSpan { min_hz: 28_000_000.0, max_hz: 29_700_000.0 },
    HamBandSpan { min_hz: 50_000_000.0, max_hz: 54_000_000.0 },
];

/// Snap `hz` to the nearest in-band frequency (unchanged when already inside a band).
pub fn clamp_hz(hz: f64) -> f64 {
    for band in &HAM_BANDS {
        if hz >= band.min_hz && hz <= band.max_hz {
            return hz;
        }
    }

    let mut best = HAM_BANDS[0].min_hz;
    let mut best_dist = f64::MAX;
    for band in &HAM_BANDS {
        if hz < band.min_hz {
            let dist = band.min_hz - hz;
            if dist < best_dist {
                best_dist = dist;
                best = band.min_hz;
            }
        } else if hz > band.max_hz {
            let dist = hz - band.max_hz;
            if dist < best_dist {
                best_dist = dist;
                best = band.max_hz;
            }
        }
    }
    best
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inside_band_unchanged() {
        assert_eq!(clamp_hz(14_010_000.0), 14_010_000.0);
        assert_eq!(clamp_hz(50_090_000.0), 50_090_000.0);
    }

    #[test]
    fn gap_snaps_to_nearest_edge() {
        assert_eq!(clamp_hz(16_000_000.0), 14_350_000.0);
        assert_eq!(clamp_hz(45_000_000.0), 50_000_000.0);
    }

    #[test]
    fn below_lowest_snaps_to_160m() {
        assert_eq!(clamp_hz(500_000.0), 1_800_000.0);
    }

    #[test]
    fn above_highest_snaps_to_6m_top() {
        assert_eq!(clamp_hz(60_000_000.0), 54_000_000.0);
    }

    #[test]
    fn sixty_meter_band_edges() {
        assert_eq!(clamp_hz(5_354_000.0), 5_354_000.0);
        assert_eq!(clamp_hz(5_400_000.0), 5_366_500.0);
    }
}
