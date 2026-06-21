//! Speaker output via cpal — plays demodulated baseband audio from the IQ stream.

use cpal::traits::{DeviceTrait, HostTrait, StreamTrait};
use cpal::{Device, SampleRate, Stream, SupportedStreamConfig};
use rtrb::{Consumer, Producer, RingBuffer};

use crate::log;

const RING_CAPACITY: usize = 48_000;

pub struct AudioOutput {
    producer: Producer<f32>,
    output_rate: u32,
    device_name: String,
    _stream: Stream,
}

impl AudioOutput {
    pub fn list_output_devices() -> Vec<String> {
        let host = cpal::default_host();
        let Ok(devices) = host.output_devices() else {
            return Vec::new();
        };
        devices.filter_map(|d| d.name().ok()).collect()
    }

    /// Open the default output device.
    pub fn try_open_default(source_rate: u32) -> Option<Self> {
        let host = cpal::default_host();
        let device = host.default_output_device()?;
        Self::try_open_device(&device, source_rate)
    }

    /// Open a named output device.
    pub fn try_open_named(name: &str, source_rate: u32) -> Option<Self> {
        let host = cpal::default_host();
        let Ok(mut devices) = host.output_devices() else {
            return None;
        };
        let device = devices.find(|d| d.name().ok().as_deref() == Some(name))?;
        Self::try_open_device(&device, source_rate)
    }

    fn try_open_device(device: &Device, source_rate: u32) -> Option<Self> {
        let device_name = device.name().unwrap_or_else(|_| "unknown".to_string());
        let config = pick_output_config(device, source_rate)?;
        let output_rate = config.sample_rate().0;
        let channels = config.channels() as usize;
        log::info(format!("audio: {device_name} @ {output_rate} Hz, {channels} ch"));

        let (producer, consumer) = RingBuffer::<f32>::new(RING_CAPACITY);
        let mut cons = consumer;
        let err_fn = |e| log::error(format!("audio stream error: {e}"));

        let stream = device
            .build_output_stream(
                &config.config(),
                move |data: &mut [f32], _| fill_output(data, channels, &mut cons),
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
            _stream: stream,
        })
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

fn fill_output(data: &mut [f32], channels: usize, consumer: &mut Consumer<f32>) {
    if channels == 1 {
        for sample in data.iter_mut() {
            *sample = consumer.pop().unwrap_or(0.0);
        }
    } else {
        for frame in data.chunks_mut(channels) {
            let s = consumer.pop().unwrap_or(0.0);
            for ch in frame.iter_mut() {
                *ch = s;
            }
        }
    }
}

fn pick_output_config(device: &Device, source_rate: u32) -> Option<SupportedStreamConfig> {
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
        .find(|c| c.min_sample_rate().0 <= source_rate && c.max_sample_rate().0 >= source_rate)
    {
        return Some(c.with_sample_rate(SampleRate(source_rate)));
    }

    if let Some(c) = configs
        .iter()
        .find(|c| c.min_sample_rate().0 <= 48_000 && c.max_sample_rate().0 >= 48_000)
    {
        return Some(c.with_sample_rate(SampleRate(48_000)));
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
