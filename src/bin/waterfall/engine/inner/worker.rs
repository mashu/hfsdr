//! Main engine loop.

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, TryRecvError};
use std::sync::Mutex;
use std::thread;
use std::time::{Duration, Instant};

use hfsdr::{DecimFilterKind, FirDecimator, IngressWorker, IqAudioDemod, SpectrumAnalyzer, SpectrumFrontEnd};

use crate::skimmer::SkimmerHandle;

use crate::engine::audio::AudioScopeRing;
use super::Engine;
use crate::engine::policy::{catchup_pumps_max, MAX_DRAIN_WIDEBAND};
use crate::engine::types::{ConnState, EngineCommand, EngineParams, EngineShared};
use crate::engine::{FFT_HOP, FFT_SIZE, MIN_SPECTRUM_ROWS_WIDEBAND};

impl Engine {
    pub(crate) fn new(
        cmd_rx: Receiver<EngineCommand>,
        shared: Arc<Mutex<EngineShared>>,
        params: Arc<Mutex<EngineParams>>,
        connect_cancel: Arc<AtomicBool>,
    ) -> Self {
        Self {
            cmd_rx,
            shared,
            params,
            skimmer: SkimmerHandle::spawn("rx".into()),
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
            ingress_worker: Some(IngressWorker::spawn()),
            audio_scratch: Vec::new(),
            audio_scope: AudioScopeRing::new(),
            latest: vec![-120.0; FFT_SIZE],
            skimmer_peak_hold: vec![-120.0; FFT_SIZE],
            last_skimmer_center_hz: f64::NAN,
            fft_size: FFT_SIZE,
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
            last_spectrum_rows: MIN_SPECTRUM_ROWS_WIDEBAND,
            row_pool: Vec::new(),
            level_audio_peak: 0.0,
            level_audio_rms: 0.0,
            level_agc_gain: 1.0,
            level_agc_envelope: 0.0,
            level_iq_rf: 0.0,
            level_audio_scope: Vec::new(),
            connect_cancel,
        }
    }

    pub(crate) fn run(&mut self) {
        while self.running {
            self.drain_commands();
            if !self.running {
                break;
            }

            let streaming = self.conn.is_some() || self.playback.is_some();
            if streaming {
                self.poll_handshake();
                let (ring_fill, _) = self.measure_iq_buffer();
                let iq_recording = self.recorder.is_some();
                let full_drain = self
                    .params
                    .lock()
                    .map(|p| p.full_drain_spectrum)
                    .unwrap_or(false);
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
                    thread::sleep(Duration::from_millis(3));
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
                match self.cmd_rx.recv_timeout(Duration::from_millis(20)) {
                    Ok(cmd) => self.handle_command(cmd),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => self.running = false,
                }
            }
        }
        // Clean shutdown: stop source so the reader thread exits.
        if let Some(conn) = &mut self.conn {
            let _ = conn.source.stop();
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
