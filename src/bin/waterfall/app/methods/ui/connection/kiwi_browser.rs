use crate::app::WaterfallApp;
use crate::app::prelude::*;
use crate::kiwi_directory::{any_reachable, reachable_from_page, receiver_line, sort_for_display};

impl WaterfallApp {

    pub(crate) fn connection_kiwi_browser_section(&mut self, ui: &mut egui::Ui) {
        popup_section(ui, "Public KiwiSDRs", None, |ui| {
            if self.connection.kiwi.fetch_rx.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new("Loading…").small().color(MUTED));
                });
            } else if !self.connection.kiwi.nearby.is_empty() {
                // An https page cannot open a plain-ws socket, so receivers
                // without TLS are not merely unlikely to work — the browser
                // refuses before the request leaves the tab. Sort them last and
                // say why rather than offering a click that cannot succeed.
                let page_https = crate::app::page_requires_tls();
                let mut nearby = self.connection.kiwi.nearby.clone();
                sort_for_display(&mut nearby, page_https);
                if !any_reachable(&nearby, page_https) {
                    alert_banner(
                        ui,
                        "None of these accept wss:// (TLS), so this page cannot reach any \
                         of them. Run hfsdr locally over http, or use the desktop build, \
                         to use them.",
                        None,
                    );
                }
                egui::ScrollArea::vertical()
                    .max_height(130.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for rx in nearby {
                            // `list_row` senses clicks even when painted
                            // disabled, so the reachability guard has to be
                            // repeated here. Occupancy is left as it was: a
                            // full receiver refuses on its own, an unreachable
                            // one never gets asked.
                            let ok = reachable_from_page(page_https, rx.tls);
                            let enabled = ok && rx.users < rx.users_max;
                            let resp = list_row(ui, &receiver_line(&rx, page_https), enabled);
                            if resp.clicked() && ok {
                                self.connection.form.host = rx.host;
                                self.connection.form.port = rx.port;
                                self.connect_now();
                            }
                        }
                    });
                if ghost_button(ui, "Refresh").clicked() {
                    self.start_kiwi_directory_fetch(true);
                }
            } else if let Some(err) = &self.connection.kiwi.error {
                alert_banner(ui, err, None);
                if ghost_button(ui, "Retry").clicked() {
                    self.connection.kiwi.error = None;
                    self.start_kiwi_directory_fetch(true);
                }
            } else if ghost_button(ui, "Refresh").clicked() {
                self.start_kiwi_directory_fetch(true);
            }
        });
    }

}
