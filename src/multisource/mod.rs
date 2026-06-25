//! Selection diversity and per-source SNR helpers for multi-RX spots.

use crate::skimmer::Spot;

/// A source's running SNR for one signal ("what heard it").
#[derive(Clone, Debug)]
pub struct SourceSnr {
    pub label: String,
    pub snr_db: f32,
}

/// Selection diversity: choose the best-SNR source for a signal.
pub fn select_best(sources: &[SourceSnr]) -> Option<usize> {
    sources
        .iter()
        .enumerate()
        .max_by(|(_, a), (_, b)| a.snr_db.total_cmp(&b.snr_db))
        .map(|(i, _)| i)
}

/// SNR-weighted blend weights (sum to 1) for non-coherent combining.
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

/// Display SNR for a spot: best instantaneous source when tracked, else aggregate.
pub fn spot_display_snr(spot: &Spot) -> f32 {
    if spot.sources.is_empty() {
        return spot.snr_db;
    }
    let ranked: Vec<SourceSnr> = spot
        .sources
        .iter()
        .map(|(label, snr_db)| SourceSnr {
            label: label.clone(),
            snr_db: *snr_db,
        })
        .collect();
    select_best(&ranked)
        .map(|i| ranked[i].snr_db)
        .unwrap_or(spot.snr_db)
}

/// Primary source label for a spot (best SNR), if known.
pub fn spot_primary_source(spot: &Spot) -> Option<String> {
    spot.sources
        .iter()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .map(|(label, _)| label.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skimmer::{Spot, SpotKind};
    use std::time::Instant;

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

    #[test]
    fn spot_display_snr_uses_best_source() {
        let now = Instant::now();
        let spot = Spot {
            frequency_hz: 7_030_000.0,
            callsign: Some("AA1A".into()),
            kind: SpotKind::Heard,
            snr_db: 10.0,
            wpm: 24.0,
            first_heard: now,
            last_heard: now,
            sources: vec![("rx1".into(), 12.0), ("rx2".into(), 20.0)],
            callsign_rank: 0,
        };
        assert_eq!(spot_display_snr(&spot), 20.0);
        assert_eq!(spot_primary_source(&spot), Some("rx2".into()));
    }

    #[test]
    fn empty_sources_use_aggregate_snr() {
        let now = Instant::now();
        let spot = Spot {
            frequency_hz: 7_030_000.0,
            callsign: None,
            kind: SpotKind::Heard,
            snr_db: 14.5,
            wpm: 22.0,
            first_heard: now,
            last_heard: now,
            sources: Vec::new(),
            callsign_rank: 0,
        };
        assert_eq!(spot_display_snr(&spot), 14.5);
        assert_eq!(spot_primary_source(&spot), None);
    }

    #[test]
    fn snr_weights_empty_returns_empty() {
        assert!(snr_weights(&[]).is_empty());
    }

    #[test]
    fn select_best_empty_is_none() {
        assert_eq!(select_best(&[]), None);
    }
}
