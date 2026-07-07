//! Speaker output via cpal — plays demodulated baseband audio from the IQ stream.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleRate, Stream, SupportedStreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::log;

#[cfg(test)]
use std::sync::Mutex;

#[cfg(test)]
static TEST_OUTPUT_DEVICES: Mutex<Option<Vec<String>>> = Mutex::new(None);

/// Headless UI tests: skip cpal/ALSA enumeration (faster, quieter CI).
#[cfg(test)]
pub fn set_test_output_devices(devices: Option<Vec<String>>) {
    if let Ok(mut guard) = TEST_OUTPUT_DEVICES.lock() {
        *guard = devices;
    }
}

/// Standard device rate — demod output is resampled in [`AudioOutput::push`].
pub const OUTPUT_SAMPLE_RATE: u32 = 48_000;

/// ~200 ms at 48 kHz — enough jitter headroom without desyncing from the waterfall.
const RING_CAPACITY: usize = 9_600;

pub struct AudioOutput {
    producer: Producer<f32>,
    output_rate: u32,
    device_name: String,
    skip_samples: Arc<AtomicUsize>,
    /// Fractional source read position carried across [`Self::push`] calls
    /// (sits in [-1, 0) between blocks, pointing back at `resample_last`).
    resample_pos: f64,
    /// Last source sample of the previous block — interpolation anchor.
    resample_last: f32,
    /// Smoothed ring occupancy for the clock-drift servo.
    fill_avg: f32,
    _stream: Stream,
}

impl AudioOutput {
    pub fn list_output_devices() -> Vec<String> {
        #[cfg(test)]
        if let Ok(guard) = TEST_OUTPUT_DEVICES.lock() {
            if let Some(devices) = guard.as_ref() {
                return devices.clone();
            }
        }
        let host = cpal::default_host();
        let Ok(devices) = host.output_devices() else {
            return Vec::new();
        };
        devices.filter_map(|d| d.name().ok()).collect()
    }

    /// Open the default output device at [`OUTPUT_SAMPLE_RATE`].
    pub fn try_open_default(_iq_rate: u32) -> Option<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        Self::try_open_device(&device)
    }

    /// Open a named output device at [`OUTPUT_SAMPLE_RATE`].
    pub fn try_open_named(name: &str, _iq_rate: u32) -> Option<Self> {
        let host = cpal::default_host();
        let Ok(mut devices) = host.output_devices() else {
            return None;
        };
        let device = devices.find(|d| d.name().ok().as_deref() == Some(name))?;
        Self::try_open_device(&device)
    }

    fn try_open_device(device: &Device) -> Option<Self> {
        let device_name = device.name().unwrap_or_else(|_| "unknown".to_string());
        let config = pick_output_config(device)?;
        let output_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        log::info(format!("audio: {device_name} @ {output_rate} Hz, {channels} ch"));

        let (producer, consumer) = RingBuffer::<f32>::new(RING_CAPACITY);
        let skip_samples = Arc::new(AtomicUsize::new(0));
        let skip_for_callback = Arc::clone(&skip_samples);
        let mut cons = consumer;
        let err_fn = |e| log::error(format!("audio stream error: {e}"));

        let stream = device
            .build_output_stream(
                &config.config(),
                move |data: &mut [f32], _| {
                    fill_output(data, channels, &mut cons, &skip_for_callback)
                },
                err_fn,
                None,
            )
            .map_err(|e| {
                log::error(format!("audio stream build: {e}"));
                e
            })
            .ok()?;

        stream.play()
            .map_err(|e| {
                log::error(format!("audio stream play: {e}"));
                e
            })
            .ok()?;

        Some(Self {
            producer,
            output_rate,
            device_name,
            skip_samples,
            resample_pos: 0.0,
            resample_last: 0.0,
            fill_avg: 0.5,
            _stream: stream,
        })
    }

    /// Drop queued output so speaker audio stays aligned after IQ ring catch-up.
    pub fn skip_seconds(&self, secs: f32) {
        if secs <= 0.0 {
            return;
        }
        let n = (secs * self.output_rate as f32).round() as usize;
        if n > 0 {
            self.skip_samples.fetch_add(n, Ordering::Relaxed);
        }
    }

    pub fn output_rate(&self) -> u32 {
        self.output_rate
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    /// Push mono samples at `source_rate`; resamples linearly when rates differ.
    ///
    /// The interpolation phase carries across calls, so block boundaries stay
    /// continuous, and a slow servo (±0.3 %) on the resample ratio keeps the ring
    /// near half full so source/sink clock mismatch never accumulates into an
    /// overflow drop or an underrun gap.
    pub fn push(&mut self, mono: &[f32], source_rate: u32, volume: f32) {
        if mono.is_empty() || volume <= 0.0 {
            return;
        }
        if source_rate == self.output_rate {
            for &s in mono {
                if self.producer.is_full() {
                    break;
                }
                let _ = self.producer.push(s * volume);
            }
            return;
        }

        let fill = 1.0 - self.producer.slots() as f32 / RING_CAPACITY as f32;
        self.fill_avg += 0.02 * (fill - self.fill_avg);
        let trim = ((self.fill_avg - 0.5) * 0.01).clamp(-0.003, 0.003);
        let step = source_rate as f64 / self.output_rate as f64 * (1.0 + trim as f64);

        let (pos, last) = resample_push(
            &mut self.producer,
            mono,
            step,
            self.resample_pos,
            self.resample_last,
            volume,
        );
        self.resample_pos = pos;
        self.resample_last = last;
    }
}

/// Linear-interpolation resampler with phase carried across blocks.
///
/// `pos` is the fractional read position into `mono` (may sit in [-1, 0)
/// pointing at `last`, the final sample of the previous block). Returns the
/// carried `(pos, last)` for the next call.
fn resample_push(
    producer: &mut Producer<f32>,
    mono: &[f32],
    step: f64,
    mut pos: f64,
    last: f32,
    volume: f32,
) -> (f64, f32) {
    let n = mono.len();
    let limit = n as f64 - 1.0;
    while pos < limit {
        if producer.is_full() {
            break;
        }
        let i = pos.floor();
        let frac = (pos - i) as f32;
        let (a, b) = if i < 0.0 {
            (last, mono[0])
        } else {
            let idx = i as usize;
            (mono[idx], mono[idx + 1])
        };
        let _ = producer.push((a + (b - a) * frac) * volume);
        pos += step;
    }
    // Lands in [-1, 0) when the block was fully consumed; clamp after an
    // overflow break (the dropped tail is a discontinuity either way).
    ((pos - n as f64).max(-1.0), mono[n - 1])
}

fn fill_output(
    data: &mut [f32],
    channels: usize,
    consumer: &mut Consumer<f32>,
    skip: &AtomicUsize,
) {
    if channels == 1 {
        for sample in data.iter_mut() {
            *sample = next_output_sample(consumer, skip);
        }
    } else {
        for frame in data.chunks_mut(channels) {
            let s = next_output_sample(consumer, skip);
            for ch in frame.iter_mut() {
                *ch = s;
            }
        }
    }
}

fn next_output_sample(consumer: &mut Consumer<f32>, skip: &AtomicUsize) -> f32 {
    // Drain the whole skip budget at once: dropping stale samples in one go
    // resumes fresh audio immediately instead of muting for the skip duration.
    let mut remaining = skip.swap(0, Ordering::Relaxed);
    while remaining > 0 && consumer.pop().is_ok() {
        remaining -= 1;
    }
    if remaining > 0 {
        skip.fetch_add(remaining, Ordering::Relaxed);
        return 0.0;
    }
    consumer.pop().unwrap_or(0.0)
}

fn pick_output_config(device: &Device) -> Option<SupportedStreamConfig> {
    let configs: Vec<_> = device
        .supported_output_configs()
        .map_err(|e| {
            log::error(format!("audio configs: {e}"));
            e
        })
        .ok()?
        .filter(|c| c.sample_format() == cpal::SampleFormat::F32)
        .collect();

    if let Some(c) = configs
        .iter()
        .find(|c| {
            c.min_sample_rate().0 <= OUTPUT_SAMPLE_RATE && c.max_sample_rate().0 >= OUTPUT_SAMPLE_RATE
        })
    {
        return Some(c.with_sample_rate(SampleRate(OUTPUT_SAMPLE_RATE)));
    }

    if let Some(c) = configs
        .iter()
        .find(|c| c.min_sample_rate().0 <= 44_100 && c.max_sample_rate().0 >= 44_100)
    {
        return Some(c.with_sample_rate(SampleRate(44_100)));
    }

    device
        .default_output_config()
        .map_err(|e| {
            log::error(format!("audio default config: {e}"));
            e
        })
        .ok()
        .filter(|c| c.sample_format() == cpal::SampleFormat::F32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use rtrb::RingBuffer;

    #[test]
    fn fill_output_mono_drains_ring() {
        let (mut prod, mut cons) = RingBuffer::<f32>::new(4);
        let skip = AtomicUsize::new(0);
        prod.push(0.25).unwrap();
        prod.push(0.75).unwrap();
        let mut data = [0.0_f32; 3];
        fill_output(&mut data, 1, &mut cons, &skip);
        assert!((data[0] - 0.25).abs() < 1e-6);
        assert!((data[1] - 0.75).abs() < 1e-6);
        assert_eq!(data[2], 0.0);
    }

    #[test]
    fn fill_output_stereo_duplicates_mono() {
        let (mut prod, mut cons) = RingBuffer::<f32>::new(2);
        let skip = AtomicUsize::new(0);
        prod.push(0.5).unwrap();
        let mut data = [0.0_f32; 4];
        fill_output(&mut data, 2, &mut cons, &skip);
        assert!((data[0] - 0.5).abs() < 1e-6);
        assert!((data[1] - 0.5).abs() < 1e-6);
        assert_eq!(data[2], 0.0);
        assert_eq!(data[3], 0.0);
    }

    #[test]
    fn skip_drains_stale_samples_and_resumes_immediately() {
        let (mut prod, mut cons) = RingBuffer::<f32>::new(4);
        let skip = AtomicUsize::new(1);
        prod.push(0.25).unwrap();
        prod.push(0.75).unwrap();
        let mut data = [1.0_f32; 1];
        fill_output(&mut data, 1, &mut cons, &skip);
        // The stale 0.25 is dropped in one go; fresh audio plays right away.
        assert!((data[0] - 0.75).abs() < 1e-6);
        assert_eq!(skip.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn skip_larger_than_queue_keeps_remainder() {
        let (mut prod, mut cons) = RingBuffer::<f32>::new(4);
        let skip = AtomicUsize::new(3);
        prod.push(0.25).unwrap();
        let mut data = [1.0_f32; 1];
        fill_output(&mut data, 1, &mut cons, &skip);
        assert_eq!(data[0], 0.0);
        assert_eq!(skip.load(Ordering::Relaxed), 2);
    }

    #[test]
    fn resampler_is_continuous_across_block_boundaries() {
        // 12 kHz -> 48 kHz sine split into uneven blocks must match a single
        // one-shot pass exactly (phase carried, no per-block reset).
        let n = 480;
        let src: Vec<f32> = (0..n)
            .map(|i| (std::f32::consts::TAU * 600.0 * i as f32 / 12_000.0).sin())
            .collect();
        let step = 12_000.0f64 / 48_000.0;

        let (mut prod_a, mut cons_a) = RingBuffer::<f32>::new(8192);
        let mut pos = 0.0;
        let mut last = 0.0;
        for chunk in [&src[..37], &src[37..300], &src[300..]] {
            let (p, l) = resample_push(&mut prod_a, chunk, step, pos, last, 1.0);
            pos = p;
            last = l;
        }

        let (mut prod_b, mut cons_b) = RingBuffer::<f32>::new(8192);
        let _ = resample_push(&mut prod_b, &src, step, 0.0, 0.0, 1.0);

        let mut max_err = 0.0f32;
        loop {
            match (cons_a.pop(), cons_b.pop()) {
                (Ok(a), Ok(b)) => max_err = max_err.max((a - b).abs()),
                (Err(_), Err(_)) => break,
                _ => panic!("chunked and one-shot resample lengths differ"),
            }
        }
        assert!(max_err < 1e-6, "block-boundary discontinuity: {max_err}");
    }

    #[test]
    fn list_output_devices_includes_test_injection() {
        set_test_output_devices(Some(vec!["Test Output".into()]));
        let names = AudioOutput::list_output_devices();
        assert!(names.iter().any(|n| n == "Test Output"));
    }
}
