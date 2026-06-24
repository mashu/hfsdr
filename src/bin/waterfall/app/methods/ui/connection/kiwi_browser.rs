// `ui/connection/kiwi_browser` — public KiwiSDR directory browser.

    fn connection_kiwi_browser_section(&mut self, ui: &mut egui::Ui) {
        popup_section(ui, "Public KiwiSDRs", None, |ui| {
            if self.kiwi_directory_rx.is_some() {
                ui.horizontal(|ui| {
                    ui.spinner();
                    ui.label(egui::RichText::new("Loading…").small().color(MUTED));
                });
            } else if !self.kiwi_nearby.is_empty() {
                let mut nearby = self.kiwi_nearby.clone();
                nearby.sort_by(|a, b| {
                    let af = a.users >= a.users_max;
                    let bf = b.users >= b.users_max;
                    af.cmp(&bf).then_with(|| {
                        a.distance_km
                            .partial_cmp(&b.distance_km)
                            .unwrap_or(std::cmp::Ordering::Equal)
                    })
                });
                egui::ScrollArea::vertical()
                    .max_height(130.0)
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        for rx in nearby {
                            let full = rx.users >= rx.users_max;
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
                            let line = format!(
                                "{}:{} · {}{} · {}",
                                rx.host, rx.port, dist, users, rx.location
                            );
                            let resp = list_row(ui, &line, !full);
                            if resp.clicked() {
                                self.form_host = rx.host;
                                self.form_port = rx.port;
                                self.connect_now();
                            }
                        }
                    });
                if ghost_button(ui, "Refresh").clicked() {
                    self.start_kiwi_directory_fetch(true);
                }
            } else if let Some(err) = &self.kiwi_directory_error {
                alert_banner(ui, err, None);
                if ghost_button(ui, "Retry").clicked() {
                    self.kiwi_directory_error = None;
                    self.start_kiwi_directory_fetch(true);
                }
            } else if ghost_button(ui, "Refresh").clicked() {
                self.start_kiwi_directory_fetch(true);
            }
        });
    }
