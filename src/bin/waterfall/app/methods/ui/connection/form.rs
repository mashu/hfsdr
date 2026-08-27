use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn connection_form_section(&mut self, ui: &mut egui::Ui) {
        popup_section(ui, "Connect", None, |ui| {
            // A tab can reach a KiwiSDR (WebSocket) but no local device, and a
            // page served over https cannot open a plain ws:// socket at all.
            // Both are worth saying next to the button rather than leaving the
            // user to guess at a connection that fails for invisible reasons.
            #[cfg(all(target_arch = "wasm32", not(feature = "gui-core")))]
            {
                ui.label(
                    egui::RichText::new(
                        "Browser build: KiwiSDR only — local devices need drivers a tab \
                         cannot load.",
                    )
                    .small()
                    .color(crate::theme::WARN),
                );
                if crate::app::page_requires_tls() {
                    ui.label(
                        egui::RichText::new(
                            "This page is served over https, so it can only reach receivers \
                             that accept wss:// (TLS). A plain http KiwiSDR is blocked by the \
                             browser as mixed content — run this page over http to reach one.",
                        )
                        .small()
                        .color(crate::theme::WARN),
                    );
                }
                ui.add_space(6.0);
            }
            self.connection.form.kind = sanitize_source_kind(self.connection.form.kind);
            let labels = source_kind_labels();
            let selected = source_kind_index(self.connection.form.kind);
            let btn_w = if labels.len() > 4 { 44.0 } else { 64.0 };
            if let Some(i) = segment_choice_sized(ui, "source_kind", selected, &labels, btn_w) {
                #[cfg(feature = "soapy")]
                let prev = self.connection.form.kind;
                self.connection.form.kind = source_kind_from_index(i);
                #[cfg(feature = "soapy")]
                if self.connection.form.kind == SourceKind::Soapy && prev != SourceKind::Soapy {
                    self.refresh_soapy_devices();
                }
            }

            if self.connection.form.kind == SourceKind::Kiwi {
                ui.horizontal(|ui| {
                    ui.label(egui::RichText::new("Host").small().color(MUTED));
                    ui.add(
                        egui::TextEdit::singleline(&mut self.connection.form.host)
                            .hint_text("kiwi.example.com")
                            .desired_width(ui.available_width() - 72.0),
                    );
                    ui.label(egui::RichText::new("Port").small().color(MUTED));
                    ui.add(egui::DragValue::new(&mut self.connection.form.port).range(1..=65535));
                });
            }

            ui.horizontal(|ui| {
                ui.label(
                    egui::RichText::new(format!("RX {:.6} MHz", self.radio.center_khz / 1000.0))
                        .small()
                        .monospace()
                        .color(MUTED),
                );
            });

            let session_active = self.connection_session_live();
            let can_connect = self.can_connect_from_form();
            ui.horizontal(|ui| {
                if primary_button(ui, "Connect", can_connect && !session_active).clicked() {
                    self.connect_now();
                }
                if session_active {
                    let connecting = matches!(
                        self.engine_ui.conn_state,
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

}
