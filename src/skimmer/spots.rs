//! Spot store — the contest dashboard's backing model.
//!
//! One [`Spot`] per decoded station, keyed by frequency. Designed to later carry
//! per-receiver SNR ("what heard it") and DXCC/continent tags once multi-source
//! aggregation and `cty.dat` resolution land.

use std::collections::HashMap;
use std::time::{Duration, Instant};

/// CQ / run-frequency classification for a decoded station.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotKind {
    /// A plain decode with no recognised pattern yet.
    Heard,
    /// Station calling CQ (run frequency candidate).
    CallingCq,
    /// Station answering someone.
    Answering,
}

/// A decoded station on the band.
#[derive(Clone, Debug)]
pub struct Spot {
    pub frequency_hz: f64,
    pub callsign: Option<String>,
    pub kind: SpotKind,
    pub snr_db: f32,
    pub wpm: f32,
    pub first_heard: Instant,
    pub last_heard: Instant,
    /// Per-source SNR ("what heard it"): (source label, SNR dB).
    pub sources: Vec<(String, f32)>,
    /// SCP/heuristic quality — higher wins when multiple decodes collide.
    pub callsign_rank: u32,
}

impl Spot {
    pub fn age(&self) -> Duration {
        self.last_heard.elapsed()
    }
}

/// How spots are sorted in the table.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SpotSort {
    SnrDesc,
    Frequency,
    LastHeard,
    Callsign,
}

/// Collection of active spots, merged by frequency proximity.
#[derive(Debug, Default)]
pub struct SpotStore {
    spots: HashMap<i64, Spot>,
    bucket_hz: f64,
}

impl SpotStore {
    pub fn new() -> Self {
        Self {
            spots: HashMap::new(),
            bucket_hz: 50.0,
        }
    }

    fn key(&self, frequency_hz: f64) -> i64 {
        (frequency_hz / self.bucket_hz).round() as i64
    }

    /// Insert or update a spot at `frequency_hz`.
    pub fn observe(
        &mut self,
        frequency_hz: f64,
        callsign: Option<String>,
        callsign_rank: u32,
        kind: SpotKind,
        snr_db: f32,
        wpm: f32,
        source: &str,
    ) {
        let key = self.key(frequency_hz);
        let now = Instant::now();
        let entry = self.spots.entry(key).or_insert_with(|| Spot {
            frequency_hz,
            callsign: callsign.clone(),
            kind,
            snr_db,
            wpm,
            first_heard: now,
            last_heard: now,
            sources: Vec::new(),
            callsign_rank: 0,
        });
        entry.frequency_hz = frequency_hz;
        entry.kind = kind;
        entry.snr_db = snr_db;
        entry.wpm = wpm;
        entry.last_heard = now;
        if let Some(ref new_call) = callsign {
            if callsign_rank >= entry.callsign_rank {
                entry.callsign = Some(new_call.clone());
                entry.callsign_rank = callsign_rank;
            }
        }
        match entry.sources.iter_mut().find(|(s, _)| s == source) {
            Some((_, s)) => *s = snr_db,
            none => {
                let _ = none;
                entry.sources.push((source.to_string(), snr_db));
            }
        }
    }

    /// Drop spots not heard within `max_age`.
    pub fn prune(&mut self, max_age: Duration) {
        self.spots.retain(|_, s| s.age() <= max_age);
    }

    pub fn clear(&mut self) {
        self.spots.clear();
    }

    pub fn len(&self) -> usize {
        self.spots.len()
    }

    pub fn is_empty(&self) -> bool {
        self.spots.is_empty()
    }

    /// Spots sorted for display.
    pub fn sorted(&self, sort: SpotSort) -> Vec<Spot> {
        let mut out: Vec<Spot> = self.spots.values().cloned().collect();
        match sort {
            SpotSort::SnrDesc => out.sort_by(|a, b| b.snr_db.total_cmp(&a.snr_db)),
            SpotSort::Frequency => out.sort_by(|a, b| a.frequency_hz.total_cmp(&b.frequency_hz)),
            SpotSort::LastHeard => out.sort_by_key(|s| s.last_heard),
            SpotSort::Callsign => out.sort_by(|a, b| a.callsign.cmp(&b.callsign)),
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn merges_nearby_frequencies() {
        let mut store = SpotStore::new();
        store.observe(7_030_000.0, Some("AA1A".into()), 100, SpotKind::CallingCq, 20.0, 28.0, "rx1");
        store.observe(7_030_010.0, Some("AA1A".into()), 100, SpotKind::CallingCq, 22.0, 28.0, "rx1");
        assert_eq!(store.len(), 1);
    }

    #[test]
    fn tracks_multiple_sources() {
        let mut store = SpotStore::new();
        store.observe(7_030_000.0, Some("AA1A".into()), 100, SpotKind::Heard, 20.0, 28.0, "rx1");
        store.observe(7_030_000.0, Some("AA1A".into()), 50, SpotKind::Heard, 15.0, 28.0, "rx2");
        let spot = &store.sorted(SpotSort::SnrDesc)[0];
        assert_eq!(spot.sources.len(), 2);
    }

    #[test]
    fn higher_rank_call_wins() {
        let mut store = SpotStore::new();
        store.observe(7_030_000.0, Some("AA1A".into()), 10, SpotKind::Heard, 20.0, 28.0, "rx1");
        store.observe(7_030_000.0, Some("BB2B".into()), 50, SpotKind::Heard, 20.0, 28.0, "rx1");
        let spot = &store.sorted(SpotSort::SnrDesc)[0];
        assert_eq!(spot.callsign.as_deref(), Some("BB2B"));
    }

    #[test]
    fn prune_drops_stale_spots() {
        let mut store = SpotStore::new();
        store.observe(7_030_000.0, None, 0, SpotKind::Heard, 10.0, 25.0, "rx1");
        store.prune(Duration::from_secs(0));
        assert!(store.is_empty());
    }

    #[test]
    fn clear_and_sort_orders() {
        let mut store = SpotStore::new();
        store.observe(7_040_000.0, Some("ZZ9Z".into()), 10, SpotKind::CallingCq, 10.0, 30.0, "rx1");
        store.observe(7_030_000.0, Some("AA1A".into()), 10, SpotKind::Heard, 30.0, 28.0, "rx1");
        let by_freq = store.sorted(SpotSort::Frequency);
        assert!(by_freq[0].frequency_hz < by_freq[1].frequency_hz);
        let by_call = store.sorted(SpotSort::Callsign);
        assert_eq!(by_call[0].callsign.as_deref(), Some("AA1A"));
        store.clear();
        assert_eq!(store.len(), 0);
    }
}
