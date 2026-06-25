use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn connection_recent_section(&mut self, ui: &mut egui::Ui) {
        if self.connection.recent_hosts.is_empty() {
            return;
        }
        popup_section(ui, "Recent", None, |ui| {
            let labels: Vec<String> = self.connection.recent_hosts.iter().map(|r| r.label()).collect();
            let recents = self.connection.recent_hosts.clone();
            if let Some(i) = chip_row(ui, &labels) {
                let req = &recents[i];
                self.apply_connect_form(req);
                self.connect_now();
            }
        });
    }

    pub(crate) fn connection_status_footer(&mut self, ui: &mut egui::Ui, connecting: bool) {
        if let Some(err) = &self.last_error {
            if connecting {
                alert_banner(ui, err, None);
            }
        }

        let mut stats = vec![
            (
                "rate",
                format!("{:.1} kS/s", self.stats.effective_sps / 1000.0),
            ),
            ("drops", self.stats.dropped.to_string()),
        ];
        if let Some(rssi) = self.stats.rssi_dbm {
            stats.push(("S", format!("{rssi:.0} dBm")));
        }
        inline_stats(ui, &stats);
    }

}
