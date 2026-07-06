//! Discrete UI commands.

use std::sync::atomic::Ordering;

use std::time::Instant;

use hfsdr::{IqAudioDemod, IqPlayback, IqRecorder};

use crate::log;
use crate::source::controls_dispatch as src_ctl;

use super::Engine;
use crate::engine::types::{ConnState, EngineCommand};


impl Engine {
pub(super) fn handle_command(&mut self, cmd: EngineCommand) {
        match cmd {
            EngineCommand::Connect(req) => {
                self.request = Some(req.clone());
                self.reconnect_attempt = 0;
                self.retry_at = None;
                self.start_connect(&req);
            }
            EngineCommand::Disconnect => {
                self.connect_cancel.store(true, Ordering::Relaxed);
                self.teardown();
                self.request = None;
                self.retry_at = None;
                self.reconnect_attempt = 0;
                self.set_error(None);
                self.set_state(ConnState::Disconnected);
            }
            EngineCommand::Tune(hz) => {
                if let Some(conn) = &mut self.conn {
                    log::warn_if_err(format!("tune to {hz} Hz"), conn.device.tune(hz));
                    conn.center_hz = hz;
                }
                if let Some(req) = &mut self.request {
                    req.center_hz = hz;
                }
            }
            EngineCommand::SetRfAgc(on) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::kiwi_set_rf_agc(&mut conn.device, on);
                }
            }
            EngineCommand::SetKiwiManGain(gain) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::kiwi_set_man_gain(&mut conn.device, gain);
                }
            }
            EngineCommand::SetKiwiRfAttn(db) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::kiwi_set_rf_attn_db(&mut conn.device, db);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyAtt(step) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::airspy_set_hf_att(&mut conn.device, step);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyLna(on) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::airspy_set_hf_lna(&mut conn.device, on);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyAgcThreshold(high) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::airspy_set_hf_agc_threshold(&mut conn.device, high);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyFrontendOptions(flags) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::airspy_set_frontend_options(&mut conn.device, flags);
                }
            }
            #[cfg(feature = "airspy")]
            EngineCommand::SetAirspyBiasTee(on) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::airspy_set_bias_tee(&mut conn.device, on);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrRtlAgc(on) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::rtlsdr_set_rtl_agc(&mut conn.device, on);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrManualGain(manual) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::rtlsdr_set_manual_gain(&mut conn.device, manual);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrTunerGain(gain_db10) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::rtlsdr_set_tuner_gain(&mut conn.device, gain_db10);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrBiasTee(on) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::rtlsdr_set_bias_tee(&mut conn.device, on);
                }
            }
            #[cfg(feature = "rtlsdr")]
            EngineCommand::SetRtlSdrPpm(ppm) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::rtlsdr_set_ppm(&mut conn.device, ppm);
                }
            }
            #[cfg(feature = "qmx")]
            EngineCommand::SetQmxRfGain(db) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::qmx_set_rf_gain_db(&mut conn.device, db);
                }
            }
            #[cfg(feature = "soapy")]
            EngineCommand::SetSoapyGain(db) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::soapy_set_gain_db(&mut conn.device, db);
                }
            }
            #[cfg(feature = "soapy")]
            EngineCommand::SetSoapyAgc(on) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::soapy_set_agc(&mut conn.device, on);
                }
            }
            #[cfg(feature = "soapy")]
            EngineCommand::SetSoapyAntenna(name) => {
                if let Some(conn) = &mut self.conn {
                    src_ctl::soapy_set_antenna(&mut conn.device, &name);
                }
            }
            EngineCommand::SetAudioDevice(name) => {
                self.audio_device = name;
                self.reopen_audio();
            }
            EngineCommand::ClearSkimmerSpots => {
                self.skimmer.clear();
                self.reset_skimmer_peak_hold(self.fft_size);
            }
            EngineCommand::ReloadScp => {
                self.skimmer.reload_scp();
                self.publish_stats(0);
            }
            EngineCommand::ReloadScpFrom(path) => {
                self.skimmer.reload_scp_from(path);
                self.publish_stats(0);
            }
            EngineCommand::StartIqRecord(path) => {
                if self.recorder.is_some() {
                    return;
                }
                let (sr, center) = self
                    .conn
                    .as_ref()
                    .map(|c| (c.sample_rate as u32, c.center_hz))
                    .or_else(|| {
                        self.playback
                            .as_ref()
                            .map(|p| (p.meta().sample_rate, p.meta().center_hz))
                    })
                    .unwrap_or((12_000, 0.0));
                match IqRecorder::start(path.clone(), sr, center) {
                    Ok(rec) => {
                        self.recorder_samples = 0;
                        self.recorder = Some(rec);
                        log::info(format!("IQ recording → {}", path.display()));
                    }
                    Err(e) => self.set_error(Some(format!("IQ record failed: {e}"))),
                }
            }
            EngineCommand::StopIqRecord => {
                self.stop_recorder();
            }
            EngineCommand::PlayIqFile(path) => {
                self.teardown();
                self.request = None;
                match IqPlayback::open(path.clone()) {
                    Ok(pb) => {
                        let meta = pb.meta();
                        self.demod = IqAudioDemod::new();
                        self.audio_device_open(meta.sample_rate);
                        self.first_iq_received = true;
                        self.last_data = Instant::now();
                        self.rate_window_start = Instant::now();
                        self.rate_window_count = 0;
                        self.playback = Some(pb);
                        self.set_state(ConnState::Streaming);
                        self.set_error(None);
                        log::info(format!(
                            "IQ playback: {} ({:.1}s @ {} Hz)",
                            path.display(),
                            meta.duration_secs(),
                            meta.sample_rate
                        ));
                    }
                    Err(e) => {
                        self.set_error(Some(format!("IQ playback failed: {e}")));
                        self.set_state(ConnState::Disconnected);
                    }
                }
            }
            EngineCommand::StopIqPlayback => {
                self.playback = None;
                self.set_state(ConnState::Disconnected);
            }
            EngineCommand::Shutdown => {
                self.connect_cancel.store(true, Ordering::Relaxed);
                self.teardown();
                self.audio = None;
                self.ingress_worker.take();
                self.running = false;
            }
        }
    }
}
