// `spots/logic` — spot filtering, labels, and SCP download polling.

    fn annotate_new_spots(&mut self, center_hz: f64) {
        for spot in &self.skimmer_spots {
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

    fn spot_filter_config(&self) -> SpotFilterConfig {
        SpotFilterConfig {
            min_snr_db: self.min_spot_snr,
            cq_only: self.spot_cq_only,
            max_age_secs: self.spot_max_age_secs,
            callsign_prefix: self.spot_callsign_filter.clone(),
            continent_filter: self.continent_filter,
            show_continents: self.show_continents,
            sort: self.spot_sort,
        }
    }

    fn visible_spots(&self) -> Vec<Spot> {
        filter_spots(
            &self.skimmer_spots,
            &self.spot_filter_config(),
            &self.resolver,
        )
    }

    fn spot_labels(&self, center_hz: f64) -> Vec<SpotLabel> {
        build_spot_labels(
            &self.frame_visible_spots,
            center_hz,
            &SpotLabelConfig {
                hide_heard: self.spot_hide_heard_labels,
                bucket_hz: self.skimmer.bucket_hz,
                label_limit: self.spot_label_limit,
            },
        )
    }

    fn clear_spots(&mut self) {
        self.engine.send(EngineCommand::ClearSkimmerSpots);
        self.skimmer_spots.clear();
        self.frame_visible_spots.clear();
        self.annotated.clear();
        log::info("spots cleared");
    }

    fn poll_scp_download(&mut self) {
        let Some(rx) = self.scp_download_rx.as_ref() else {
            return;
        };
        let Ok(result) = rx.try_recv() else {
            return;
        };
        self.scp_download_rx = None;
        match result {
            Ok(path) => {
                log::info(format!("MASTER.SCP saved to {}", path.display()));
                self.engine.send(EngineCommand::ReloadScpFrom(path.clone()));
                self.scp_reload_pending = true;
                self.scp_reload_deadline = Some(Instant::now() + Duration::from_secs(8));
                self.scp_notice = Some(format!("Downloaded — loading {}", path.display()));
            }
            Err(e) => {
                log::error(format!("MASTER.SCP download failed: {e}"));
                self.scp_notice = Some(format!("Download failed: {e}"));
            }
        }
    }
