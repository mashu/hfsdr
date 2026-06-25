use std::sync::mpsc::Receiver;

use crate::kiwi_directory::{GeoLocation, KiwiReceiver};
use crate::source::{AirspySettings, ConnectRequest, KiwiSettings, QmxSettings, RtlSdrSettings, SourceKind};

#[derive(Debug)]
pub struct ConnectionState {
    pub pending_connect: Option<ConnectRequest>,
    pub form_kind: SourceKind,
    pub form_host: String,
    pub form_port: u16,
    pub form_kiwi: KiwiSettings,
    pub form_sample_rate: u32,
    pub form_airspy: AirspySettings,
    pub last_airspy_rf: AirspySettings,
    pub form_rtlsdr: RtlSdrSettings,
    pub last_rtlsdr_rf: RtlSdrSettings,
    pub form_qmx: QmxSettings,
    pub last_qmx_rf: QmxSettings,
    pub recent_hosts: Vec<ConnectRequest>,
    pub kiwi_geo: Option<GeoLocation>,
    pub kiwi_nearby: Vec<KiwiReceiver>,
    pub kiwi_directory_rx: Option<Receiver<Result<(Option<GeoLocation>, Vec<KiwiReceiver>), String>>>,
    pub kiwi_directory_error: Option<String>,
    pub show_connection_drawer: bool,
}
