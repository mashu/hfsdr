//! Multi-source combining (build-order item 6) — honest about what is real.
//!
//! Three regimes, easiest first:
//! - **Selection diversity** (implemented here): per signal, pick the source
//!   with the best instantaneous SNR. No phase coherence needed; kills QSB
//!   nulls and local QRM at one site. Biggest bang-for-buck.
//! - **Non-coherent combining** (scaffolded): time-align envelopes via
//!   cross-correlation, resample to a common rate with drift tracking, then
//!   SNR-weight and sum. Modest gain, strong anti-fade.
//! - **Coherent MRC** (out of scope): needs sample + carrier-phase alignment;
//!   not generally recoverable across independent Kiwis without a shared clock.

/// A source's running SNR for one signal ("what heard it").
#[derive(Clone, Debug)]
pub struct SourceSnr {
    pub label: String,
    pub snr_db: f32,
}

/// Selection diversity: choose the best-SNR source for a signal.
///
/// Returns the index of the winning source, or `None` if the list is empty.
pub fn select_best(sources: &[SourceSnr]) -> Option<usize> {
    sources
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.snr_db.total_cmp(&b.snr_db))
        .map(|(i, _)| i)
}

/// SNR-weighted blend weights (sum to 1) for non-coherent combining.
///
/// Envelope/time alignment and clock-drift resampling must be applied *before*
/// blending; those live in [`align`] (not yet implemented).
pub fn snr_weights(sources: &[SourceSnr]) -> Vec<f32> {
    let lin: Vec<f32> = sources
        .iter()
        .map(|s| 10f32.powf(s.snr_db / 10.0))
        .collect();
    let total: f32 = lin.iter().sum();
    if total <= 0.0 {
        return vec![0.0; sources.len()];
    }
    lin.iter().map(|&w| w / total).collect()
}

pub mod align {
    //! Time/clock alignment primitives for non-coherent combining (scaffold).
    //!
    //! Sample-delay estimation via envelope cross-correlation and per-source
    //! clock-drift tracking. Not yet implemented — combining only applies per
    //! signal that multiple sources actually hear.

    /// Estimate the integer sample delay that best aligns `b` onto `a`.
    ///
    /// TODO: implement envelope cross-correlation with sub-sample interpolation
    /// and continuous drift tracking (independent LOs/clocks drift).
    pub fn estimate_delay(_a: &[f32], _b: &[f32]) -> Option<isize> {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn s(label: &str, snr: f32) -> SourceSnr {
        SourceSnr {
            label: label.into(),
            snr_db: snr,
        }
    }

    #[test]
    fn selection_picks_best() {
        let sources = [s("rx1", 12.0), s("rx2", 20.0), s("rx3", 5.0)];
        assert_eq!(select_best(&sources), Some(1));
    }

    #[test]
    fn weights_favor_strong_source() {
        let sources = [s("rx1", 20.0), s("rx2", 0.0)];
        let w = snr_weights(&sources);
        assert!(w[0] > w[1]);
        assert!((w.iter().sum::<f32>() - 1.0).abs() < 1e-5);
    }
}
