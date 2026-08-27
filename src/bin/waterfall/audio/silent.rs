//! No-device fallback: a build with neither cpal nor a browser to play into.
//!
//! Only the headless test configuration reaches this. It keeps the same public
//! surface so the engine needs no cfg of its own.

use std::sync::Mutex;

static TEST_OUTPUT_DEVICES: Mutex<Option<Vec<String>>> = Mutex::new(None);

pub fn set_test_output_devices(devices: Option<Vec<String>>) {
    if let Ok(mut g) = TEST_OUTPUT_DEVICES.lock() {
        *g = devices;
    }
}

pub const OUTPUT_SAMPLE_RATE: u32 = 48_000;

pub struct AudioOutput {
    device_name: String,
}

impl AudioOutput {
    pub fn list_output_devices() -> Vec<String> {
        TEST_OUTPUT_DEVICES
            .lock()
            .ok()
            .and_then(|g| g.clone())
            .unwrap_or_default()
    }

    pub fn try_open_default(_iq_rate: u32) -> Option<Self> {
        None
    }

    pub fn try_open_named(_name: &str, _iq_rate: u32) -> Option<Self> {
        None
    }

    pub fn skip_seconds(&self, _secs: f32) {}

    pub fn output_rate(&self) -> u32 {
        OUTPUT_SAMPLE_RATE
    }

    pub fn device_name(&self) -> &str {
        &self.device_name
    }

    pub fn push(&mut self, _mono: &[f32], _source_rate: u32, _volume: f32) {}
}
