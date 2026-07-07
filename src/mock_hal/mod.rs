//! In-process mock IQ sources for tests and coverage runs (no USB / network hardware).
#![cfg(any(test, coverage, mock_hal))]

use std::cell::Cell;
use std::f32::consts::TAU;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};
use std::time::Duration;

use rtrb::{Consumer, RingBuffer};

use crate::source::Complex32;

thread_local! {
    static ENABLED: Cell<bool> = const { Cell::new(false) };
}

/// Enable mock HAL backends until the matching [`MockGuard`] is dropped (per thread).
pub fn enable() {
    ENABLED.with(|e| e.set(true));
}

pub fn disable() {
    ENABLED.with(|e| e.set(false));
}

pub fn enabled() -> bool {
    ENABLED.with(|e| e.get())
}

/// RAII toggle for mock HAL in unit tests.
pub struct MockGuard;

impl Default for MockGuard {
    fn default() -> Self {
        Self::new()
    }
}

impl MockGuard {
    pub fn new() -> Self {
        enable();
        Self
    }
}

impl Drop for MockGuard {
    fn drop(&mut self) {
        disable();
    }
}

/// Background tone generator feeding a ring buffer (implements the streaming side of HAL).
pub struct MockIqStream {
    stop: Arc<AtomicBool>,
    thread: Option<JoinHandle<()>>,
    pub dropped: Arc<AtomicU64>,
}

impl MockIqStream {
    pub fn start(sample_rate: u32, ring_cap: usize) -> (Self, Consumer<Complex32>) {
        let (mut prod, cons) = RingBuffer::<Complex32>::new(ring_cap.max(4096));
        let stop = Arc::new(AtomicBool::new(false));
        let dropped = Arc::new(AtomicU64::new(0));
        let stop_t = Arc::clone(&stop);
        let dropped_t = Arc::clone(&dropped);
        let sr = sample_rate.max(1);
        let thread = thread::Builder::new()
            .name("mock-iq".into())
            .spawn(move || {
                let mut phase = 0.0f32;
                let inc = TAU * 700.0 / sr as f32;
                while !stop_t.load(Ordering::Relaxed) {
                    let sample = Complex32::new(phase.cos() * 0.25, phase.sin() * 0.25);
                    if prod.push(sample).is_err() {
                        dropped_t.fetch_add(1, Ordering::Relaxed);
                    }
                    phase += inc;
                    if prod.slots() == 0 {
                        thread::sleep(Duration::from_micros(200));
                    }
                }
            })
            .expect("mock iq thread");
        (
            Self {
                stop,
                thread: Some(thread),
                dropped,
            },
            cons,
        )
    }

    pub fn stop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(h) = self.thread.take() {
            let _ = h.join();
        }
    }
}

impl Drop for MockIqStream {
    fn drop(&mut self) {
        self.stop();
    }
}

/// In-memory QMX CAT port (no serial device).
#[derive(Clone, Debug, Default)]
pub struct MockCat {
    pub vfo_hz: u64,
    pub rf_gain_db: u8,
    pub iq_mode: bool,
    pub cat_timeout: bool,
    pub cw_mode: bool,
    pub transmitting: bool,
    pub smeter_db: f32,
}

impl MockCat {
    pub fn send(&mut self, cmd: &str) -> crate::source::Result<()> {
        if cmd.contains('\r') || cmd.contains('\n') {
            return Err(crate::source::SourceError::Unsupported(
                "CAT commands must not contain CR/LF".into(),
            ));
        }
        if let Some(hz) = cmd.strip_prefix("FA").and_then(|s| s.strip_suffix(';')) {
            if let Ok(v) = hz.parse::<u64>() {
                self.vfo_hz = v;
            }
        } else if let Some(g) = cmd.strip_prefix("RG").and_then(|s| s.strip_suffix(';')) {
            if let Ok(v) = g.parse::<u8>() {
                self.rf_gain_db = v.min(99);
            }
        } else if cmd.starts_with("Q9") {
            self.iq_mode = cmd.contains("Q91");
        } else if cmd.starts_with("QB") {
            self.cat_timeout = cmd.contains("QB1");
        } else if cmd == "MD3;" {
            self.cw_mode = true;
        } else if cmd == "RX;" {
            self.transmitting = false;
        }
        Ok(())
    }

    pub fn query(&mut self, cmd: &str) -> crate::source::Result<String> {
        self.send(cmd)?;
        if cmd.starts_with("TQ;") {
            return Ok(format!("TQ{};", u8::from(self.transmitting)));
        }
        if cmd.starts_with("SM;") {
            let raw = ((self.smeter_db + 127.0) * 10.0).round().clamp(0.0, 9999.0) as u32;
            return Ok(format!("SM{raw:04};"));
        }
        Ok(String::new())
    }

    pub fn set_iq_mode(&mut self, on: bool) -> crate::source::Result<()> {
        self.iq_mode = on;
        self.send(&format!("Q9{};", u8::from(on)))
    }

    pub fn ensure_receive(&mut self) -> crate::source::Result<()> {
        self.transmitting = false;
        self.send("RX;")
    }

    pub fn set_cat_timeout_enabled(&mut self, on: bool) -> crate::source::Result<()> {
        self.cat_timeout = on;
        self.send(&format!("QB{};", u8::from(on)))
    }

    pub fn set_vfo_a_hz(&mut self, hz: u64) -> crate::source::Result<()> {
        self.vfo_hz = hz;
        self.send(&format!("FA{hz:011};"))
    }

    pub fn set_rf_gain_db(&mut self, db: u8) -> crate::source::Result<()> {
        self.rf_gain_db = db.min(99);
        self.send(&format!("RG{db:03};"))
    }

    pub fn set_operating_mode_cw(&mut self) -> crate::source::Result<()> {
        self.cw_mode = true;
        self.send("MD3;")
    }

    pub fn is_transmitting(&mut self) -> crate::source::Result<bool> {
        Ok(self.transmitting)
    }

    pub fn read_smeter_db(&mut self) -> crate::source::Result<Option<f32>> {
        Ok(Some(self.smeter_db))
    }
}

#[cfg(any(test, coverage, mock_hal))]
/// Device args for mock Pluto (USB) used in tests.
pub const MOCK_PLUTO_USB_ARGS: &str =
    "driver=plutosdr,label=Mock Pluto USB,serial=MOCKPLUTO001";

#[cfg(any(test, coverage, mock_hal))]
/// Device args for mock Pluto (network) used in tests.
pub const MOCK_PLUTO_NET_ARGS: &str =
    "driver=plutosdr,label=Mock Pluto Network,uri=ip:192.168.2.1";

#[cfg(any(test, coverage, mock_hal))]
const MOCK_SOAPY_CATALOG: &[(&str, &str)] = &[
    ("Mock Pluto USB · MOCKPLUTO001", MOCK_PLUTO_USB_ARGS),
    ("Mock Pluto Network · 192.168.2.1", MOCK_PLUTO_NET_ARGS),
    (
        "Mock RTL-Soapy · 00000001",
        "driver=rtlsdr,label=Mock RTL-Soapy,serial=00000001",
    ),
];

#[cfg(any(test, coverage, mock_hal))]
pub fn soapy_enumerate(driver: &str) -> Vec<(String, String)> {
    let filter = driver.trim();
    MOCK_SOAPY_CATALOG
        .iter()
        .filter(|(_, args)| {
            filter.is_empty() || args.contains(&format!("driver={filter},"))
        })
        .map(|(label, args)| ((*label).to_string(), (*args).to_string()))
        .collect()
}

#[cfg(any(test, coverage, mock_hal))]
pub fn soapy_mock_openable(args: &str) -> bool {
    let trimmed = args.trim();
    MOCK_SOAPY_CATALOG.iter().any(|(_, a)| *a == trimmed)
        || trimmed.starts_with("driver=plutosdr")
        || trimmed.starts_with("driver=rtlsdr")
        || trimmed.starts_with("driver=mock")
}

#[cfg(any(test, coverage, mock_hal))]
pub fn soapy_mock_driver(args: &str) -> String {
    for part in args.split(',') {
        if let Some(d) = part.strip_prefix("driver=") {
            return d.trim().to_string();
        }
    }
    "mock".into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mock_iq_stream_pumps_samples() {
        let (_guard, mut stream, mut cons) = {
            let _guard = MockGuard::new();
            let (stream, cons) = MockIqStream::start(48_000, 4096);
            (_guard, stream, cons)
        };
        std::thread::sleep(Duration::from_millis(50));
        assert!(cons.pop().is_ok());
        stream.stop();
    }

    #[test]
    fn mock_cat_vfo_and_smeter() {
        let mut cat = MockCat::default();
        cat.set_vfo_a_hz(14_010_000).unwrap();
        assert_eq!(cat.vfo_hz, 14_010_000);
        cat.smeter_db = -73.0;
        assert_eq!(cat.read_smeter_db().unwrap(), Some(-73.0));
    }

    #[test]
    fn mock_soapy_enumerate_plutosdr() {
        let _guard = MockGuard::new();
        let devices = soapy_enumerate("plutosdr");
        assert_eq!(devices.len(), 2);
        assert!(devices[0].1.contains("driver=plutosdr"));
    }

    #[test]
    fn mock_soapy_enumerate_all_drivers() {
        let _guard = MockGuard::new();
        assert!(soapy_enumerate("").len() >= 3);
    }
}
