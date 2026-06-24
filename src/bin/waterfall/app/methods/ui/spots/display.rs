// `ui/spots/display` — spot table and filter controls.

    fn spot_display_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(ui, "spots", "Spots", None, false, |ui| {
            self.spot_display_body(ui);
        });
    }


    fn spot_display_body(&mut self, ui: &mut egui::Ui) {
            ui.horizontal(|ui| {
                toggle(ui, &mut self.skimmer_enabled, "Skimmer on");
                if ui.button("Clear").on_hover_text("Clear all spots").clicked() {
                    self.clear_spots();
                }
                let n = self.frame_visible_spots.len();
                ui.label(
                    egui::RichText::new(format!("{n} shown · {} decoded", self.skimmer_spots.len()))
                        .small()
                        .color(MUTED),
                );
            });
            if !self.skimmer_enabled {
                ui.colored_label(MUTED, "Enable skimmer to decode callsigns on the band.");
            } else if !self.skimmer_spectrum_ok() {
                ui.colored_label(
                    WARN,
                    "Skimmer needs Process IQ ≤96 kHz on Airspy (Connection → Process IQ), then reconnect.",
                );
            }
            scroll_slider_f32(ui, &mut self.min_spot_snr, 0.0..=40.0, "Table min SNR");
            scroll_slider_f32(ui, &mut self.spot_max_age_secs, 0.0..=300.0, "Max age (s, 0=all)");
            let mut label_lim = self.spot_label_limit as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Plot labels").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut label_lim).range(8..=80).speed(1));
            });
            self.spot_label_limit = label_lim as usize;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Call filter").small().color(MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut self.spot_callsign_filter)
                        .desired_width(100.0)
                        .hint_text("e.g. G or DL"),
                );
            });
            toggle(ui, &mut self.spot_cq_only, "CQ only");
            toggle(ui, &mut self.spot_hide_heard_labels, "Hide unconfirmed on plot");
            ui.checkbox(&mut self.continent_filter, "Filter by continent");
            if self.continent_filter {
                ui.horizontal_wrapped(|ui| {
                    for c in Continent::ALL {
                        let idx = continent_index(c);
                        let on = self.show_continents[idx];
                        if ui.selectable_label(on, c.code()).clicked() {
                            self.show_continents[idx] = !on;
                        }
                    }
                });
            }
            if self.continent_filter && !self.show_continents.iter().any(|&on| on) {
                ui.colored_label(WARN, "All continents off — no spots will match");
            }
            ui.separator();
            self.spot_table(ui);
    }


    fn spot_table(&mut self, ui: &mut egui::Ui) {
        let spots = &self.frame_visible_spots;
        let sort = &mut self.spot_sort;
        let mut tune_to: Option<f64> = None;
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .max_scroll_height(300.0)
            .column(Column::exact(24.0))
            .column(Column::remainder().at_least(56.0))
            .column(Column::exact(72.0))
            .column(Column::exact(40.0))
            .column(Column::exact(40.0))
            .column(Column::exact(36.0))
            .header(18.0, |mut header| {
                header.col(|_| {});
                header.col(|ui| {
                    if ui.button("Call").clicked() {
                        *sort = SpotSort::Callsign;
                    }
                });
                header.col(|ui| {
                    if ui.button("kHz").clicked() {
                        *sort = SpotSort::Frequency;
                    }
                });
                header.col(|ui| {
                    if ui.button("dB").clicked() {
                        *sort = SpotSort::SnrDesc;
                    }
                });
                header.col(|ui| {
                    ui.label(egui::RichText::new("wpm").small().color(MUTED));
                });
                header.col(|ui| {
                    if ui.button("Age").clicked() {
                        *sort = SpotSort::LastHeard;
                    }
                });
            })
            .body(|mut body| {
                for spot in spots {
                    body.row(18.0, |mut row| {
                        let (glyph, color) = match spot.kind {
                            SpotKind::CallingCq => ("CQ", WARN),
                            SpotKind::Answering => ("→", OK),
                            SpotKind::Heard => ("·", MUTED),
                        };
                        row.col(|ui| {
                            ui.label(egui::RichText::new(glyph).monospace().color(color));
                        });
                        row.col(|ui| {
                            let call = match (spot.callsign.as_deref(), spot.kind) {
                                (Some(c), _) => c,
                                (None, SpotKind::CallingCq) => "CQ",
                                (None, _) => "…",
                            };
                            ui.label(egui::RichText::new(call).monospace().color(color));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.1}", spot.frequency_hz / 1000.0));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.0}", spot.snr_db));
                        });
                        row.col(|ui| {
                            ui.label(format!("{:.0}", spot.wpm));
                        });
                        row.col(|ui| {
                            let secs = spot.age().as_secs();
                            ui.label(
                                egui::RichText::new(if secs < 60 {
                                    format!("{secs}s")
                                } else {
                                    format!("{}m", secs / 60)
                                })
                                .small()
                                .color(MUTED),
                            );
                        });
                        if row.response().clicked() {
                            tune_to = Some(spot.frequency_hz);
                        }
                    });
                }
            });
        if let Some(hz) = tune_to {
            self.tune_to_hz(hz);
        }
    }


