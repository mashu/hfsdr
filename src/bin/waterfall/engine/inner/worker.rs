//! Main engine loop.

use std::sync::Arc;
use std::sync::atomic::AtomicBool;
use std::sync::mpsc::{Receiver, TryRecvError};
#[cfg(not(target_arch = "wasm32"))]
use std::sync::mpsc::RecvTimeoutError;
use std::sync::Mutex;
#[cfg(not(target_arch = "wasm32"))]
use std::thread;
use hfsdr::time::Instant;
#[cfg(not(target_arch = "wasm32"))]
use hfsdr::time::Duration;

use hfsdr::{DecimFilterKind, FirDecimator, IqAudioDemod, PipelineMetrics, SpectrumAnalyzer, SpectrumFrontEnd, DEFAULT_FFT_WINDOW, DEFAULT_KAISER_BETA};


#[cfg(not(target_arch = "wasm32"))]
use hfsdr::IngressWorker;

use crate::engine::audio::{AudioScopeRing, AudioWaveformRing};
use super::Engine;
use crate::engine::policy::{catchup_pumps_max, MAX_DRAIN_WIDEBAND};
use crate::engine::link::EngineLink;
use crate::engine::types::{EngineCommand, EngineParams};
use crate::engine::policy::MIN_SPECTRUM_ROWS_WIDEBAND;
use crate::engine::{FFT_HOP, FFT_SIZE};

/// Why an idle engine iteration returned, when no command was waiting.
enum IdleWait {
    Empty,
    Disconnected,
}

/// How the engine may spend an iteration with no work to do.
///
/// The two drivers need opposite behaviour here. A native engine owns its
/// thread and should park rather than spin a core at 100%. A browser engine
/// runs on the frame callback, where any blocking wait freezes the UI for
/// exactly as long as it waits — there the repaint interval *is* the pacing.
#[derive(Clone, Copy)]
pub(crate) enum IdlePacing {
    /// Block briefly; the caller has its own thread.
    Park,
    /// Return at once; the caller is driven by something else.
    Return,
}

impl IdlePacing {
    /// Wait out a pump that produced nothing, so an idle engine does not spin.
    fn pause_briefly(self) {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Park => thread::sleep(Duration::from_millis(3)),
            // Unreachable on wasm32 (nothing constructs `Park` there), but the
            // variant still exists, and sleeping in a tab would be wrong anyway.
            #[cfg(target_arch = "wasm32")]
            Self::Park => {}
            Self::Return => {}
        }
    }

    /// Take the next command, blocking only when this driver is allowed to.
    fn await_command(
        self,
        rx: &Receiver<EngineCommand>,
    ) -> std::result::Result<EngineCommand, IdleWait> {
        match self {
            #[cfg(not(target_arch = "wasm32"))]
            Self::Park => rx.recv_timeout(Duration::from_millis(20)).map_err(|e| match e {
                RecvTimeoutError::Timeout => IdleWait::Empty,
                RecvTimeoutError::Disconnected => IdleWait::Disconnected,
            }),
            #[cfg(target_arch = "wasm32")]
            Self::Park => Self::Return.await_command(rx),
            Self::Return => rx.try_recv().map_err(|e| match e {
                TryRecvError::Empty => IdleWait::Empty,
                TryRecvError::Disconnected => IdleWait::Disconnected,
            }),
        }
    }
}

impl Engine {
    pub(crate) fn new(link: EngineLink) -> Self {
        let EngineLink {
            cmd_rx,
            snapshot,
            rows_tx,
            spent_rows,
            params,
            connect_cancel,
        } = link;
        Self {
            cmd_rx,
            snapshot,
            rows_tx,
            spent_rows,
            params,
            rows_dropped: 0,
            state: crate::engine::types::ConnState::Disconnected,
            last_error: None,
            last_slow: false,
            last_snr: 0.0,
            conn: None,
            request: None,
            audio: None,
            audio_device: None,
            demod: IqAudioDemod::new(),
            analyzer: SpectrumAnalyzer::new(FFT_SIZE, FFT_HOP),
            spectrum_front: SpectrumFrontEnd::new(12_000.0, 1, 0.0),
            spectrum_scratch: Vec::new(),
            drain: Vec::with_capacity(MAX_DRAIN_WIDEBAND),
            drain_decim: Vec::with_capacity(MAX_DRAIN_WIDEBAND),
            spectrum_ingress: FirDecimator::with_factor(384_000.0, 1, true, DecimFilterKind::LinearFir),
            spectrum_ingress_factor: 1,
            spectrum_ingress_rate: 384_000.0,
            spectrum_ingress_filter: DecimFilterKind::LinearFir,
            // wasm32 has no threads; the pump decimates inline when this is None.
            #[cfg(not(target_arch = "wasm32"))]
            ingress_worker: Some(IngressWorker::spawn()),
            #[cfg(target_arch = "wasm32")]
            ingress_worker: None,
            audio_scratch: Vec::new(),
            audio_scope: AudioScopeRing::new(),
            audio_waveform: AudioWaveformRing::new(),
            latest: vec![-120.0; FFT_SIZE],
            skimmer_peak_hold: vec![-120.0; FFT_SIZE],
            last_skimmer_center_hz: f64::NAN,
            fft_size: FFT_SIZE,
            spectrum_window: DEFAULT_FFT_WINDOW,
            spectrum_kaiser_beta: DEFAULT_KAISER_BETA,
            spectrum_rate: 12_000.0,
            spectrum_decim: 1,
            spectrum_pan_hz: 0.0,
            spectrum_hop: FFT_SIZE / 2,
            pump_serial: 0,
            last_data: Instant::now(),
            connected_at: Instant::now(),
            first_iq_received: false,
            reconnect_attempt: 0,
            retry_at: None,
            rate_window_start: Instant::now(),
            rate_window_count: 0,
            cached_rate: 0.0,
            slow_since: None,
            running: true,
            recorder: None,
            recorder_samples: 0,
            playback: None,
            iq_buffer_fill: 0.0,
            iq_buffer_secs: 0.0,
            iq_buffer_peak: 0.0,
            last_pump_got: 0,
            last_pump_at: Instant::now(),
            last_iq_dropped: 0,
            last_spectrum_rows: MIN_SPECTRUM_ROWS_WIDEBAND,
            row_pool: Vec::new(),
            level_audio_peak: 0.0,
            level_audio_rms: 0.0,
            level_agc_gain: 1.0,
            level_agc_envelope: 0.0,
            level_iq_rf: 0.0,
            level_estimated_wpm: 20.0,
            level_keying_confident: false,
            level_audio_scope: Vec::new(),
            level_audio_waveform: Vec::new(),
            pipeline_avg: PipelineMetrics::default(),
            last_perf_log: Instant::now(),
            last_pipeline: PipelineMetrics::default(),
            connect_cancel,
        }
    }

    pub(crate) fn run(&mut self) {
        while self.running {
            self.step(IdlePacing::Park);
        }
        // Clean shutdown: stop source so the reader thread exits.
        if let Some(conn) = &mut self.conn {
            crate::log::warn_if_err("stop device on engine shutdown", conn.device.stop());
        }
    }

    /// Run one iteration of the engine loop.
    ///
    /// Split out of [`Self::run`] because a browser tab has no thread to run
    /// that loop on: there the frame callback calls this instead, once per
    /// repaint. Everything the engine does per iteration is here; `run` is the
    /// native driver around it.
    pub(crate) fn step(&mut self, idle: IdlePacing) {
        {
            self.drain_commands();
            if !self.running {
                return;
            }

            let streaming = self.conn.is_some() || self.playback.is_some();
            if streaming {
                self.poll_handshake();
                let (ring_fill, _) = self.measure_iq_buffer();
                let iq_recording = self.recorder.is_some();
                let full_drain = self.params.slot().full_drain_spectrum;
                let max_pumps = catchup_pumps_max(ring_fill, iq_recording, full_drain);
                let mut pumps = 0usize;
                loop {
                    let got = self.pump_stream();
                    pumps += 1;
                    self.drain_commands();
                    if !self.running || got == 0 || pumps >= max_pumps {
                        break;
                    }
                    let (fill, _) = self.measure_iq_buffer();
                    if fill < 0.2 {
                        break;
                    }
                }
                self.maybe_reconnect_on_stall();
                if self.last_pump_got == 0 {
                    idle.pause_briefly();
                }
            } else {
                self.maybe_retry_reconnect();
                let (sample_rate, _, _) = self.link_meta();
                let dt = self
                    .last_pump_at
                    .elapsed()
                    .as_secs_f32()
                    .clamp(0.001, 0.1);
                self.update_ring_utilization(sample_rate, (0.0, 0.0), 0, dt);
                self.last_pump_at = Instant::now();
                self.publish_stats(0);
                match idle.await_command(&self.cmd_rx) {
                    Ok(cmd) => self.handle_command(cmd),
                    Err(IdleWait::Empty) => {}
                    Err(IdleWait::Disconnected) => self.running = false,
                }
            }
        }
    }

    pub(super) fn drain_commands(&mut self) {
        loop {
            match self.cmd_rx.try_recv() {
                Ok(cmd) => self.handle_command(cmd),
                Err(TryRecvError::Empty) => break,
                Err(TryRecvError::Disconnected) => {
                    self.running = false;
                    break;
                }
            }
        }
    }
}
