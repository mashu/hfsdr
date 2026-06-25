use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn scp_section(&mut self, ui: &mut egui::Ui) {
        let scp = &self.engine_ui.stats.scp;
        let downloading = self.skimmer_ui.scp_download_rx.is_some();
        collapsible_section(ui, "scp", "MASTER.SCP", None, false, |ui| {
            if scp.loaded {
                let ver = scp.version.as_deref().unwrap_or("unknown version");
                stat_row(ui, "Database", format!("{} calls ({ver})", scp.calls));
                if let Some(path) = &scp.path {
                    stat_row(ui, "Path", path.clone());
                }
            } else {
                ui.colored_label(
                    WARN,
                    "Not loaded — using heuristic callsign check (more false positives)",
                );
                section_hint(ui, "Install N1MM+ MASTER.SCP or click Download below.");
            }
            if let Some(msg) = &self.skimmer_ui.scp_notice {
                ui.colored_label(OK, msg);
            }
            ui.horizontal(|ui| {
                ui.add_enabled_ui(!downloading, |ui| {
                    if ui.button("Download").clicked() {
                        let (tx, rx) = std::sync::mpsc::channel();
                        self.skimmer_ui.scp_download_rx = Some(rx);
                        self.skimmer_ui.scp_notice = Some("Downloading MASTER.SCP…".into());
                        std::thread::spawn(move || {
                            let _ = tx.send(crate::scp_fetch::download_master_scp());
                        });
                    }
                });
                if downloading {
                    ui.spinner();
                }
                if ui.button("Reload").clicked() {
                    self.engine.send(EngineCommand::ReloadScp);
                    self.skimmer_ui.scp_reload_pending = true;
                    self.skimmer_ui.scp_reload_deadline = Some(Instant::now() + Duration::from_secs(8));
                    self.skimmer_ui.scp_notice = Some("Reloading MASTER.SCP…".into());
                    log::info("MASTER.SCP reload requested");
                }
            });
        });
    }

}
