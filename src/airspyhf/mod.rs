//! Safe wrapper over libairspyhf implementing [`IqSource`], plus Airspy HF+
//! specific controls (calibration, HF AGC / attenuator / LNA, Low-IF query)
//! that a generic HAL would hide.

mod sys;

pub use sys::{FLAGS_OPTIMIZE_BAND_III, FLAGS_OPTIMIZE_PLL_INT_BOUNDARY};

use crate::source::controls::AirspyControls;
use crate::source::{Complex32, Consumer, IqSource, Result, SourceError};
use rtrb::{Producer, RingBuffer};
use std::os::raw::{c_char, c_int, c_void};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

/// State shared with the streaming callback. Boxed so its address is stable
/// while the C side holds a raw pointer to it; only the USB thread touches
/// `prod`, only via that pointer, so there is no aliasing on the Rust side.
struct StreamCtx {
    prod: Producer<Complex32>,
    dropped: Arc<AtomicU64>,
}

/// Runs on libairspyhf's USB thread. Copies IQ into the ring and returns.
/// No allocation, no locks, no blocking — on overflow it drops and counts.
extern "C" fn stream_cb(transfer: *mut sys::airspyhf_transfer_t) -> c_int {
    // SAFETY: libairspyhf guarantees `transfer`, the `samples`/`sample_count`
    // it points to, and the `ctx` pointer (the `StreamCtx` we passed to
    // `airspyhf_start`) are valid for the duration of this call.
    let t = unsafe { &*transfer };
    if t.ctx.is_null() || t.samples.is_null() {
        return 0;
    }
    let ctx = unsafe { &mut *(t.ctx as *mut StreamCtx) };
    let n = t.sample_count.max(0) as usize;
    let iq = unsafe { std::slice::from_raw_parts(t.samples, n) };

    let avail = ctx.prod.slots();
    let to_write = n.min(avail);
    if to_write > 0 {
        if let Ok(mut chunk) = ctx.prod.write_chunk_uninit(to_write) {
            let (first, second) = chunk.as_mut_slices();
            for (slot, &sample) in first
                .iter_mut()
                .chain(second.iter_mut())
                .zip(iq.iter().take(to_write))
            {
                slot.write(sample);
            }
            // SAFETY: every one of the `to_write` slots was initialized above.
            unsafe { chunk.commit_all() };
        }
    }
    let dropped = (n - to_write) as u64;
    if dropped > 0 {
        ctx.dropped.fetch_add(dropped, Ordering::Relaxed);
    }
    0
}

/// An Airspy HF+ front end.
pub struct AirspyHf {
    dev: *mut sys::airspyhf_device_t,
    rates: Vec<u32>,
    sample_rate: u32,
    freq_hz: f64,
    streaming: bool,
    dropped: Arc<AtomicU64>,
    // Kept alive (and pinned) for as long as the C side may call back.
    ctx: Option<Box<StreamCtx>>,
}

impl AirspyHf {
    /// Library version string, e.g. `"1.6.8"`. Does not require a device.
    pub fn lib_version() -> String {
        let mut v = sys::airspyhf_lib_version_t {
            major_version: 0,
            minor_version: 0,
            revision: 0,
        };
        // SAFETY: writes into a valid local struct.
        unsafe { sys::airspyhf_lib_version(&mut v) };
        format!("{}.{}.{}", v.major_version, v.minor_version, v.revision)
    }

    /// Open the first available Airspy HF+.
    pub fn open() -> Result<Self> {
        let mut dev: *mut sys::airspyhf_device_t = std::ptr::null_mut();
        // SAFETY: `dev` is a valid out-pointer.
        let rc = unsafe { sys::airspyhf_open(&mut dev) };
        if rc != sys::SUCCESS || dev.is_null() {
            return Err(SourceError::NotFound);
        }
        let rates = read_sample_rates(dev);
        let sample_rate = rates.first().copied().unwrap_or(0);
        Ok(Self {
            dev,
            rates,
            sample_rate,
            freq_hz: 0.0,
            streaming: false,
            dropped: Arc::new(AtomicU64::new(0)),
            ctx: None,
        })
    }

    /// Open a specific unit by serial number.
    pub fn open_serial(serial: u64) -> Result<Self> {
        let mut dev: *mut sys::airspyhf_device_t = std::ptr::null_mut();
        // SAFETY: `dev` is a valid out-pointer.
        let rc = unsafe { sys::airspyhf_open_sn(&mut dev, serial) };
        if rc != sys::SUCCESS || dev.is_null() {
            return Err(SourceError::NotFound);
        }
        let rates = read_sample_rates(dev);
        let sample_rate = rates.first().copied().unwrap_or(0);
        Ok(Self {
            dev,
            rates,
            sample_rate,
            freq_hz: 0.0,
            streaming: false,
            dropped: Arc::new(AtomicU64::new(0)),
            ctx: None,
        })
    }

    /// Serial numbers of all attached Airspy HF+ units.
    pub fn list_devices() -> Vec<u64> {
        // SAFETY: count query with a null buffer.
        let count = unsafe { sys::airspyhf_list_devices(std::ptr::null_mut(), 0) };
        if count <= 0 {
            return Vec::new();
        }
        let mut serials = vec![0u64; count as usize];
        // SAFETY: buffer holds `count` slots.
        let got = unsafe { sys::airspyhf_list_devices(serials.as_mut_ptr(), count) };
        serials.truncate(got.max(0) as usize);
        serials
    }

    /// Enable/disable the library's IQ correction, IF shift, and fine tuning.
    pub fn set_lib_dsp(&mut self, on: bool) -> Result<()> {
        check("airspyhf_set_lib_dsp", unsafe {
            sys::airspyhf_set_lib_dsp(self.dev, on as u8)
        })
    }

    /// Frequency calibration in parts-per-billion.
    pub fn set_calibration_ppb(&mut self, ppb: i32) -> Result<()> {
        check("airspyhf_set_calibration", unsafe {
            sys::airspyhf_set_calibration(self.dev, ppb)
        })
    }

    /// Read the stored calibration (ppb).
    pub fn calibration_ppb(&self) -> Result<i32> {
        let mut ppb = 0i32;
        // SAFETY: `ppb` is a valid out-pointer.
        let rc = unsafe { sys::airspyhf_get_calibration(self.dev, &mut ppb) };
        check("airspyhf_get_calibration", rc).map(|_| ppb)
    }

    /// HF AGC on/off.
    pub fn set_hf_agc(&mut self, on: bool) -> Result<()> {
        check("airspyhf_set_hf_agc", unsafe {
            sys::airspyhf_set_hf_agc(self.dev, on as u8)
        })
    }

    /// HF AGC threshold: `false` = low, `true` = high.
    pub fn set_hf_agc_threshold(&mut self, high: bool) -> Result<()> {
        check("airspyhf_set_hf_agc_threshold", unsafe {
            sys::airspyhf_set_hf_agc_threshold(self.dev, high as u8)
        })
    }

    /// HF attenuator step, 0..=8 (0..48 dB in 6 dB steps).
    pub fn set_hf_att(&mut self, step: u8) -> Result<()> {
        if step > 8 {
            return Err(SourceError::Unsupported(format!(
                "attenuator step {step} out of range 0..=8"
            )));
        }
        check("airspyhf_set_hf_att", unsafe {
            sys::airspyhf_set_hf_att(self.dev, step)
        })
    }

    /// LNA / preamp on/off (+6 dB, compensated digitally).
    pub fn set_hf_lna(&mut self, on: bool) -> Result<()> {
        check("airspyhf_set_hf_lna", unsafe {
            sys::airspyhf_set_hf_lna(self.dev, on as u8)
        })
    }

    /// Frontend option flags (`sys::FLAGS_*`). Discovery/Ranger band-tracking
    /// preselectors are automatic; these tune VHF Band-III and PLL behavior.
    /// Requires libairspyhf >= 1.8.
    pub fn set_frontend_options(&mut self, flags: u32) -> Result<()> {
        #[cfg(airspyhf_extended_api)]
        {
            return check("airspyhf_set_frontend_options", unsafe {
                sys::airspyhf_set_frontend_options(self.dev, flags)
            });
        }
        #[cfg(not(airspyhf_extended_api))]
        {
            let _ = flags;
            Err(extended_api_unsupported("frontend options"))
        }
    }

    /// Requires libairspyhf >= 1.8.
    pub fn frontend_options(&self) -> Result<u32> {
        #[cfg(airspyhf_extended_api)]
        {
            let mut flags = 0u32;
            let rc = unsafe { sys::airspyhf_get_frontend_options(self.dev, &mut flags) };
            return check("airspyhf_get_frontend_options", rc).map(|_| flags);
        }
        #[cfg(not(airspyhf_extended_api))]
        Err(extended_api_unsupported("frontend options"))
    }

    /// Antenna-port bias tee: powers external preamps/upconverters (0 = off, 1 = on).
    /// Requires libairspyhf >= 1.8.
    pub fn set_bias_tee(&mut self, on: bool) -> Result<()> {
        #[cfg(airspyhf_extended_api)]
        {
            return check("airspyhf_set_bias_tee", unsafe {
                sys::airspyhf_set_bias_tee(self.dev, on as i8)
            });
        }
        #[cfg(not(airspyhf_extended_api))]
        {
            let _ = on;
            Err(extended_api_unsupported("bias tee"))
        }
    }

    /// Whether the current sample rate runs the radio in Low-IF mode.
    pub fn is_low_if(&self) -> bool {
        // SAFETY: valid device handle.
        unsafe { sys::airspyhf_is_low_if(self.dev) == 1 }
    }

    /// IQ samples delivered per callback at the current sample rate.
    pub fn output_size(&self) -> usize {
        // SAFETY: valid device handle.
        let n = unsafe { sys::airspyhf_get_output_size(self.dev) };
        n.max(0) as usize
    }

    /// Firmware version string reported by the device.
    pub fn firmware_version(&self) -> String {
        let mut buf = [0u8; 64];
        // SAFETY: 64-byte buffer; the library writes a NUL-terminated string.
        let rc = unsafe {
            sys::airspyhf_version_string_read(
                self.dev,
                buf.as_mut_ptr() as *mut c_char,
                buf.len() as u8,
            )
        };
        if rc != sys::SUCCESS {
            return String::new();
        }
        let end = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
        String::from_utf8_lossy(&buf[..end]).into_owned()
    }
}

impl IqSource for AirspyHf {
    fn sample_rates(&self) -> Vec<u32> {
        self.rates.clone()
    }

    fn sample_rate(&self) -> u32 {
        self.sample_rate
    }

    fn set_sample_rate(&mut self, sr: u32) -> Result<()> {
        if self.streaming {
            return Err(SourceError::InvalidState("stop before changing sample rate"));
        }
        if !self.rates.is_empty() && !self.rates.contains(&sr) {
            return Err(SourceError::Unsupported(format!("sample rate {sr} not supported")));
        }
        check("airspyhf_set_samplerate", unsafe {
            sys::airspyhf_set_samplerate(self.dev, sr)
        })?;
        self.sample_rate = sr;
        Ok(())
    }

    fn tune(&mut self, hz: f64) -> Result<()> {
        let freq = hz.round().clamp(0.0, u32::MAX as f64) as u32;
        check("airspyhf_set_freq", unsafe {
            sys::airspyhf_set_freq(self.dev, freq)
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
        // Raw pointer to the heap allocation; stays valid after `ctx` is moved
        // into `self.ctx`, because moving a Box does not move its pointee.
        let ctx_ptr = (&mut *ctx as *mut StreamCtx) as *mut c_void;

        // SAFETY: `stream_cb` matches the expected signature and `ctx_ptr` lives
        // until `airspyhf_stop` returns (we hold the Box in `self.ctx`).
        let rc = unsafe { sys::airspyhf_start(self.dev, stream_cb, ctx_ptr) };
        check("airspyhf_start", rc)?;

        self.ctx = Some(ctx);
        self.streaming = true;
        Ok(cons)
    }

    fn stop(&mut self) -> Result<()> {
        if !self.streaming {
            return Ok(());
        }
        // SAFETY: valid device handle; `airspyhf_stop` blocks until the callback
        // has returned for the last time, so dropping `ctx` afterwards is sound.
        let rc = unsafe { sys::airspyhf_stop(self.dev) };
        self.streaming = false;
        self.ctx = None;
        check("airspyhf_stop", rc)
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn is_streaming(&self) -> bool {
        self.streaming
    }
}

impl AirspyControls for AirspyHf {
    fn set_hf_att(&mut self, step: u8) -> Result<()> {
        AirspyHf::set_hf_att(self, step)
    }

    fn set_hf_lna(&mut self, on: bool) -> Result<()> {
        AirspyHf::set_hf_lna(self, on)
    }

    fn set_hf_agc_threshold(&mut self, high: bool) -> Result<()> {
        AirspyHf::set_hf_agc_threshold(self, high)
    }

    fn set_frontend_options(&mut self, flags: u32) -> Result<()> {
        AirspyHf::set_frontend_options(self, flags)
    }

    fn set_bias_tee(&mut self, on: bool) -> Result<()> {
        AirspyHf::set_bias_tee(self, on)
    }

    fn set_agc(&mut self, on: bool) -> Result<()> {
        AirspyHf::set_hf_agc(self, on)
    }
}

/// ~2 s of IQ at the selected rate (power-of-two, clamped for memory).
pub fn iq_ring_capacity(sample_rate: u32) -> usize {
    let target = (sample_rate as usize).saturating_mul(2);
    target.next_power_of_two().clamp(1 << 18, 1 << 21)
}

impl Drop for AirspyHf {
    fn drop(&mut self) {
        if self.streaming {
            // SAFETY: stop the stream before freeing the callback context.
            unsafe { sys::airspyhf_stop(self.dev) };
            self.ctx = None;
        }
        if !self.dev.is_null() {
            // SAFETY: opened by us, closed exactly once.
            unsafe { sys::airspyhf_close(self.dev) };
            self.dev = std::ptr::null_mut();
        }
    }
}

/// Two-call sample-rate enumeration: query the count, then fill the buffer.
fn read_sample_rates(dev: *mut sys::airspyhf_device_t) -> Vec<u32> {
    let mut count: u32 = 0;
    // SAFETY: with `len == 0`, libairspyhf writes the count into `*buffer`.
    unsafe { sys::airspyhf_get_samplerates(dev, &mut count, 0) };
    if count == 0 {
        return Vec::new();
    }
    let mut buf = vec![0u32; count as usize];
    // SAFETY: `buf` has `count` slots.
    let rc = unsafe { sys::airspyhf_get_samplerates(dev, buf.as_mut_ptr(), count) };
    if rc != sys::SUCCESS {
        return Vec::new();
    }
    buf
}

fn check(op: &'static str, rc: c_int) -> Result<()> {
    if rc == sys::SUCCESS {
        Ok(())
    } else {
        Err(SourceError::Backend { op, code: rc })
    }
}

#[cfg(not(airspyhf_extended_api))]
fn extended_api_unsupported(feature: &'static str) -> SourceError {
    SourceError::Unsupported(format!(
        "{feature} requires libairspyhf >= 1.8 (upgrade libairspyhf-dev / brew install airspyhf)"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    #[test]
    fn iq_ring_capacity_scales_and_clamps() {
        let low = iq_ring_capacity(250_000);
        let high = iq_ring_capacity(2_048_000);
        assert!(high >= low);
        assert!(low >= 1 << 18);
        assert!(high <= 1 << 21);
        assert_eq!(iq_ring_capacity(0), 1 << 18);
    }

    #[test]
    fn frontend_flag_constants() {
        assert_eq!(FLAGS_OPTIMIZE_BAND_III, 1);
        assert_eq!(FLAGS_OPTIMIZE_PLL_INT_BOUNDARY, 2);
    }

    #[test]
    fn check_maps_success_and_error() {
        assert!(check("ok", sys::SUCCESS).is_ok());
        let err = check("fail", -1).unwrap_err();
        assert!(matches!(err, SourceError::Backend { op: "fail", code: -1 }));
    }

    #[test]
    fn stream_cb_writes_iq_samples() {
        let (prod, mut cons) = RingBuffer::<Complex32>::new(8);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut ctx = StreamCtx {
            prod,
            dropped: Arc::clone(&dropped),
        };
        let samples = [
            Complex32::new(1.0, 0.0),
            Complex32::new(0.0, -1.0),
        ];
        let mut transfer = sys::airspyhf_transfer_t {
            device: std::ptr::null_mut(),
            ctx: (&mut ctx as *mut StreamCtx).cast(),
            samples: samples.as_ptr().cast_mut(),
            sample_count: 2,
            dropped_samples: 0,
        };
        assert_eq!(stream_cb(&mut transfer), 0);
        drop(ctx);

        let s0 = cons.pop().expect("sample 0");
        assert!((s0.re - 1.0).abs() < 1e-6);
        assert!(s0.im.abs() < 1e-6);
        let s1 = cons.pop().expect("sample 1");
        assert!(s1.re.abs() < 1e-6);
        assert!((s1.im + 1.0).abs() < 1e-6);
        assert_eq!(dropped.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn stream_cb_drops_on_full_ring() {
        let (prod, _cons) = RingBuffer::<Complex32>::new(1);
        let dropped = Arc::new(AtomicU64::new(0));
        let mut ctx = StreamCtx {
            prod,
            dropped: Arc::clone(&dropped),
        };
        let samples = [
            Complex32::new(1.0, 0.0),
            Complex32::new(0.5, 0.0),
            Complex32::new(0.25, 0.0),
        ];
        let mut transfer = sys::airspyhf_transfer_t {
            device: std::ptr::null_mut(),
            ctx: (&mut ctx as *mut StreamCtx).cast(),
            samples: samples.as_ptr().cast_mut(),
            sample_count: 3,
            dropped_samples: 0,
        };
        stream_cb(&mut transfer);
        drop(ctx);
        assert_eq!(dropped.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn stream_cb_ignores_null_pointers() {
        let mut transfer = sys::airspyhf_transfer_t {
            device: std::ptr::null_mut(),
            ctx: std::ptr::null_mut(),
            samples: std::ptr::null_mut(),
            sample_count: 4,
            dropped_samples: 0,
        };
        assert_eq!(stream_cb(&mut transfer), 0);
    }
}
