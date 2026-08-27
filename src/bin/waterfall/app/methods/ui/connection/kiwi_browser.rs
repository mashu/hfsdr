use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn connection_kiwi_browser_section(&mut self, ui: &mut egui::Ui) {
        popup_section(ui, "Public KiwiSDRs", None, |ui| {
            if self.connection.kiwi.fetch_rx.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new("Loading…").small().color(MUTED));
                });
            } else if !self.connection.kiwi.nearby.is_empty() {
                let mut nearby = self.connection.kiwi.nearby.clone();
                // An https page cannot open a plain-ws socket, so receivers
                // without TLS are not merely unlikely to work — the browser
                // refuses before the request leaves the tab. Sort them last and
                // say why rather than offering a click that cannot succeed.
                let page_https = crate::app::page_requires_tls();
                let reachable = |rx: &crate::kiwi_directory::KiwiReceiver| {
                    crate::kiwi_directory::reachable_from_page(page_https, rx.tls)
                };
                nearby.sort_by(|a, b| {
                    let ar = !reachable(a);
                    let br = !reachable(b);
                    let af = a.users >= a.users_max;
                    let bf = b.users >= b.users_max;
                    ar.cmp(&br).then_with(|| {
                        af.cmp(&bf).then_with(|| {
                            a.distance_km
                                .partial_cmp(&b.distance_km)
                                .unwrap_or(std::cmp::Ordering::Equal)
                        })
                    })
                });
                if page_https && !nearby.iter().any(reachable) {
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
                            let full = rx.users >= rx.users_max;
                            let ok = reachable(&rx);
                            let dist = if rx.distance_km > 0.0 {
                                format!("{:.0}km ", rx.distance_km)
                            } else {
                                String::new()
                            };
                            let users = if full {
                                format!("FULL {}/{}", rx.users, rx.users_max)
                            } else {
                                format!("{}/{}", rx.users, rx.users_max)
                            };
                            let line = if ok {
                                format!(
                                    "{}:{} · {}{} · {}",
                                    rx.host, rx.port, dist, users, rx.location
                                )
                            } else {
                                format!(
                                    "{}:{} · no TLS — unreachable from https · {}",
                                    rx.host, rx.port, rx.location
                                )
                            };
                            let resp = list_row(ui, &line, !full && ok);
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
