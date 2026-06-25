use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn spot_display_section(&mut self, ui: &mut egui::Ui) {
        collapsible_section(
            ui,
            "spots",
            "Spots",
            Some(&[
                ("Skimmer", ACCENT),
                ("Decode callsigns across the visible band and list them below.", MUTED),
                ("Table", OK),
                ("Click a row to tune. Sort by column headers.", MUTED),
            ]),
            false,
            |ui| {
                self.spot_display_body(ui);
            },
        );
    }

    pub(crate) fn spot_display_body(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            toggle(ui, &mut self.skimmer_ui.skimmer_enabled, "Skimmer on");
            if ui.button("Clear").on_hover_text("Clear all spots").clicked() {
                self.clear_spots();
            }
        });
        let n = self.skimmer_ui.frame_visible_spots.len();
        stat_row(
            ui,
            "Visible",
            format!("{n} shown · {} decoded", self.skimmer_ui.skimmer_spots.len()),
        );
        if !self.skimmer_ui.skimmer_enabled {
            ui.colored_label(MUTED, "Enable skimmer to decode callsigns on the band.");
        } else if !self.skimmer_spectrum_ok() {
            ui.colored_label(
                WARN,
                "Skimmer needs Process IQ ≤96 kHz on Airspy (Connection → Process IQ), then reconnect.",
            );
        }

        popup_section(ui, "Table filters", Some("Which decoded spots appear in the list"), |ui| {
            scroll_slider_f32(ui, &mut self.skimmer_ui.min_spot_snr, 0.0..=40.0, "Min SNR");
            scroll_slider_f32(
                ui,
                &mut self.skimmer_ui.spot_max_age_secs,
                0.0..=300.0,
                "Max age (s, 0 = all)",
            );
            toggle(ui, &mut self.skimmer_ui.spot_cq_only, "CQ only");
        });

        popup_section(ui, "Plot labels", Some("Callsign tags drawn on the spectrum"), |ui| {
            let mut label_lim = self.skimmer_ui.spot_label_limit as i32;
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Max labels").small().color(MUTED));
                ui.add(egui::DragValue::new(&mut label_lim).range(8..=80).speed(1));
            });
            self.skimmer_ui.spot_label_limit = label_lim as usize;
            toggle(
                ui,
                &mut self.skimmer_ui.spot_hide_heard_labels,
                "Hide unconfirmed on plot",
            );
        });

        popup_section(ui, "Call filter", Some("Narrow by prefix or continent"), |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Prefix").small().color(MUTED));
                ui.add(
                    egui::TextEdit::singleline(&mut self.skimmer_ui.spot_callsign_filter)
                        .desired_width(ui.available_width().min(120.0))
                        .hint_text("e.g. G or DL"),
                );
            });
            toggle(ui, &mut self.skimmer_ui.continent_filter, "Filter by continent");
            if self.skimmer_ui.continent_filter {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(4.0, 4.0);
                    for c in Continent::ALL {
                        let idx = continent_index(c);
                        let on = self.skimmer_ui.show_continents[idx];
                        if ui.selectable_label(on, c.code()).clicked() {
                            self.skimmer_ui.show_continents[idx] = !on;
                        }
                    }
                });
            }
            if self.skimmer_ui.continent_filter && !self.skimmer_ui.show_continents.iter().any(|&on| on) {
                ui.colored_label(WARN, "All continents off — no spots will match");
            }
        });

        ui.add_space(4.0);
        ui.label(egui::RichText::new("Spot table").small().strong().color(ACCENT));
        self.spot_table(ui);
    }

    pub(crate) fn spot_table(&mut self, ui: &mut egui::Ui) {
        let spots = &self.skimmer_ui.frame_visible_spots;
        let sort = &mut self.skimmer_ui.spot_sort;
        let mut tune_to: Option<f64> = None;
        TableBuilder::new(ui)
            .striped(true)
            .sense(egui::Sense::click())
            .max_scroll_height(300.0)
            .column(Column::exact(18.0))
            .column(Column::remainder().at_least(40.0))
            .column(Column::exact(50.0))
            .column(Column::exact(26.0))
            .column(Column::exact(26.0))
            .column(Column::exact(30.0))
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
}
