//! Safe wrapper over librtlsdr implementing [`IqSource`].

mod sys;

use crate::source::{Complex32, Consumer, IqSource, Result, SourceError};
use rtrb::{Producer, RingBuffer};
use std::ffi::CStr;
use std::os::raw::{c_int, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::thread::{self, JoinHandle};

/// Common RTL-SDR sample rates (Hz). The hardware accepts other values in
/// 225001–300000 and 900001–3200000 ranges; these are practical presets.
pub const SAMPLE_RATES: &[u32] = &[
    250_000,
    1_024_000,
    1_200_000,
    1_280_000,
    1_600_000,
    1_920_000,
    2_048_000,
    2_400_000,
    2_560_000,
    3_200_000,
];

/// Default sample rate for HF work (upconverter or wideband IQ).
pub const DEFAULT_SAMPLE_RATE: u32 = 2_048_000;

struct StreamCtx {
    prod: Producer<Complex32>,
    dropped: Arc<AtomicU64>,
}

extern "C" fn stream_cb(buf: *mut u8, len: u32, ctx: *mut c_void) {
    if ctx.is_null() || buf.is_null() || len < 2 {
        return;
    }
    let ctx = unsafe { &mut *(ctx as *mut StreamCtx) };
    let raw = unsafe { std::slice::from_raw_parts(buf, len as usize) };
    let sample_count = raw.len() / 2;
    if sample_count == 0 {
        return;
    }

    let avail = ctx.prod.slots();
    let to_write = sample_count.min(avail);
    if to_write == 0 {
        ctx.dropped
            .fetch_add(sample_count as u64, Ordering::Relaxed);
        return;
    }

    if let Ok(mut chunk) = ctx.prod.write_chunk_uninit(to_write) {
        let (first, second) = chunk.as_mut_slices();
        let mut out = first.iter_mut().chain(second.iter_mut());
        for pair in raw.chunks_exact(2).take(to_write) {
            let re = (pair[0] as f32 - 127.5) / 128.0;
            let im = (pair[1] as f32 - 127.5) / 128.0;
            out.next().unwrap().write(Complex32::new(re, im));
        }
        unsafe { chunk.commit_all() };
    }
    let dropped = sample_count - to_write;
    if dropped > 0 {
        ctx.dropped.fetch_add(dropped as u64, Ordering::Relaxed);
    }
}

/// An RTL2832-based USB dongle front end.
pub struct RtlSdr {
    dev: *mut sys::rtlsdr_dev_t,
    device_index: u32,
    sample_rate: u32,
    freq_hz: f64,
    tuner_gains: Vec<i32>,
    streaming: bool,
    dropped: Arc<AtomicU64>,
    ctx: Option<Box<StreamCtx>>,
    async_thread: Option<JoinHandle<()>>,
}

impl RtlSdr {
    /// Open the first available RTL-SDR (`index` 0).
    pub fn open() -> Result<Self> {
        Self::open_index(0)
    }

    /// Open a specific device by index (see [`RtlSdr::device_count`]).
    pub fn open_index(index: u32) -> Result<Self> {
        let mut dev: *mut sys::rtlsdr_dev_t = std::ptr::null_mut();
        let rc = unsafe { sys::rtlsdr_open(&mut dev, index) };
        if rc != sys::SUCCESS || dev.is_null() {
            return Err(SourceError::NotFound);
        }
        let mut sdr = Self {
            dev,
            device_index: index,
            sample_rate: 0,
            freq_hz: 0.0,
            tuner_gains: read_tuner_gains(dev),
            streaming: false,
            dropped: Arc::new(AtomicU64::new(0)),
            ctx: None,
            async_thread: None,
        };
        sdr.set_sample_rate(DEFAULT_SAMPLE_RATE)?;
        Ok(sdr)
    }

    pub fn device_count() -> u32 {
        unsafe { sys::rtlsdr_get_device_count() }
    }

    pub fn device_name(index: u32) -> String {
        let ptr = unsafe { sys::rtlsdr_get_device_name(index) };
        if ptr.is_null() {
            return format!("RTL-SDR #{index}");
        }
        unsafe { CStr::from_ptr(ptr) }
            .to_string_lossy()
            .into_owned()
    }

    pub fn device_index(&self) -> u32 {
        self.device_index
    }

    pub fn tuner_gains(&self) -> &[i32] {
        &self.tuner_gains
    }

    /// Nearest supported tuner gain (tenths of dB) at or below `gain_db10`.
    pub fn clamp_tuner_gain(&self, gain_db10: i32) -> i32 {
        self.tuner_gains
            .iter()
            .copied()
            .filter(|&g| g <= gain_db10)
            .max()
            .or_else(|| self.tuner_gains.first().copied())
            .unwrap_or(0)
    }

    pub fn set_rtl_agc(&mut self, on: bool) -> Result<()> {
        check("rtlsdr_set_agc_mode", unsafe {
            sys::rtlsdr_set_agc_mode(self.dev, on as c_int)
        })
    }

    pub fn set_direct_sampling(&mut self, mode: u8) -> Result<()> {
        if mode > 2 {
            return Err(SourceError::Unsupported(format!(
                "direct sampling mode {mode} (use 0, 1, or 2)"
            )));
        }
        check("rtlsdr_set_direct_sampling", unsafe {
            sys::rtlsdr_set_direct_sampling(self.dev, mode as c_int)
        })
    }

    pub fn set_offset_tuning(&mut self, on: bool) -> Result<()> {
        check("rtlsdr_set_offset_tuning", unsafe {
            sys::rtlsdr_set_offset_tuning(self.dev, on as c_int)
        })
    }

    pub fn set_bias_tee(&mut self, on: bool) -> Result<()> {
        check("rtlsdr_set_bias_tee", unsafe { sys::rtlsdr_set_bias_tee(self.dev, on as c_int) })
    }
}

impl IqSource for RtlSdr {
    fn sample_rates(&self) -> Vec<u32> {
        SAMPLE_RATES.to_vec()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn set_sample_rate(&mut self, sr: u32) -> Result<()> {
        if self.streaming {
            return Err(SourceError::InvalidState("stop before changing sample rate"));
        }
        check("rtlsdr_set_sample_rate", unsafe {
            sys::rtlsdr_set_sample_rate(self.dev, sr)
        })?;
        let actual = unsafe { sys::rtlsdr_get_sample_rate(self.dev) };
        self.sample_rate = if actual == 0 { sr } else { actual };
        Ok(())
    }

    fn tune(&mut self, hz: f64) -> Result<()> {
        let freq = hz.round().clamp(0.0, u32::MAX as f64) as u32;
        check("rtlsdr_set_center_freq", unsafe {
            sys::rtlsdr_set_center_freq(self.dev, freq)
        })?;
        self.freq_hz = hz;
        Ok(())
    }

    fn frequency(&self) -> f64 {
        self.freq_hz
    }

    fn start(&mut self) -> Result<Consumer<Complex32>> {
        if self.streaming {
            return Err(SourceError::InvalidState("already streaming"));
        }
        let capacity = iq_ring_capacity(self.sample_rate);
        let (prod, cons) = RingBuffer::<Complex32>::new(capacity);

        let mut ctx = Box::new(StreamCtx {
            prod,
            dropped: Arc::clone(&self.dropped),
        });
        let ctx_ptr = (&mut *ctx as *mut StreamCtx) as *mut c_void;

        check("rtlsdr_reset_buffer", unsafe { sys::rtlsdr_reset_buffer(self.dev) })?;

        let dev_addr = self.dev as usize;
        let ctx_addr = ctx_ptr as usize;
        let handle = thread::Builder::new()
            .name("rtlsdr-async".into())
            .spawn(move || {
                let dev = dev_addr as *mut sys::rtlsdr_dev_t;
                let ctx = ctx_addr as *mut c_void;
                let _ = unsafe { sys::rtlsdr_read_async(dev, stream_cb, ctx, 0, 0) };
            })
            .map_err(|_| SourceError::Backend {
                op: "spawn rtlsdr async thread",
                code: -1,
            })?;

        self.ctx = Some(ctx);
        self.async_thread = Some(handle);
        self.streaming = true;
        Ok(cons)
    }

    fn stop(&mut self) -> Result<()> {
        if !self.streaming {
            return Ok(());
        }
        let rc = unsafe { sys::rtlsdr_cancel_async(self.dev) };
        if let Some(handle) = self.async_thread.take() {
            let _ = handle.join();
        }
        self.streaming = false;
        self.ctx = None;
        check("rtlsdr_cancel_async", rc)
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn is_streaming(&self) -> bool {
        self.streaming
    }

    fn set_agc(&mut self, on: bool) -> Result<()> {
        self.set_rtl_agc(on)
    }

    fn set_tuner_gain(&mut self, gain_db10: i32) -> Result<()> {
        let gain = self.clamp_tuner_gain(gain_db10);
        check("rtlsdr_set_tuner_gain", unsafe {
            sys::rtlsdr_set_tuner_gain(self.dev, gain)
        })
    }

    fn set_tuner_gain_mode(&mut self, manual: bool) -> Result<()> {
        check("rtlsdr_set_tuner_gain_mode", unsafe {
            sys::rtlsdr_set_tuner_gain_mode(self.dev, manual as c_int)
        })
    }

    fn set_freq_correction(&mut self, ppm: i32) -> Result<()> {
        check("rtlsdr_set_freq_correction", unsafe {
            sys::rtlsdr_set_freq_correction(self.dev, ppm)
        })
    }

    fn set_bias_tee(&mut self, on: bool) -> Result<()> {
        RtlSdr::set_bias_tee(self, on)
    }
}

impl Drop for RtlSdr {
    fn drop(&mut self) {
        let _ = self.stop();
        if !self.dev.is_null() {
            unsafe { sys::rtlsdr_close(self.dev) };
            self.dev = std::ptr::null_mut();
        }
    }
}

/// ~2 s of IQ at the selected rate (power-of-two, clamped for memory).
pub fn iq_ring_capacity(sample_rate: u32) -> usize {
    let target = (sample_rate as usize).saturating_mul(2);
    target.next_power_of_two().clamp(1 << 18, 1 << 21)
}

fn read_tuner_gains(dev: *mut sys::rtlsdr_dev_t) -> Vec<i32> {
    let count = unsafe { sys::rtlsdr_get_tuner_gains(dev, std::ptr::null_mut()) };
    if count <= 0 {
        return Vec::new();
    }
    let mut gains = vec![0i32; count as usize];
    let got = unsafe { sys::rtlsdr_get_tuner_gains(dev, gains.as_mut_ptr()) };
    gains.truncate(got.max(0) as usize);
    gains
}

fn check(op: &'static str, rc: c_int) -> Result<()> {
    if rc == sys::SUCCESS {
        Ok(())
    } else {
        Err(SourceError::Backend { op, code: rc })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_rate_presets_non_empty() {
        assert!(SAMPLE_RATES.contains(&DEFAULT_SAMPLE_RATE));
    }

    #[test]
    fn iq_ring_capacity_scales_with_rate() {
        assert!(iq_ring_capacity(2_048_000) >= iq_ring_capacity(250_000));
    }

    #[test]
    fn stream_cb_decodes_unsigned_iq() {
        let (prod, mut cons) = RingBuffer::<Complex32>::new(8);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut ctx = StreamCtx {
            prod,
            dropped: Arc::clone(&dropped),
        };
        let mut raw = [255u8, 127, 128, 0];
        stream_cb(
            raw.as_mut_ptr(),
            raw.len() as u32,
            &mut ctx as *mut StreamCtx as *mut c_void,
        );
        drop(ctx);
        let mut out = Vec::new();
        while let Ok(s) = cons.pop() {
            out.push(s);
        }
        assert_eq!(out.len(), 2);
        assert!(out[0].re > 0.9);
    }

    #[test]
    fn check_maps_success_and_error() {
        assert!(check("ok", sys::SUCCESS).is_ok());
        assert!(check("fail", -1).is_err());
    }
}
