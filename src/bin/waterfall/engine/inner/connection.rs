//! Connect, teardown, audio device.

use std::sync::atomic::Ordering;
use hfsdr::time::Instant;

use crate::audio::AudioOutput;
use hfsdr::IqAudioDemod;
use crate::log;
use crate::source::{connect, ConnectRequest};

use super::Engine;
use crate::engine::types::ConnState;


/// Whether an output that is already open still satisfies what is wanted.
///
/// Extracted from [`Engine::audio_device_open`] because the alternative is a
/// test that needs a real sound card: on a headless runner every attempt to
/// open one returns `None`, so a test written against the effect passes without
/// exercising anything. This is the decision that was wrong, so this is what
/// gets tested.
///
/// `wanted == None` means "whatever the system default is", which any open
/// output satisfies — reopening to discover the same device is what created a
/// discarded output per connection attempt.
pub(crate) fn keep_existing_output(open: Option<&str>, wanted: Option<&str>) -> bool {
    match (open, wanted) {
        (None, _) => false,
        (Some(_), None) => true,
        (Some(open), Some(wanted)) => open == wanted,
    }
}

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
                    log::warn_if_err("stop device after cancelled connect", conn.device.stop());
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
            log::warn_if_err("stop device", conn.device.stop());
        }
        self.conn = None;
        self.playback = None;
        self.iq_buffer_fill = 0.0;
        self.iq_buffer_secs = 0.0;
        self.iq_buffer_peak = 0.0;
        self.last_pump_got = 0;
        self.last_pump_at = Instant::now();
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

    /// Open an audio output for the selected device, keeping any usable one.
    ///
    /// This runs on every connection attempt, so it must not tear down a
    /// working output each time. A receiver that refuses the connection is
    /// retried on a backoff, and reopening per attempt meant a discarded output
    /// per retry — in a browser, a whole `AudioContext` each, of which a page
    /// gets only a handful before it can have no audio at all.
    pub(super) fn audio_device_open(&mut self, _iq_rate: u32) {
        let open = self.audio.as_ref().map(|a| a.device_name());
        if keep_existing_output(open, self.audio_device.as_deref()) {
            return;
        }
        self.open_audio_device();
    }

    /// Switch devices: the current output must go before the next one opens, so
    /// the old device is released rather than held by two streams at once.
    pub(super) fn reopen_audio(&mut self) {
        self.audio = None;
        self.open_audio_device();
    }

    fn open_audio_device(&mut self) {
        self.audio = match &self.audio_device {
            Some(name) => AudioOutput::try_open_named(name, 0)
                .or_else(|| AudioOutput::try_open_default(0)),
            None => AudioOutput::try_open_default(0),
        };
        if self.audio.is_none() {
            log::error("audio output unavailable (need PulseAudio/PipeWire/ALSA and F32 output)");
        }
    }
}
