//! Safe wrapper over libSoapySDR implementing [`IqSource`].

mod sys;

use crate::source::controls::SoapyControls;
use crate::source::{Complex32, Consumer, IqSource, Result, SourceError};
use rtrb::{Producer, RingBuffer};
use std::ffi::c_void;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

pub use sys::enumerate_devices;

pub fn last_error() -> String {
    sys::last_error_message()
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
}

impl SoapySource {
    pub fn open(device_args: &str) -> Result<Self> {
        if !sys::library_loaded() {
            return Err(SourceError::NotFound);
        }
        let args = device_args.trim();
        if args.is_empty() {
            return Err(SourceError::Unsupported(
                "SoapySDR device args are empty".into(),
            ));
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
        check("SoapySDRDevice_setAntenna", sys::set_antenna(self.dev, name))
    }

    pub fn set_overall_gain(&mut self, db: f64) -> Result<()> {
        let clamped = db.clamp(self.gain_min, self.gain_max);
        check("SoapySDRDevice_setGain", sys::set_gain(self.dev, clamped))
    }

    pub fn set_automatic_gain(&mut self, on: bool) -> Result<()> {
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
        check("SoapySDRDevice_setSampleRate", sys::set_sample_rate(self.dev, sr))?;
        let actual = sys::get_sample_rate(self.dev);
        self.sample_rate = if actual == 0 { sr } else { actual };
        Ok(())
    }

    fn tune(&mut self, hz: f64) -> Result<()> {
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
        sys::get_gain(self.dev)
    }

    fn agc_on(&self) -> bool {
        sys::get_gain_mode(self.dev)
    }
}

impl Drop for SoapySource {
    fn drop(&mut self) {
        let _ = self.stop();
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
        let buf_ptr = buffer.as_mut_ptr() as *mut c_void;
        let bufs: [*mut c_void; 1] = [buf_ptr];
        let n = sys::read_stream(dev, stream, bufs.as_ptr(), mtu, 100_000);
        if n == sys::SOAPY_SDR_TIMEOUT {
            continue;
        }
        if n <= 0 {
            if stop.load(Ordering::Relaxed) {
                break;
            }
            continue;
        }
        let count = n as usize;
        let avail = prod.slots();
        let to_write = count.min(avail);
        if to_write == 0 {
            dropped.fetch_add(count as u64, Ordering::Relaxed);
            continue;
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

    #[test]
    fn default_sample_rate_prefers_2048k() {
        let rates = &[250_000, 2_048_000, 3_200_000];
        assert_eq!(default_sample_rate(rates), 2_048_000);
    }

    #[test]
    fn iq_ring_capacity_scales_with_rate() {
        assert!(iq_ring_capacity(2_048_000) >= iq_ring_capacity(250_000));
    }
}
