//! Connected front end as a typed enum (IQ streaming + device-specific controls).

use hfsdr::{KiwiControls, QmxControls, Complex32, Consumer, IqSource, KiwiSource, Result};
#[cfg(feature = "airspy")]
use hfsdr::AirspyHf;
#[cfg(feature = "qmx")]
use hfsdr::QmxSource;
#[cfg(feature = "rtlsdr")]
use hfsdr::RtlSdr;

/// A live device handle owned by the engine thread.
pub enum DeviceSource {
    Kiwi(KiwiSource),
    #[cfg(feature = "airspy")]
    Airspy(AirspyHf),
    #[cfg(feature = "rtlsdr")]
    RtlSdr(RtlSdr),
    #[cfg(feature = "qmx")]
    Qmx(QmxSource),
}

impl DeviceSource {
    pub fn as_iq(&mut self) -> &mut dyn IqSource {
        match self {
            Self::Kiwi(s) => s,
            #[cfg(feature = "airspy")]
            Self::Airspy(s) => s,
            #[cfg(feature = "rtlsdr")]
            Self::RtlSdr(s) => s,
            #[cfg(feature = "qmx")]
            Self::Qmx(s) => s,
        }
    }

    fn as_iq_ref(&self) -> &dyn IqSource {
        match self {
            Self::Kiwi(s) => s,
            #[cfg(feature = "airspy")]
            Self::Airspy(s) => s,
            #[cfg(feature = "rtlsdr")]
            Self::RtlSdr(s) => s,
            #[cfg(feature = "qmx")]
            Self::Qmx(s) => s,
        }
    }

    pub fn tune(&mut self, hz: f64) -> Result<()> {
        self.as_iq().tune(hz)
    }

    pub fn stop(&mut self) -> Result<()> {
        self.as_iq().stop()
    }

    pub fn dropped_samples(&self) -> u64 {
        self.as_iq_ref().dropped_samples()
    }

    pub fn is_streaming(&self) -> bool {
        self.as_iq_ref().is_streaming()
    }

    pub fn rssi_dbm(&self) -> Option<f32> {
        match self {
            Self::Kiwi(s) => KiwiControls::rssi_dbm(s),
            #[cfg(feature = "qmx")]
            Self::Qmx(s) => QmxControls::rssi_dbm(s),
            #[cfg(feature = "airspy")]
            Self::Airspy(_) => None,
            #[cfg(feature = "rtlsdr")]
            Self::RtlSdr(_) => None,
        }
    }

    pub fn hw_rf_gain(&self) -> Option<u8> {
        match self {
            Self::Kiwi(s) => KiwiControls::hw_rf_gain(s),
            _ => None,
        }
    }

    pub fn kiwi_rf_stats(&self) -> (bool, f32) {
        match self {
            Self::Kiwi(s) => (
                KiwiControls::has_rf_attn(s),
                KiwiControls::rf_attn_db(s).unwrap_or(0.0),
            ),
            _ => (false, 0.0),
        }
    }

    pub fn link_error(&self) -> Option<String> {
        match self {
            Self::Kiwi(s) => KiwiControls::link_error(s),
            _ => None,
        }
    }

    pub fn link_alive(&self) -> bool {
        match self {
            Self::Kiwi(s) => KiwiControls::link_alive(s),
            _ => true,
        }
    }
}

/// IQ consumer plus metadata for a connected [`DeviceSource`].
pub struct Connection {
    pub device: DeviceSource,
    pub iq: Consumer<Complex32>,
    pub iq_ring_capacity: usize,
    /// Native device IQ rate (full passband for demod and display).
    pub device_sample_rate: f32,
    /// Rate after optional client-side decimation (spectrum FFT path only).
    pub sample_rate: f32,
    pub center_hz: f64,
    pub is_kiwi: bool,
    /// Client-side integer decimation for the spectrum path (1 = none).
    pub iq_ingress_decim: usize,
}
