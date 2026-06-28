//! Connection UI state: form fields vs async Kiwi directory lookup.

use std::sync::mpsc::Receiver;

use crate::kiwi_directory::{GeoLocation, KiwiReceiver};
use crate::source::{AirspySettings, ConnectRequest, KiwiSettings, QmxSettings, RtlSdrSettings, SourceKind};
#[cfg(feature = "soapy")]
use crate::source::SoapySettings;

#[derive(Debug)]
pub struct ConnectionFormState {
    pub pending_connect: Option<ConnectRequest>,
    pub kind: SourceKind,
    pub host: String,
    pub port: u16,
    pub kiwi: KiwiSettings,
    pub sample_rate: u32,
    pub airspy: AirspySettings,
    pub last_airspy_rf: AirspySettings,
    pub rtlsdr: RtlSdrSettings,
    pub last_rtlsdr_rf: RtlSdrSettings,
    pub qmx: QmxSettings,
    pub last_qmx_rf: QmxSettings,
    #[cfg(feature = "soapy")]
    pub soapy: SoapySettings,
    #[cfg(feature = "soapy")]
    pub last_soapy_rf: SoapySettings,
    #[cfg(feature = "soapy")]
    pub soapy_device_labels: Vec<String>,
    #[cfg(feature = "soapy")]
    pub soapy_device_args_list: Vec<String>,
    #[cfg(feature = "soapy")]
    pub soapy_device_index: usize,
    #[cfg(feature = "soapy")]
    pub soapy_enumerate_error: Option<String>,
    pub recent_hosts: Vec<ConnectRequest>,
    pub show_connection_drawer: bool,
}

#[derive(Debug)]
pub struct KiwiDirectoryState {
    pub geo: Option<GeoLocation>,
    pub nearby: Vec<KiwiReceiver>,
    pub fetch_rx: Option<Receiver<Result<(Option<GeoLocation>, Vec<KiwiReceiver>), String>>>,
    pub error: Option<String>,
}

#[derive(Debug)]
pub struct ConnectionState {
    pub form: ConnectionFormState,
    pub kiwi: KiwiDirectoryState,
}
