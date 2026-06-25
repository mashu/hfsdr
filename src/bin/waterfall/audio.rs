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
        let ratio = self.output_rate as f64 / source_rate as f64;
        let out_len = (mono.len() as f64 * ratio).ceil() as usize;
        for o in 0..out_len {
            if self.producer.is_full() {
                break;
            }
            let src_idx = o as f64 / ratio;
            let i = src_idx.floor() as usize;
            let frac = (src_idx - i as f64) as f32;
            let a = mono.get(i).copied().unwrap_or(0.0);
            let b = mono.get(i + 1).copied().unwrap_or(a);
            let _ = self.producer.push((a + (b - a) * frac) * volume);
        }
    }
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
    if skip.load(Ordering::Relaxed) > 0 {
        let _ = consumer.pop();
        skip.fetch_sub(1, Ordering::Relaxed);
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
    fn skip_outputs_silence_and_drains_ring() {
        let (mut prod, mut cons) = RingBuffer::<f32>::new(4);
        let skip = AtomicUsize::new(1);
        prod.push(0.25).unwrap();
        prod.push(0.75).unwrap();
        let mut data = [1.0_f32; 1];
        fill_output(&mut data, 1, &mut cons, &skip);
        assert_eq!(data[0], 0.0);
        assert_eq!(skip.load(Ordering::Relaxed), 0);
        let mut tail = [0.0_f32; 1];
        fill_output(&mut tail, 1, &mut cons, &skip);
        assert!((tail[0] - 0.75).abs() < 1e-6);
    }

    #[test]
    fn list_output_devices_includes_test_injection() {
        set_test_output_devices(Some(vec!["Test Output".into()]));
        let names = AudioOutput::list_output_devices();
        assert!(names.iter().any(|n| n == "Test Output"));
    }
}
