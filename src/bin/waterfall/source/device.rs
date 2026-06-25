//! Connected front end as a typed enum (IQ streaming + device-specific controls).

use hfsdr::{KiwiControls, QmxControls, Complex32, Consumer, IqSource, KiwiSource, Result};
use rtrb::RingBuffer;
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

use super::iq_bridge::IqDualRingBridge;

/// IQ consumer plus metadata for a connected [`DeviceSource`].
pub struct Connection {
    pub device: DeviceSource,
    /// Raw device-rate IQ (demod, recording).
    pub iq: Consumer<Complex32>,
    /// Pre-decimated IQ for spectrum/skimmer when [`iq_ingress_decim`] > 1.
    pub iq_spectrum: Option<Consumer<Complex32>>,
    /// Keeps the ingress bridge thread alive.
    pub(crate) bridge: Option<IqDualRingBridge>,
    pub iq_ring_capacity: usize,
    pub iq_spectrum_ring_capacity: usize,
    /// Native device IQ rate (full passband for demod and display).
    pub device_sample_rate: f32,
    /// Rate after optional client-side decimation (spectrum FFT path only).
    pub sample_rate: f32,
    pub center_hz: f64,
    pub is_kiwi: bool,
    /// Client-side integer decimation for the spectrum path (1 = none).
    pub iq_ingress_decim: usize,
}

impl Connection {
    pub fn dual_ring_active(&self) -> bool {
        self.iq_spectrum.is_some()
    }

    pub fn bridge_decim_dropped(&self) -> u64 {
        self.bridge
            .as_ref()
            .map(IqDualRingBridge::decim_dropped)
            .unwrap_or(0)
    }

    pub fn bridge_raw_dropped(&self) -> u64 {
        self.bridge
            .as_ref()
            .map(IqDualRingBridge::raw_dropped)
            .unwrap_or(0)
    }

    pub fn sync_bridge_decim_filter(&self, kind: hfsdr::DecimFilterKind) {
        if let Some(bridge) = &self.bridge {
            bridge.set_decim_filter(kind);
        }
    }
}

impl Connection {
    /// When Kiwi RF AGC is on, firmware ignores `manGain` — return the same dB offset for software scaling.
    pub fn kiwi_software_man_gain_db(&self) -> f32 {
        if !self.is_kiwi {
            return 0.0;
        }
        let DeviceSource::Kiwi(kiwi) = &self.device else {
            return 0.0;
        };
        if !KiwiControls::rf_agc_on(kiwi) {
            return 0.0;
        }
        KiwiControls::hw_rf_gain(kiwi)
            .map(|mg| hfsdr::kiwi::protocol::man_gain_db_below_max(mg) as f32)
            .unwrap_or(0.0)
    }

    /// Ring buffer pre-filled with IQ for engine pump tests and benchmarks.
    pub(crate) fn mock_ring(samples: &[Complex32], center_hz: f64, is_kiwi: bool) -> Self {
        let (mut prod, cons) = RingBuffer::<Complex32>::new(65_536);
        for &s in samples {
            let _ = prod.push(s);
        }
        Connection {
            device: DeviceSource::Kiwi(KiwiSource::new("test.local", 8073)),
            iq: cons,
            iq_spectrum: None,
            bridge: None,
            iq_ring_capacity: 65_536,
            iq_spectrum_ring_capacity: 0,
            device_sample_rate: 12_000.0,
            sample_rate: 12_000.0,
            center_hz,
            is_kiwi,
            iq_ingress_decim: 1,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use hfsdr::kiwi::KiwiSource;
    use hfsdr::source::controls::KiwiControls;
    use rtrb::RingBuffer;

    fn kiwi_conn(agc_on: bool, man_gain: u8) -> Connection {
        let mut kiwi = KiwiSource::new("test.local", 8073);
        KiwiControls::set_agc(&mut kiwi, agc_on).unwrap();
        KiwiControls::set_man_gain(&mut kiwi, man_gain).unwrap();
        let (_prod, cons) = RingBuffer::new(64);
        Connection {
            device: DeviceSource::Kiwi(kiwi),
            iq: cons,
            iq_spectrum: None,
            bridge: None,
            iq_ring_capacity: 64,
            iq_spectrum_ring_capacity: 0,
            device_sample_rate: 12_000.0,
            sample_rate: 12_000.0,
            center_hz: 14_000_000.0,
            is_kiwi: true,
            iq_ingress_decim: 1,
        }
    }

    #[test]
    fn kiwi_man_gain_emulated_when_rf_agc_on() {
        let conn = kiwi_conn(true, 50);
        assert!((conn.kiwi_software_man_gain_db() - (-50.0)).abs() < 0.01);
    }

    #[test]
    fn kiwi_man_gain_not_emulated_when_rf_agc_off() {
        let conn = kiwi_conn(false, 50);
        assert!((conn.kiwi_software_man_gain_db()).abs() < 1e-6);
    }

    #[test]
    fn combined_rf_gain_clamps_with_kiwi_emulation() {
        let conn = kiwi_conn(true, 50);
        let combined = (10.0 + conn.kiwi_software_man_gain_db()).clamp(-80.0, 80.0);
        assert!((combined - (-40.0)).abs() < 0.01);
    }
}
