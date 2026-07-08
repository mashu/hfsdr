use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn annotate_new_spots(&mut self, center_hz: f64) {
        for spot in &self.skimmer_ui.skimmer_spots {
            let Some(call) = &spot.callsign else { continue };
            let key = format!("{call}@{:.0}", spot.frequency_hz);
            if self.annotated.insert(key) {
                let offset = (spot.frequency_hz - center_hz) as f32;
                let label = match spot.kind {
                    SpotKind::CallingCq => format!("CQ {call}"),
                    SpotKind::Answering => format!("→ {call}"),
                    SpotKind::Heard => call.clone(),
                };
                self.slow.annotate(offset, label, spot.snr_db);
            }
        }
        if self.annotated.len() > 512 {
            self.annotated.clear();
        }
    }

    pub(crate) fn spot_filter_config(&self) -> SpotFilterConfig {
        SpotFilterConfig {
            min_snr_db: self.skimmer_ui.min_spot_snr,
            cq_only: self.skimmer_ui.spot_cq_only,
            max_age_secs: self.skimmer_ui.spot_max_age_secs,
            callsign_prefix: self.skimmer_ui.spot_callsign_filter.clone(),
            continent_filter: self.skimmer_ui.continent_filter,
            show_continents: self.skimmer_ui.show_continents,
            sort: self.skimmer_ui.spot_sort,
        }
    }

    /// Refresh the cached visible-spot list only when something changed:
    /// new spots from the engine, a filter edit, or the periodic tick that
    /// age-based filters and the age column need. Filtering, cloning and
    /// sorting every spot every frame was a measurable per-frame cost.
    pub(crate) fn refresh_visible_spots(&mut self) {
        const REFRESH_EVERY: Duration = Duration::from_millis(250);
        let cfg = self.spot_filter_config();
        let center_hz = self.radio.center_khz * 1000.0;
        let stale = self
            .skimmer_ui
            .last_spot_refresh
            .is_none_or(|t| t.elapsed() >= REFRESH_EVERY);
        let cfg_changed = self.skimmer_ui.last_spot_filter.as_ref() != Some(&cfg);
        let center_changed =
            (center_hz - self.skimmer_ui.last_label_center_hz).abs() > 1e-6;
        if !(self.skimmer_ui.spots_dirty || cfg_changed || stale || center_changed) {
            return;
        }
        self.skimmer_ui.frame_visible_spots =
            filter_spots(&self.skimmer_ui.skimmer_spots, &cfg, &self.resolver);
        self.skimmer_ui.callsign_log_cache = self.callsign_log_entries();
        self.skimmer_ui.spot_label_cache = self.spot_labels(center_hz);
        self.skimmer_ui.last_label_center_hz = center_hz;
        self.skimmer_ui.last_spot_filter = Some(cfg);
        self.skimmer_ui.last_spot_refresh = Some(Instant::now());
        self.skimmer_ui.spots_dirty = false;
    }

    /// Rolling callsign log (last 10 min), newest first — unfiltered by table SNR.
    pub(crate) fn callsign_log_entries(&self) -> Vec<Spot> {
        const MAX_AGE_SECS: f32 = 600.0;
        let mut out: Vec<Spot> = self
            .skimmer_ui
            .skimmer_spots
            .iter()
            .filter(|s| s.callsign.is_some())
            .filter(|s| MAX_AGE_SECS <= 0.0 || s.age().as_secs_f32() <= MAX_AGE_SECS)
            .cloned()
            .collect();
        out.sort_by(|a, b| b.last_heard.cmp(&a.last_heard));
        out
    }

    pub(crate) fn spot_labels(&self, center_hz: f64) -> Vec<SpotLabel> {
        build_spot_labels(
            &self.skimmer_ui.frame_visible_spots,
            center_hz,
            &SpotLabelConfig {
                hide_heard: self.skimmer_ui.spot_hide_heard_labels,
                bucket_hz: self.skimmer_ui.skimmer.bucket_hz,
                label_limit: self.skimmer_ui.spot_label_limit,
            },
        )
    }

    pub(crate) fn clear_spots(&mut self) {
        self.engine.send(EngineCommand::ClearSkimmerSpots);
        self.skimmer_ui.skimmer_spots.clear();
        self.skimmer_ui.frame_visible_spots.clear();
        self.skimmer_ui.skimmer_decode_channels.clear();
        self.annotated.clear();
        log::info("spots cleared");
    }

    pub(crate) fn poll_scp_download(&mut self) {
        let Some(rx) = self.skimmer_ui.scp_download_rx.as_ref() else {
            return;
        };
        let Ok(result) = rx.try_recv() else {
            return;
        };
        self.skimmer_ui.scp_download_rx = None;
        match result {
            Ok(path) => {
                log::info(format!("MASTER.SCP saved to {}", path.display()));
                self.engine.send(EngineCommand::ReloadScpFrom(path.clone()));
                self.skimmer_ui.scp_reload_pending = true;
                self.skimmer_ui.scp_reload_deadline = Some(Instant::now() + Duration::from_secs(8));
                self.skimmer_ui.scp_notice = Some(format!("Downloaded — loading {}", path.display()));
            }
            Err(e) => {
                log::error(format!("MASTER.SCP download failed: {e}"));
                self.skimmer_ui.scp_notice = Some(format!("Download failed: {e}"));
            }
        }
    }

}
