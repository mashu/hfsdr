//! Pure spot filtering / label selection (testable, no egui).

use std::collections::HashMap;

use hfsdr::{Continent, ContinentResolver, Spot, SpotKind, SpotSort};
use hfsdr::multisource::spot_display_snr;

use crate::widgets::SpotLabel;

#[derive(Clone, Debug)]
pub struct SpotFilterConfig {
    pub min_snr_db: f32,
    pub cq_only: bool,
    pub max_age_secs: f32,
    pub callsign_prefix: String,
    pub continent_filter: bool,
    pub show_continents: [bool; 7],
    pub sort: SpotSort,
}

#[derive(Clone, Debug)]
pub struct SpotLabelConfig {
    pub hide_heard: bool,
    pub bucket_hz: f32,
    pub label_limit: usize,
}

pub fn continent_index(c: Continent) -> usize {
    match c {
        Continent::NorthAmerica => 0,
        Continent::SouthAmerica => 1,
        Continent::Europe => 2,
        Continent::Africa => 3,
        Continent::Asia => 4,
        Continent::Oceania => 5,
        Continent::Antarctica => 6,
    }
}

pub fn continent_allowed(
    spot: &Spot,
    filter_on: bool,
    show: &[bool; 7],
    resolver: &ContinentResolver,
) -> bool {
    if !filter_on {
        return true;
    }
    let Some(call) = spot.callsign.as_deref() else {
        return true;
    };
    match resolver.continent_of(call) {
        Some(c) => show[continent_index(c)],
        None => true,
    }
}

pub fn filter_spots(spots: &[Spot], cfg: &SpotFilterConfig, resolver: &ContinentResolver) -> Vec<Spot> {
    let prefix = cfg.callsign_prefix.trim().to_ascii_uppercase();
    let max_age = cfg.max_age_secs;

    let mut out: Vec<Spot> = spots
        .iter()
        .filter(|s| spot_display_snr(s) >= cfg.min_snr_db)
        .filter(|s| continent_allowed(s, cfg.continent_filter, &cfg.show_continents, resolver))
        .filter(|s| !cfg.cq_only || s.kind == SpotKind::CallingCq)
        .filter(|s| max_age <= 0.0 || s.age().as_secs_f32() <= max_age)
        .filter(|s| {
            if prefix.is_empty() {
                return true;
            }
            s.callsign
                .as_ref()
                .is_some_and(|c| c.to_ascii_uppercase().starts_with(&prefix))
        })
        .cloned()
        .collect();

    match cfg.sort {
        SpotSort::SnrDesc => out.sort_by(|a, b| {
            spot_display_snr(b).total_cmp(&spot_display_snr(a))
        }),
        SpotSort::Frequency => out.sort_by(|a, b| a.frequency_hz.total_cmp(&b.frequency_hz)),
        SpotSort::LastHeard => out.sort_by_key(|s| s.last_heard),
        SpotSort::Callsign => out.sort_by(|a, b| a.callsign.cmp(&b.callsign)),
    }
    out
}

pub fn build_spot_labels(
    spots: &[Spot],
    center_hz: f64,
    label_cfg: &SpotLabelConfig,
) -> Vec<SpotLabel> {
    let bucket = label_cfg.bucket_hz as f64;
    let mut best: HashMap<i64, Spot> = HashMap::new();
    for s in spots {
        let Some(call) = s.callsign.clone() else {
            continue;
        };
        if label_cfg.hide_heard && s.kind == SpotKind::Heard {
            continue;
        }
        let key = (s.frequency_hz / bucket).round() as i64;
        match best.get(&key) {
            Some(prev) if spot_display_snr(prev) >= spot_display_snr(s) => {}
            _ => {
                best.insert(
                    key,
                    Spot {
                        callsign: Some(call),
                        ..s.clone()
                    },
                );
            }
        }
    }
    let mut merged: Vec<Spot> = best.into_values().collect();
    merged.sort_by(|a, b| spot_display_snr(b).total_cmp(&spot_display_snr(a)));
    merged.truncate(label_cfg.label_limit);
    merged
        .into_iter()
        .filter_map(|s| {
            let snr = spot_display_snr(&s);
            let text = s.callsign.clone()?;
            Some(SpotLabel {
                offset_hz: (s.frequency_hz - center_hz) as f32,
                text,
                cq: s.kind == SpotKind::CallingCq,
                snr_db: snr,
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    fn spot(call: &str, snr: f32, kind: SpotKind) -> Spot {
        let now = Instant::now();
        Spot {
            frequency_hz: 7_030_000.0,
            callsign: Some(call.into()),
            kind,
            snr_db: snr,
            wpm: 24.0,
            first_heard: now,
            last_heard: now,
            sources: Vec::new(),
            callsign_rank: 0,
        }
    }

    #[test]
    fn prefix_filter_is_prefix_not_substring() {
        let cfg = SpotFilterConfig {
            min_snr_db: 0.0,
            cq_only: false,
            max_age_secs: 0.0,
            callsign_prefix: "G".into(),
            continent_filter: false,
            show_continents: [true; 7],
            sort: SpotSort::SnrDesc,
        };
        let spots = vec![spot("G0ABC", 10.0, SpotKind::Heard), spot("DL1G", 10.0, SpotKind::Heard)];
        let out = filter_spots(&spots, &cfg, &ContinentResolver::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].callsign.as_deref(), Some("G0ABC"));
    }

    #[test]
    fn cq_only_filters() {
        let cfg = SpotFilterConfig {
            cq_only: true,
            min_snr_db: 0.0,
            max_age_secs: 0.0,
            callsign_prefix: String::new(),
            continent_filter: false,
            show_continents: [true; 7],
            sort: SpotSort::SnrDesc,
        };
        let spots = vec![
            spot("G0ABC", 10.0, SpotKind::CallingCq),
            spot("DL1ABC", 12.0, SpotKind::Answering),
        ];
        let out = filter_spots(&spots, &cfg, &ContinentResolver::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].kind, SpotKind::CallingCq);
    }

    #[test]
    fn labels_dedupe_by_bucket() {
        let mut a = spot("G0AAA", 10.0, SpotKind::Heard);
        a.frequency_hz = 7_030_000.0;
        let mut b = spot("G0AAB", 20.0, SpotKind::Heard);
        b.frequency_hz = 7_030_030.0;
        let labels = build_spot_labels(
            &[a, b],
            7_030_000.0,
            &SpotLabelConfig {
                hide_heard: false,
                bucket_hz: 80.0,
                label_limit: 10,
            },
        );
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].text, "G0AAB");
    }

    #[test]
    fn min_snr_filters_weak_spots() {
        let cfg = SpotFilterConfig {
            min_snr_db: 15.0,
            cq_only: false,
            max_age_secs: 0.0,
            callsign_prefix: String::new(),
            continent_filter: false,
            show_continents: [true; 7],
            sort: SpotSort::SnrDesc,
        };
        let spots = vec![spot("G0ABC", 10.0, SpotKind::Heard), spot("G1ABC", 20.0, SpotKind::Heard)];
        let out = filter_spots(&spots, &cfg, &ContinentResolver::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].callsign.as_deref(), Some("G1ABC"));
    }

    #[test]
    fn hide_heard_labels_skips_heard_kind() {
        let labels = build_spot_labels(
            &[spot("G0ABC", 20.0, SpotKind::Heard)],
            7_030_000.0,
            &SpotLabelConfig {
                hide_heard: true,
                bucket_hz: 80.0,
                label_limit: 10,
            },
        );
        assert!(labels.is_empty());
    }

    #[test]
    fn cq_label_flag_set_for_calling_cq() {
        let labels = build_spot_labels(
            &[spot("G0ABC", 20.0, SpotKind::CallingCq)],
            7_030_000.0,
            &SpotLabelConfig {
                hide_heard: false,
                bucket_hz: 80.0,
                label_limit: 10,
            },
        );
        assert_eq!(labels.len(), 1);
        assert!(labels[0].cq);
    }

    #[test]
    fn label_limit_truncates() {
        let spots: Vec<Spot> = (0..5)
            .map(|i| {
                let mut s = spot(&format!("G{i}ABC"), 10.0 + i as f32, SpotKind::Heard);
                s.frequency_hz = 7_030_000.0 + i as f64 * 5_000.0;
                s
            })
            .collect();
        let labels = build_spot_labels(
            &spots,
            7_030_000.0,
            &SpotLabelConfig {
                hide_heard: false,
                bucket_hz: 10.0,
                label_limit: 2,
            },
        );
        assert_eq!(labels.len(), 2);
    }

    #[test]
    fn sort_by_frequency_orders_ascending() {
        let mut a = spot("G0AAA", 10.0, SpotKind::Heard);
        a.frequency_hz = 7_040_000.0;
        let mut b = spot("G0AAB", 10.0, SpotKind::Heard);
        b.frequency_hz = 7_020_000.0;
        let cfg = SpotFilterConfig {
            min_snr_db: 0.0,
            cq_only: false,
            max_age_secs: 0.0,
            callsign_prefix: String::new(),
            continent_filter: false,
            show_continents: [true; 7],
            sort: SpotSort::Frequency,
        };
        let out = filter_spots(&[a, b], &cfg, &ContinentResolver::new());
        assert!(out[0].frequency_hz < out[1].frequency_hz);
    }

    #[test]
    fn continent_filter_respects_show_flags() {
        let cfg = SpotFilterConfig {
            min_snr_db: 0.0,
            cq_only: false,
            max_age_secs: 0.0,
            callsign_prefix: String::new(),
            continent_filter: true,
            show_continents: {
                let mut show = [true; 7];
                show[continent_index(Continent::Europe)] = false;
                show
            },
            sort: SpotSort::SnrDesc,
        };
        let spots = vec![spot("G0ABC", 15.0, SpotKind::Heard)];
        let out = filter_spots(&spots, &cfg, &ContinentResolver::new());
        assert!(out.is_empty());
    }

    #[test]
    fn max_age_filters_stale_spots() {
        let fresh = spot("G0ABC", 15.0, SpotKind::Heard);
        let mut stale = spot("G1ABC", 20.0, SpotKind::Heard);
        stale.last_heard = Instant::now() - std::time::Duration::from_secs(120);
        let cfg = SpotFilterConfig {
            min_snr_db: 0.0,
            cq_only: false,
            max_age_secs: 60.0,
            callsign_prefix: String::new(),
            continent_filter: false,
            show_continents: [true; 7],
            sort: SpotSort::SnrDesc,
        };
        let out = filter_spots(&[fresh, stale], &cfg, &ContinentResolver::new());
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].callsign.as_deref(), Some("G0ABC"));
    }

    #[test]
    fn continent_index_covers_all_variants() {
        assert_eq!(continent_index(Continent::NorthAmerica), 0);
        assert_eq!(continent_index(Continent::Antarctica), 6);
    }

    #[test]
    fn answering_spot_label_uses_callsign_text() {
        let labels = build_spot_labels(
            &[spot("G0ABC", 20.0, SpotKind::Answering)],
            7_030_000.0,
            &SpotLabelConfig {
                hide_heard: false,
                bucket_hz: 80.0,
                label_limit: 10,
            },
        );
        assert_eq!(labels.len(), 1);
        assert_eq!(labels[0].text, "G0ABC");
        assert!(!labels[0].cq);
    }
}
