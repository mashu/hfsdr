//! QRP Labs QMX / QMX+ front end: CAT serial control + USB audio IQ capture.
//!
//! IQ mode streams raw 48 ksps I/Q on the built-in USB sound card (left = I,
//! right = Q). VFO tuning uses Kenwood `FA` commands; the receiver LO is
//! offset by [`DEFAULT_IF_OFFSET_HZ`] below the displayed center frequency.

mod cat;

pub use cat::{list_serial_ports, CatPort};

use std::sync::atomic::{AtomicBool, AtomicU64, AtomicI32, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{SampleFormat, Stream};
use rtrb::{Producer, RingBuffer};

use crate::source::controls::QmxControls;
use crate::source::{Complex32, Consumer, IqSource, Result, SourceError};

/// Native IQ sample rate from the QMX USB sound card.
pub const SAMPLE_RATE: u32 = 48_000;

/// 12 kHz superhet IF — LO tunes this far below the displayed RX frequency.
pub const DEFAULT_IF_OFFSET_HZ: i32 = 12_000;

/// Ring capacity for 48 kHz (~340 ms).
pub fn iq_ring_capacity() -> usize {
    1 << 16
}

struct AudioCtx {
    prod: Producer<Complex32>,
    dropped: Arc<AtomicU64>,
    transmitting: Arc<AtomicBool>,
}

/// QMX / QMX+ IQ source (CAT + USB audio).
pub struct QmxSource {
    cat: Arc<Mutex<CatPort>>,
    cat_port_name: String,
    audio_device_name: String,
    center_hz: f64,
    if_offset_hz: i32,
    rf_gain_db: u8,
    iq_mode_was_enabled: bool,
    streaming: bool,
    dropped: Arc<AtomicU64>,
    smeter_db: Arc<AtomicI32>,
    transmitting: Arc<AtomicBool>,
    cat_poll_stop: Arc<AtomicBool>,
    audio_stream: Option<Stream>,
    cat_poll_thread: Option<JoinHandle<()>>,
}

impl QmxSource {
    pub fn open(
        serial_port: &str,
        audio_device: &str,
        if_offset_hz: i32,
        rf_gain_db: u8,
        disable_cat_timeout: bool,
        force_cw_mode: bool,
    ) -> Result<Self> {
        let cat_path = resolve_serial_port(serial_port)?;
        let mut cat = CatPort::open(&cat_path)?;
        if disable_cat_timeout {
            cat.set_cat_timeout_enabled(false)?;
        }
        if force_cw_mode {
            cat.set_operating_mode_cw()?;
        }
        cat.ensure_receive()?;
        cat.set_iq_mode(true)?;
        if rf_gain_db > 0 {
            cat.set_rf_gain_db(rf_gain_db)?;
        }

        Ok(Self {
            cat: Arc::new(Mutex::new(cat)),
            cat_port_name: cat_path,
            audio_device_name: resolve_audio_device(audio_device)?,
            center_hz: 0.0,
            if_offset_hz,
            rf_gain_db,
            iq_mode_was_enabled: true,
            streaming: false,
            dropped: Arc::new(AtomicU64::new(0)),
            smeter_db: Arc::new(AtomicI32::new(i32::MIN)),
            transmitting: Arc::new(AtomicBool::new(false)),
            cat_poll_stop: Arc::new(AtomicBool::new(false)),
            audio_stream: None,
            cat_poll_thread: None,
        })
    }

    pub fn cat_port_name(&self) -> &str {
        &self.cat_port_name
    }

    pub fn audio_device_name(&self) -> &str {
        &self.audio_device_name
    }

    pub fn if_offset_hz(&self) -> i32 {
        self.if_offset_hz
    }

    fn vfo_hz_for_center(&self, center_hz: f64) -> u64 {
        (center_hz - self.if_offset_hz as f64)
            .round()
            .clamp(0.0, 999_999_999_999.0) as u64
    }
}

impl IqSource for QmxSource {
    fn sample_rates(&self) -> Vec<u32> {
        vec![SAMPLE_RATE]
    }

    fn sample_rate(&self) -> u32 {
        SAMPLE_RATE
    }

    fn set_sample_rate(&mut self, sr: u32) -> Result<()> {
        if sr != SAMPLE_RATE {
            return Err(SourceError::Unsupported(format!(
                "QMX IQ rate is fixed at {SAMPLE_RATE} Hz"
            )));
        }
        Ok(())
    }

    fn tune(&mut self, hz: f64) -> Result<()> {
        self.cat
            .lock()
            .map_err(|_| SourceError::InvalidState("cat mutex poisoned"))?
            .set_vfo_a_hz(self.vfo_hz_for_center(hz))?;
        self.center_hz = hz;
        Ok(())
    }

    fn frequency(&self) -> f64 {
        self.center_hz
    }

    fn start(&mut self) -> Result<Consumer<Complex32>> {
        if self.streaming {
            return Err(SourceError::InvalidState("already streaming"));
        }
        let (prod, cons) = RingBuffer::<Complex32>::new(iq_ring_capacity());
        let ctx = AudioCtx {
            prod,
            dropped: Arc::clone(&self.dropped),
            transmitting: Arc::clone(&self.transmitting),
        };

        let host = cpal::default_host();
        let device = find_input_device(&host, &self.audio_device_name)?;
        let config = pick_input_config(&device)?;
        let sample_format = config.sample_format();
        let stream_config = config.config();
        let channels = stream_config.channels as usize;

        let err_fn = |_| {};
        let stream = match sample_format {
            SampleFormat::F32 => {
                let mut ctx = ctx;
                device.build_input_stream(
                    &stream_config,
                    move |data: &[f32], _| {
                        push_interleaved_f32(data, channels, &mut ctx);
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::I32 => {
                let mut ctx = ctx;
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i32], _| {
                        push_interleaved_i32(data, channels, &mut ctx);
                    },
                    err_fn,
                    None,
                )
            }
            SampleFormat::I16 => {
                let mut ctx = ctx;
                device.build_input_stream(
                    &stream_config,
                    move |data: &[i16], _| {
                        push_interleaved_i16(data, channels, &mut ctx);
                    },
                    err_fn,
                    None,
                )
            }
            other => {
                return Err(SourceError::Unsupported(format!(
                    "unsupported QMX audio sample format: {other:?}"
                )));
            }
        }
        .map_err(|e| SourceError::Unsupported(format!("audio stream: {e}")))?;

        stream
            .play()
            .map_err(|e| SourceError::Unsupported(format!("audio play: {e}")))?;

        self.cat_poll_stop.store(false, Ordering::Relaxed);
        let cat = Arc::clone(&self.cat);
        let smeter_db = Arc::clone(&self.smeter_db);
        let transmitting = Arc::clone(&self.transmitting);
        let cat_poll_stop = Arc::clone(&self.cat_poll_stop);
        let cat_poll_thread = thread::Builder::new()
            .name("qmx-cat".into())
            .spawn(move || cat_poll_loop(cat, smeter_db, transmitting, cat_poll_stop))
            .map_err(|e| SourceError::Unsupported(e.to_string()))?;

        self.audio_stream = Some(stream);
        self.cat_poll_thread = Some(cat_poll_thread);
        self.streaming = true;
        Ok(cons)
    }

    fn stop(&mut self) -> Result<()> {
        if !self.streaming {
            return Ok(());
        }
        self.cat_poll_stop.store(true, Ordering::Relaxed);
        self.audio_stream = None;
        if let Some(h) = self.cat_poll_thread.take() {
            let _ = h.join();
        }
        self.streaming = false;
        Ok(())
    }

    fn dropped_samples(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    fn is_streaming(&self) -> bool {
        self.streaming
    }
}

impl QmxSource {
    pub fn set_rf_gain_db(&mut self, db: u8) -> Result<()> {
        let db = db.min(99);
        self.cat
            .lock()
            .map_err(|_| SourceError::InvalidState("cat mutex poisoned"))?
            .set_rf_gain_db(db)?;
        self.rf_gain_db = db;
        Ok(())
    }

    pub fn rssi_dbm(&self) -> Option<f32> {
        let raw = self.smeter_db.load(Ordering::Relaxed);
        if raw == i32::MIN {
            None
        } else {
            Some(raw as f32)
        }
    }
}

impl QmxControls for QmxSource {
    fn set_rf_gain_db(&mut self, db: u8) -> Result<()> {
        QmxSource::set_rf_gain_db(self, db)
    }

    fn rssi_dbm(&self) -> Option<f32> {
        QmxSource::rssi_dbm(self)
    }
}

impl Drop for QmxSource {
    fn drop(&mut self) {
        let _ = self.stop();
        if self.iq_mode_was_enabled {
            if let Ok(mut cat) = self.cat.lock() {
                let _ = cat.set_iq_mode(false);
            }
        }
    }
}

fn cat_poll_loop(
    cat: Arc<Mutex<CatPort>>,
    smeter_db: Arc<AtomicI32>,
    transmitting: Arc<AtomicBool>,
    stop: Arc<AtomicBool>,
) {
    while !stop.load(Ordering::Relaxed) {
        let Ok(mut guard) = cat.lock() else {
            break;
        };
        if let Ok(tx) = guard.is_transmitting() {
            transmitting.store(tx, Ordering::Relaxed);
        }
        if let Ok(Some(db)) = guard.read_smeter_db() {
            smeter_db.store(db as i32, Ordering::Relaxed);
        }
        drop(guard);
        thread::sleep(std::time::Duration::from_millis(250));
    }
}

fn push_interleaved_f32(data: &[f32], channels: usize, ctx: &mut AudioCtx) {
    if channels < 2 {
        return;
    }
    let frames = data.len() / channels;
    if ctx.transmitting.load(Ordering::Relaxed) {
        return;
    }
    let avail = ctx.prod.slots();
    let to_write = frames.min(avail);
    if to_write == 0 {
        ctx.dropped
            .fetch_add(frames as u64, Ordering::Relaxed);
        return;
    }
    if let Ok(mut chunk) = ctx.prod.write_chunk_uninit(to_write) {
        let (first, second) = chunk.as_mut_slices();
        let mut out = first.iter_mut().chain(second.iter_mut());
        for frame in data.chunks_exact(channels).take(to_write) {
            let re = frame[0];
            let im = frame[1];
            out.next().unwrap().write(Complex32::new(re, im));
        }
        unsafe { chunk.commit_all() };
    }
    let dropped = frames - to_write;
    if dropped > 0 {
        ctx.dropped.fetch_add(dropped as u64, Ordering::Relaxed);
    }
}

fn push_interleaved_i32(data: &[i32], channels: usize, ctx: &mut AudioCtx) {
    if channels < 2 {
        return;
    }
    const SCALE: f32 = 1.0 / 8_388_608.0; // 24-bit in i32
    let frames = data.len() / channels;
    if ctx.transmitting.load(Ordering::Relaxed) {
        return;
    }
    let avail = ctx.prod.slots();
    let to_write = frames.min(avail);
    if to_write == 0 {
        ctx.dropped
            .fetch_add(frames as u64, Ordering::Relaxed);
        return;
    }
    if let Ok(mut chunk) = ctx.prod.write_chunk_uninit(to_write) {
        let (first, second) = chunk.as_mut_slices();
        let mut out = first.iter_mut().chain(second.iter_mut());
        for frame in data.chunks_exact(channels).take(to_write) {
            let re = frame[0] as f32 * SCALE;
            let im = frame[1] as f32 * SCALE;
            out.next().unwrap().write(Complex32::new(re, im));
        }
        unsafe { chunk.commit_all() };
    }
    let dropped = frames - to_write;
    if dropped > 0 {
        ctx.dropped.fetch_add(dropped as u64, Ordering::Relaxed);
    }
}

fn push_interleaved_i16(data: &[i16], channels: usize, ctx: &mut AudioCtx) {
    if channels < 2 {
        return;
    }
    const SCALE: f32 = 1.0 / 32768.0;
    let frames = data.len() / channels;
    if ctx.transmitting.load(Ordering::Relaxed) {
        return;
    }
    let avail = ctx.prod.slots();
    let to_write = frames.min(avail);
    if to_write == 0 {
        ctx.dropped
            .fetch_add(frames as u64, Ordering::Relaxed);
        return;
    }
    if let Ok(mut chunk) = ctx.prod.write_chunk_uninit(to_write) {
        let (first, second) = chunk.as_mut_slices();
        let mut out = first.iter_mut().chain(second.iter_mut());
        for frame in data.chunks_exact(channels).take(to_write) {
            let re = frame[0] as f32 * SCALE;
            let im = frame[1] as f32 * SCALE;
            out.next().unwrap().write(Complex32::new(re, im));
        }
        unsafe { chunk.commit_all() };
    }
    let dropped = frames - to_write;
    if dropped > 0 {
        ctx.dropped.fetch_add(dropped as u64, Ordering::Relaxed);
    }
}

/// Input devices suitable for QMX IQ capture.
pub fn list_input_devices() -> Vec<String> {
    let host = cpal::default_host();
    let Ok(devices) = host.input_devices() else {
        return Vec::new();
    };
    devices.filter_map(|d| d.name().ok()).collect()
}

fn resolve_serial_port(requested: &str) -> Result<String> {
    let trimmed = requested.trim();
    if !trimmed.is_empty() {
        return Ok(trimmed.to_string());
    }
    let ports = list_serial_ports();
    ports
        .first()
        .cloned()
        .ok_or(SourceError::NotFound)
}

fn resolve_audio_device(requested: &str) -> Result<String> {
    let trimmed = requested.trim();
    if !trimmed.is_empty() {
        return Ok(trimmed.to_string());
    }
    let devices = list_input_devices();
    if let Some(name) = devices.iter().find(|n| device_looks_like_qmx(n)) {
        return Ok(name.clone());
    }
    devices.into_iter().next().ok_or(SourceError::NotFound)
}

fn device_looks_like_qmx(name: &str) -> bool {
    let lower = name.to_ascii_lowercase();
    lower.contains("qmx") || lower.contains("qrp")
}

fn find_input_device(
    host: &cpal::Host,
    name: &str,
) -> Result<cpal::Device> {
    let devices: Vec<_> = host
        .input_devices()
        .map_err(|e| SourceError::Unsupported(e.to_string()))?
        .collect();
    devices
        .into_iter()
        .find(|d| d.name().ok().as_deref() == Some(name))
        .ok_or(SourceError::NotFound)
}

fn pick_input_config(device: &cpal::Device) -> Result<cpal::SupportedStreamConfig> {
    use cpal::SampleRate;
    let configs: Vec<_> = device
        .supported_input_configs()
        .map_err(|e| SourceError::Unsupported(e.to_string()))?
        .collect();

    for fmt in [
        SampleFormat::I32,
        SampleFormat::F32,
        SampleFormat::I16,
    ] {
        if let Some(c) = configs.iter().find(|c| {
            c.sample_format() == fmt
                && c.min_sample_rate().0 <= SAMPLE_RATE
                && c.max_sample_rate().0 >= SAMPLE_RATE
        }) {
            return Ok(c.with_sample_rate(SampleRate(SAMPLE_RATE)));
        }
    }

    device
        .default_input_config()
        .map_err(|e| SourceError::Unsupported(e.to_string()))
}
