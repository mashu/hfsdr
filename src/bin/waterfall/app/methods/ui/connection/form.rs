// `ui/connection/form` — source picker and connect / disconnect controls.

    fn connection_form_section(&mut self, ui: &mut egui::Ui) {
        popup_section(ui, "Connect", None, |ui| {
            let labels = source_kind_labels();
            let selected = source_kind_index(self.form_kind);
            if let Some(i) = segment_choice(ui, "source_kind", selected, &labels) {
                self.form_kind = source_kind_from_index(i);
            }

            if self.form_kind == SourceKind::Kiwi {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Host").small().color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.form_host)
                            .hint_text("kiwi.example.com")
                            .desired_width(ui.available_width() - 72.0),
                    );
                    ui.label(egui::RichText::new("Port").small().color(MUTED));
                    ui.add(egui::DragValue::new(&mut self.form_port).range(1..=65535));
                });
            }

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("RX {:.6} MHz", self.center_khz / 1000.0))
                        .small()
                        .monospace()
                        .color(MUTED),
                );
            });

            let session_active = self.connection_session_live();
            let can_connect = is_local_source(self.form_kind) || !self.form_host.trim().is_empty();
            ui.horizontal(|ui| {
                if primary_button(ui, "Connect", can_connect && !session_active).clicked() {
                    self.connect_now();
                }
                if session_active {
                    let connecting = matches!(
                        self.conn_state,
                        ConnState::Connecting { .. } | ConnState::Reconnecting { .. }
                    );
                    let label = if connecting { "Cancel" } else { "Disconnect" };
                    if secondary_button(ui, label)
                        .on_hover_text(if connecting {
                            "Stop connecting and cancel auto-retry"
                        } else {
                            "Disconnect from the receiver"
                        })
                        .clicked()
                    {
                        self.cancel_connection();
                    }
                }
            });
        });
    }
