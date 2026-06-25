//! Connect, teardown, audio device.

use std::sync::atomic::Ordering;
use std::time::Instant;

use crate::audio::AudioOutput;
use hfsdr::IqAudioDemod;
use crate::log;
use crate::source::{connect, ConnectRequest};

use super::Engine;
use crate::engine::types::ConnState;


impl Engine {
pub(super) fn start_connect(&mut self, req: &ConnectRequest) {
        self.connect_cancel.store(false, Ordering::Relaxed);
        self.teardown();
        if self.connect_cancel.load(Ordering::Relaxed) || self.request.is_none() {
            self.set_state(ConnState::Disconnected);
            return;
        }
        self.set_state(ConnState::Connecting { label: req.label() });
        self.drain_commands();
        if self.connect_cancel.load(Ordering::Relaxed) || self.request.is_none() {
            self.set_state(ConnState::Disconnected);
            return;
        }
        match connect(req, &self.connect_cancel) {
            Ok(conn) => {
                if self.connect_cancel.load(Ordering::Relaxed) || self.request.is_none() {
                    let mut conn = conn;
                    let _ = conn.device.stop();
                    self.set_state(ConnState::Disconnected);
                    return;
                }
                self.demod = IqAudioDemod::new();
                self.audio_device_open(conn.sample_rate as u32);
                self.last_data = Instant::now();
                self.connected_at = Instant::now();
                self.first_iq_received = !conn.is_kiwi;
                self.rate_window_start = Instant::now();
                self.rate_window_count = 0;
                self.reconnect_attempt = 0;
                self.retry_at = None;
                self.slow_since = None;
                self.conn = Some(conn);
                self.set_error(None);
                if self.conn.as_ref().is_some_and(|c| c.is_kiwi) {
                    self.set_state(ConnState::Connecting {
                        label: req.label(),
                    });
                } else {
                    self.set_state(ConnState::Streaming);
                }
                self.publish_stats(0);
            }
            Err(error) => {
                if self.connect_cancel.load(Ordering::Relaxed)
                    || self.request.is_none()
                    || error.contains("cancelled")
                {
                    self.set_state(ConnState::Disconnected);
                    return;
                }
                self.set_error(Some(error));
                self.schedule_reconnect();
                self.set_state(ConnState::Reconnecting {
                    attempt: self.reconnect_attempt,
                    retry_in_s: self.retry_secs(),
                });
            }
        }
    }

    pub(super) fn teardown(&mut self) {
        if let Some(conn) = &mut self.conn {
            let _ = conn.device.stop();
        }
        self.conn = None;
        self.playback = None;
        self.iq_buffer_fill = 0.0;
        self.iq_buffer_secs = 0.0;
        self.iq_buffer_peak = 0.0;
        self.last_pump_got = 0;
        self.last_pump_at = Instant::now();
        self.reset_skimmer_peak_hold(self.fft_size);
        self.last_skimmer_center_hz = f64::NAN;
        self.stop_recorder();
    }

    pub(super) fn reset_skimmer_peak_hold(&mut self, len: usize) {
        let len = len.max(1);
        if self.skimmer_peak_hold.len() != len {
            self.skimmer_peak_hold.resize(len, -120.0);
        } else {
            self.skimmer_peak_hold.fill(-120.0);
        }
    }

    pub(super) fn touch_skimmer_center(&mut self, center_hz: f64) {
        if self.last_skimmer_center_hz.is_nan()
            || (center_hz - self.last_skimmer_center_hz).abs() > 1.0
        {
            self.reset_skimmer_peak_hold(self.fft_size);
            self.last_skimmer_center_hz = center_hz;
        }
    }

    pub(super) fn stop_recorder(&mut self) {
        if let Some(rec) = self.recorder.take() {
            match rec.stop() {
                Ok(meta) => {
                    self.recorder_samples = meta.sample_count;
                    log::info(format!(
                        "IQ capture saved: {} samples ({:.1}s)",
                        meta.sample_count,
                        meta.duration_secs()
                    ));
                }
                Err(e) => log::error(format!("IQ capture finalize failed: {e}")),
            }
        }
    }

    pub(super) fn audio_device_open(&mut self, _iq_rate: u32) {
        self.audio = match &self.audio_device {
            Some(name) => AudioOutput::try_open_named(name, 0)
                .or_else(|| AudioOutput::try_open_default(0)),
            None => AudioOutput::try_open_default(0),
        };
        if self.audio.is_none() {
            log::error("audio output unavailable (need PulseAudio/PipeWire/ALSA and F32 output)");
        }
    }

    pub(super) fn reopen_audio(&mut self) {
        self.audio_device_open(0);
    }
}
