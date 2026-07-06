//! Safe wrapper over libSoapySDR implementing [`IqSource`].

mod sys;

use crate::source::controls::SoapyControls;
use crate::source::{Complex32, Consumer, IqSource, Result, SourceError};
use rtrb::{Producer, RingBuffer};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

pub use sys::SOAPY_SDR_TIMEOUT;

pub fn library_loaded() -> bool {
    sys::library_loaded()
}

pub fn prepare_environment() {
    sys::prepare_environment();
}

pub fn load_driver_modules() {
    #[cfg(any(test, coverage, mock_hal))]
    if crate::mock_hal::enabled() {
        return;
    }
    sys::load_driver_modules();
}

pub fn list_search_paths() -> Vec<String> {
    #[cfg(any(test, coverage, mock_hal))]
    if crate::mock_hal::enabled() {
        return vec!["/mock/SoapySDR/modules".into()];
    }
    sys::list_search_paths()
}

pub fn list_module_paths() -> Vec<String> {
    #[cfg(any(test, coverage, mock_hal))]
    if crate::mock_hal::enabled() {
        return vec![
            "/mock/SoapySDR/modules/libSoapyPlutoSDR.so".into(),
            "/mock/SoapySDR/modules/libSoapyRTLSDR.so".into(),
        ];
    }
    sys::list_module_paths()
}

/// Installed SoapySDR driver keys (e.g. `plutosdr`, `rtlsdr`).
pub fn available_driver_keys() -> Vec<String> {
    #[cfg(any(test, coverage, mock_hal))]
    if crate::mock_hal::enabled() {
        return vec!["plutosdr".into(), "rtlsdr".into()];
    }
    sys::available_driver_keys()
}

/// Log SoapySDR library status, plugin paths, drivers, and device counts to stderr.
pub fn log_startup_status() {
    if !library_loaded() {
        crate::log::warn(
            "SoapySDR: libSoapySDR not loaded — install system package or bundle next to hfsdr",
        );
        return;
    }
    load_driver_modules();
    crate::log::info("SoapySDR: libSoapySDR loaded");
    let paths = list_search_paths();
    if paths.is_empty() {
        if let Ok(env) = std::env::var("SOAPY_SDR_PLUGIN_PATH") {
            crate::log::info(format!("SoapySDR plugin path (env): {env}"));
        } else {
            crate::log::warn("SoapySDR: no plugin search paths reported");
        }
    } else {
        crate::log::info(format!("SoapySDR plugin paths: {}", paths.join(", ")));
    }
    let modules = list_module_paths();
    let drivers = available_driver_keys();
    if drivers.is_empty() {
        crate::log::warn(
            "SoapySDR: no driver modules found — install e.g. soapysdr-module-plutosdr \
             (set SOAPY_SDR_PLUGIN_PATH if modules are in a non-standard location)",
        );
    } else {
        crate::log::info(format!("SoapySDR drivers: {}", drivers.join(", ")));
        crate::log::debug(format!("SoapySDR modules: {}", modules.join(", ")));
    }
    for driver in &drivers {
        let count = enumerate_devices(driver).len();
        if count == 0 {
            crate::log::info(format!("SoapySDR: driver '{driver}' — 0 devices attached"));
        } else {
            crate::log::info(format!("SoapySDR: driver '{driver}' — {count} device(s)"));
        }
    }
    if !drivers.contains(&"plutosdr".to_string()) {
        crate::log::warn(
            "SoapySDR: plutosdr driver module not installed — Pluto needs soapysdr-module-plutosdr",
        );
    }
}

pub fn enumerate_devices(driver: &str) -> Vec<(String, String)> {
    #[cfg(any(test, coverage, mock_hal))]
    if crate::mock_hal::enabled() {
        return crate::mock_hal::soapy_enumerate(driver);
    }
    sys::enumerate_devices(driver)
}

pub fn enumeration_hint(driver: &str) -> String {
    #[cfg(any(test, coverage, mock_hal))]
    if crate::mock_hal::enabled() {
        let devices = crate::mock_hal::soapy_enumerate(driver);
        if devices.is_empty() {
            return format!("No mock devices for driver '{driver}'");
        }
        return String::new();
    }
    sys::enumeration_hint(driver)
}

pub fn last_error() -> String {
    sys::last_error_message()
}

/// Open the device briefly and return its reported RX sample rates.
pub fn probe_sample_rates(device_args: &str) -> Result<Vec<u32>> {
    let src = SoapySource::open(device_args)?;
    Ok(src.sample_rates())
}

/// Pick the nearest device-supported rate (exact match when present).
pub fn snap_sample_rate(requested: u32, supported: &[u32]) -> u32 {
    if supported.is_empty() {
        return requested;
    }
    if supported.contains(&requested) {
        return requested;
    }
    supported
        .iter()
        .min_by_key(|rate| rate.abs_diff(requested))
        .copied()
        .unwrap_or(requested)
}

/// Human-readable label for a sample rate preset combobox.
pub fn format_sample_rate(hz: u32) -> String {
    if hz >= 1_000_000 {
        let mhz = hz as f64 / 1_000_000.0;
        if (mhz - mhz.round()).abs() < 0.001 {
            format!("{} MHz", mhz.round() as u32)
        } else {
            format!("{mhz:.3} MHz")
        }
    } else if hz >= 1_000 {
        format!("{} kHz", hz / 1_000)
    } else {
        format!("{hz} Hz")
    }
}

/// readStream timeout (µs). First read in a burst; follow-ups use [`READ_BURST_TIMEOUT_US`].
const READ_TIMEOUT_US: i64 = 10_000;
const READ_BURST_TIMEOUT_US: i64 = 500;
const READ_BURST_MAX: usize = 64;

#[cfg(any(test, coverage, mock_hal))]
struct SoapyMock {
    stream: Option<crate::mock_hal::MockIqStream>,
    gain_db: f64,
    agc: bool,
    antenna: String,
}

/// A SoapySDR-backed local front end (RTL-SDR, Airspy, HackRF, Pluto, …).
pub struct SoapySource {
    dev: *mut sys::SoapySDRDevice,
    stream: Option<*mut sys::SoapySDRStream>,
    device_args: String,
    driver: String,
    sample_rate: u32,
    sample_rates: Vec<u32>,
    freq_hz: f64,
    antennas: Vec<String>,
    gain_min: f64,
    gain_max: f64,
    streaming: bool,
    dropped: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    read_thread: Option<JoinHandle<()>>,
    prod: Option<Producer<Complex32>>,
    #[cfg(any(test, coverage, mock_hal))]
    mock: Option<SoapyMock>,
}

#[cfg(any(test, coverage, mock_hal))]
impl SoapySource {
    fn mock_mut(&mut self) -> Option<&mut SoapyMock> {
        self.mock.as_mut()
    }

    fn mock_pluto_sample_rates() -> Vec<u32> {
        vec![520_834, 1_041_667, 2_083_333, 3_125_000, 4_166_667, 6_250_000]
    }
}

impl SoapySource {
    pub fn open(device_args: &str) -> Result<Self> {
        let args = device_args.trim();
        if args.is_empty() {
            return Err(SourceError::Unsupported(
                "SoapySDR device args are empty".into(),
            ));
        }

        #[cfg(any(test, coverage, mock_hal))]
        if crate::mock_hal::enabled() {
            if !crate::mock_hal::soapy_mock_openable(args) {
                return Err(SourceError::NotFound);
            }
            let driver = crate::mock_hal::soapy_mock_driver(args);
            return Ok(Self {
                dev: std::ptr::null_mut(),
                stream: None,
                device_args: args.to_string(),
                driver,
                sample_rate: 0,
                sample_rates: Self::mock_pluto_sample_rates(),
                freq_hz: 0.0,
                antennas: vec!["A".into(), "B".into()],
                gain_min: 0.0,
                gain_max: 73.0,
                streaming: false,
                dropped: Arc::new(AtomicU64::new(0)),
                stop: Arc::new(AtomicBool::new(false)),
                read_thread: None,
                prod: None,
                mock: Some(SoapyMock {
                    stream: None,
                    gain_db: 30.0,
                    agc: false,
                    antenna: "A".into(),
                }),
            });
        }

        if !sys::library_loaded() {
            return Err(SourceError::NotFound);
        }
        let dev = sys::make_device(args).ok_or(SourceError::NotFound)?;
        let driver = sys::driver_key(dev);
        let sample_rates = sys::list_sample_rates(dev);
        let antennas = sys::list_antennas(dev);
        let (gain_min, gain_max) = sys::gain_range(dev);
        Ok(Self {
            dev,
            stream: None,
            device_args: args.to_string(),
            driver,
            sample_rate: 0,
            sample_rates,
            freq_hz: 0.0,
            antennas,
            gain_min,
            gain_max,
            streaming: false,
            dropped: Arc::new(AtomicU64::new(0)),
            stop: Arc::new(AtomicBool::new(false)),
            read_thread: None,
            prod: None,
            #[cfg(any(test, coverage, mock_hal))]
            mock: None,
        })
    }

    pub fn device_args(&self) -> &str {
        &self.device_args
    }

    pub fn driver(&self) -> &str {
        &self.driver
    }

    pub fn antennas(&self) -> &[String] {
        &self.antennas
    }

    pub fn gain_range_db(&self) -> (f64, f64) {
        (self.gain_min, self.gain_max)
    }

    pub fn set_antenna_name(&mut self, name: &str) -> Result<()> {
        if name.is_empty() {
            return Ok(());
        }
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            m.antenna = name.to_string();
            return Ok(());
        }
        check("SoapySDRDevice_setAntenna", sys::set_antenna(self.dev, name))
    }

    pub fn set_overall_gain(&mut self, db: f64) -> Result<()> {
        let clamped = db.clamp(self.gain_min, self.gain_max);
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            m.gain_db = clamped;
            return Ok(());
        }
        check("SoapySDRDevice_setGain", sys::set_gain(self.dev, clamped))
    }

    pub fn set_automatic_gain(&mut self, on: bool) -> Result<()> {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = self.mock_mut() {
            m.agc = on;
            return Ok(());
        }
        check("SoapySDRDevice_setGainMode", sys::set_gain_mode(self.dev, on))
    }
}

impl IqSource for SoapySource {
    fn sample_rates(&self) -> Vec<u32> {
        self.sample_rates.clone()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn set_sample_rate(&mut self, sr: u32) -> Result<()> {
        if self.streaming {
            return Err(SourceError::InvalidState(
                "cannot change sample rate while streaming",
            ));
        }
        #[cfg(any(test, coverage, mock_hal))]
        if self.mock_mut().is_some() {
            self.sample_rate = sr;
            return Ok(());
        }
        check("SoapySDRDevice_setSampleRate", sys::set_sample_rate(self.dev, sr))?;
        let actual = sys::get_sample_rate(self.dev);
        self.sample_rate = if actual == 0 { sr } else { actual };
        Ok(())
    }

    fn tune(&mut self, hz: f64) -> Result<()> {
        #[cfg(any(test, coverage, mock_hal))]
        if self.mock_mut().is_some() {
            self.freq_hz = hz;
            return Ok(());
        }
        check("SoapySDRDevice_setFrequency", sys::set_frequency(self.dev, hz))?;
        self.freq_hz = sys::get_frequency(self.dev);
        Ok(())
    }

    fn frequency(&self) -> f64 {
        self.freq_hz
    }

    fn start(&mut self) -> Result<Consumer<Complex32>> {
        if self.streaming {
            return Err(SourceError::InvalidState("already streaming"));
        }

        #[cfg(any(test, coverage, mock_hal))]
        if self.mock.is_some() {
            let sr = self.sample_rate.max(48_000);
            let capacity = iq_ring_capacity(sr);
            let (stream, cons) = crate::mock_hal::MockIqStream::start(sr, capacity);
            if let Some(mock) = self.mock.as_mut() {
                mock.stream = Some(stream);
            }
            self.streaming = true;
            return Ok(cons);
        }

        let stream = sys::setup_rx_stream(self.dev).ok_or(SourceError::NotFound)?;
        check("SoapySDRDevice_activateStream", sys::activate_stream(self.dev, stream))?;

        let capacity = iq_ring_capacity(self.sample_rate);
        let (prod, cons) = RingBuffer::<Complex32>::new(capacity);
        let mtu = sys::stream_mtu(self.dev, stream).max(256);
        let stop = Arc::new(AtomicBool::new(false));
        let dropped = Arc::clone(&self.dropped);
        let dev_addr = self.dev as usize;
        let stream_addr = stream as usize;
        let stop_thread = Arc::clone(&stop);

        let handle = thread::Builder::new()
            .name("soapy-rx".into())
            .spawn(move || {
                soapy_read_loop(dev_addr, stream_addr, prod, dropped, stop_thread, mtu);
            })
            .map_err(|_| SourceError::Backend {
                op: "spawn soapy read thread",
                code: -1,
            })?;

        self.stream = Some(stream);
        self.stop = stop;
        self.prod = None;
        self.read_thread = Some(handle);
        self.streaming = true;
        Ok(cons)
    }

    fn stop(&mut self) -> Result<()> {
        if !self.streaming {
            return Ok(());
        }

        #[cfg(any(test, coverage, mock_hal))]
        if let Some(mock) = self.mock_mut() {
            if let Some(stream) = mock.stream.take() {
                drop(stream);
            }
            self.streaming = false;
            return Ok(());
        }

        self.stop.store(true, Ordering::Relaxed);
        if let Some(handle) = self.read_thread.take() {
            let _ = handle.join();
        }
        if let Some(stream) = self.stream.take() {
            let _ = sys::deactivate_stream(self.dev, stream);
            let _ = sys::close_stream(self.dev, stream);
        }
        self.streaming = false;
        self.prod = None;
        Ok(())
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn is_streaming(&self) -> bool {
        self.streaming
    }
}

impl SoapyControls for SoapySource {
    fn set_gain_db(&mut self, db: f64) -> Result<()> {
        SoapySource::set_overall_gain(self, db)
    }

    fn set_agc(&mut self, on: bool) -> Result<()> {
        SoapySource::set_automatic_gain(self, on)
    }

    fn set_antenna(&mut self, name: &str) -> Result<()> {
        SoapySource::set_antenna_name(self, name)
    }

    fn gain_db(&self) -> f64 {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = &self.mock {
            return m.gain_db;
        }
        sys::get_gain(self.dev)
    }

    fn agc_on(&self) -> bool {
        #[cfg(any(test, coverage, mock_hal))]
        if let Some(m) = &self.mock {
            return m.agc;
        }
        sys::get_gain_mode(self.dev)
    }
}

impl Drop for SoapySource {
    fn drop(&mut self) {
        let _ = self.stop();
        #[cfg(any(test, coverage, mock_hal))]
        if self.mock.is_some() {
            return;
        }
        if !self.dev.is_null() {
            sys::unmake_device(self.dev);
            self.dev = std::ptr::null_mut();
        }
    }
}

fn soapy_read_loop(
    dev_addr: usize,
    stream_addr: usize,
    mut prod: Producer<Complex32>,
    dropped: Arc<AtomicU64>,
    stop: Arc<AtomicBool>,
    mtu: usize,
) {
    let dev = dev_addr as *mut sys::SoapySDRDevice;
    let stream = stream_addr as *mut sys::SoapySDRStream;
    let mut buffer = vec![Complex32::new(0.0, 0.0); mtu];
    while !stop.load(Ordering::Relaxed) {
        let mut burst = 0usize;
        while burst < READ_BURST_MAX && !stop.load(Ordering::Relaxed) {
            let slots = prod.slots();
            if slots == 0 {
                break;
            }

            let read_count = slots.min(mtu).max(1);
            let timeout = if burst == 0 {
                READ_TIMEOUT_US
            } else {
                READ_BURST_TIMEOUT_US
            };
            let buf_ptr = buffer.as_mut_ptr() as *mut c_void;
            let bufs: [*mut c_void; 1] = [buf_ptr];
            let n = sys::read_stream(dev, stream, bufs.as_ptr(), read_count, timeout);
            if n == sys::SOAPY_SDR_TIMEOUT {
                break;
            }
            if n <= 0 {
                if stop.load(Ordering::Relaxed) {
                    return;
                }
                break;
            }

            let count = n as usize;
            let avail = prod.slots();
            let to_write = count.min(avail);
            if to_write == 0 {
                dropped.fetch_add(count as u64, Ordering::Relaxed);
                break;
            }
            if let Ok(mut chunk) = prod.write_chunk_uninit(to_write) {
                let (first, second) = chunk.as_mut_slices();
                let mut out = first.iter_mut().chain(second.iter_mut());
                for sample in buffer.iter().take(to_write) {
                    out.next().unwrap().write(*sample);
                }
                unsafe { chunk.commit_all() };
            }
            let lost = count - to_write;
            if lost > 0 {
                dropped.fetch_add(lost as u64, Ordering::Relaxed);
            }
            burst += 1;
        }

        if burst == 0 {
            thread::yield_now();
        }
    }
}

/// ~2 s of IQ at the selected rate (power-of-two, clamped for memory).
pub fn iq_ring_capacity(sample_rate: u32) -> usize {
    let target = (sample_rate as usize).saturating_mul(2);
    target.next_power_of_two().clamp(1 << 18, 1 << 21)
}

pub fn default_sample_rate(rates: &[u32]) -> u32 {
    const PREFERRED: [u32; 4] = [2_048_000, 1_920_000, 768_000, 384_000];
    for p in PREFERRED {
        if rates.contains(&p) {
            return p;
        }
    }
    rates.first().copied().unwrap_or(2_048_000)
}

fn check(op: &'static str, rc: i32) -> Result<()> {
    if rc == 0 {
        Ok(())
    } else {
        Err(SourceError::Backend {
            op,
            code: rc,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::mock_hal::{MockGuard, MOCK_PLUTO_USB_ARGS};

    #[test]
    fn default_sample_rate_prefers_2048k() {
        let rates = &[250_000, 2_048_000, 3_200_000];
        assert_eq!(default_sample_rate(rates), 2_048_000);
    }

    #[test]
    fn iq_ring_capacity_scales_with_rate() {
        assert!(iq_ring_capacity(2_048_000) >= iq_ring_capacity(250_000));
    }

    #[test]
    fn mock_enumerate_finds_pluto() {
        let _guard = MockGuard::new();
        let devices = enumerate_devices("plutosdr");
        assert_eq!(devices.len(), 2);
    }

    #[test]
    fn mock_open_tune_gain_stream_lifecycle() {
        let _guard = MockGuard::new();
        let mut src = SoapySource::open(MOCK_PLUTO_USB_ARGS).expect("open mock pluto");
        assert_eq!(src.driver(), "plutosdr");
        assert_eq!(src.antennas(), &["A", "B"]);
        assert!(!src.sample_rates().is_empty());

        src.set_sample_rate(2_083_333).unwrap();
        assert_eq!(src.sample_rate(), 2_083_333);
        src.tune(14_010_000.0).unwrap();
        assert_eq!(src.frequency(), 14_010_000.0);
        src.set_automatic_gain(false).unwrap();
        src.set_overall_gain(40.0).unwrap();
        assert_eq!(src.gain_db(), 40.0);
        src.set_antenna_name("B").unwrap();

        let mut cons = src.start().expect("start mock stream");
        std::thread::sleep(std::time::Duration::from_millis(30));
        assert!(cons.pop().is_ok());
        src.stop().unwrap();
        assert!(!src.is_streaming());
    }

    #[test]
    fn mock_open_rejects_unknown_args() {
        let _guard = MockGuard::new();
        assert!(SoapySource::open("driver=unknown,serial=x").is_err());
    }

    #[test]
    fn enumeration_hint_when_lib_missing() {
        let hint = enumeration_hint("plutosdr");
        assert!(!hint.is_empty() || library_loaded());
    }

    #[test]
    fn snap_sample_rate_exact_match() {
        let rates = &[384_000, 768_000, 912_000];
        assert_eq!(snap_sample_rate(768_000, rates), 768_000);
    }

    #[test]
    fn snap_sample_rate_nearest() {
        let rates = &[384_000, 768_000, 912_000];
        assert_eq!(snap_sample_rate(700_000, rates), 768_000);
        assert_eq!(snap_sample_rate(400_000, rates), 384_000);
    }

    #[test]
    fn snap_sample_rate_empty_supported() {
        assert_eq!(snap_sample_rate(2_048_000, &[]), 2_048_000);
    }

    #[test]
    fn probe_sample_rates_mock_pluto() {
        let _guard = MockGuard::new();
        let rates = probe_sample_rates(MOCK_PLUTO_USB_ARGS).expect("probe");
        assert!(rates.contains(&2_083_333));
    }

    #[test]
    fn format_sample_rate_labels() {
        assert_eq!(format_sample_rate(384_000), "384 kHz");
        assert!(format_sample_rate(2_048_000).contains("2.048"));
        assert!(format_sample_rate(2_048_000).ends_with("MHz"));
    }

    #[test]
    fn mock_available_driver_keys_include_plutosdr() {
        let _guard = MockGuard::new();
        let drivers = available_driver_keys();
        assert!(drivers.contains(&"plutosdr".to_string()));
    }

    #[test]
    fn mock_startup_status_does_not_panic() {
        let _guard = MockGuard::new();
        log_startup_status();
    }

    #[test]
    fn mock_pluto_network_open_and_stream() {
        use crate::mock_hal::MOCK_PLUTO_NET_ARGS;

        let _guard = MockGuard::new();
        let mut src = SoapySource::open(MOCK_PLUTO_NET_ARGS).expect("open network pluto");
        assert_eq!(src.driver(), "plutosdr");
        assert!(src.sample_rates().contains(&2_083_333));
        src.set_sample_rate(2_083_333).unwrap();
        src.tune(7_100_000.0).unwrap();
        let mut cons = src.start().expect("stream");
        std::thread::sleep(std::time::Duration::from_millis(20));
        assert!(cons.pop().is_ok());
        src.stop().unwrap();
    }

    #[test]
    fn mock_pluto_default_sample_rate_from_device_list() {
        let _guard = MockGuard::new();
        let rates = SoapySource::open(MOCK_PLUTO_USB_ARGS)
            .expect("open")
            .sample_rates();
        let picked = default_sample_rate(&rates);
        assert!(rates.contains(&picked));
    }

    #[test]
    fn mock_enumerate_plutosdr_driver_filter() {
        let _guard = MockGuard::new();
        let devices = enumerate_devices("plutosdr");
        assert_eq!(devices.len(), 2);
        assert!(devices.iter().all(|(_, args)| args.contains("driver=plutosdr")));
    }
}
