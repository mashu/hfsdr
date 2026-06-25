use crate::app::WaterfallApp;
use crate::app::prelude::*;

impl WaterfallApp {

    pub(crate) fn connection_card(&mut self, ui: &mut egui::Ui) {
        let connecting = matches!(
            self.conn_state,
            ConnState::Connecting { .. } | ConnState::Reconnecting { .. }
        );
        if self.connection_unstable() {
            alert_banner(ui, "Link unstable — tuning kept", self.last_error.as_deref());
            if connecting {
                section_hint(ui, "Click Cancel to stop the current attempt and disable auto-reconnect.");
            }
        }

        self.connection_form_section(ui);

        #[cfg(feature = "airspy")]
        if self.connection.form_kind == SourceKind::Airspy {
            self.connection_airspy_section(ui);
        }

        #[cfg(feature = "rtlsdr")]
        if self.connection.form_kind == SourceKind::RtlSdr {
            self.connection_rtlsdr_section(ui);
        }

        #[cfg(feature = "qmx")]
        if self.connection.form_kind == SourceKind::Qmx {
            self.connection_qmx_section(ui);
        }

        if self.connection.form_kind == SourceKind::Kiwi {
            self.connection_kiwi_iq_section(ui);
            self.connection_kiwi_browser_section(ui);
        }

        self.connection_recent_section(ui);
        self.connection_status_footer(ui, connecting);
    }

}
