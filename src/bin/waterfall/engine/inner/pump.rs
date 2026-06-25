//! IQ drain pump and stats publishing.

use std::sync::Arc;
use std::time::Instant;

use hfsdr::{Complex32, FirDecimator};
use rayon::join;

use crate::log;
use super::Engine;
use crate::engine::policy::{
    is_wideband_rate, ring_catchup_target_slots, skimmer_throttle, SKIMMER_PEAK_HOLD_DECAY_DB,
};
use crate::engine::types::{ConnState, EngineStats};
use crate::engine::{WATERFALL_ROWS};


impl Engine {
/// Drain and process available IQ; returns sample count processed.
    pub(super) fn pump_stream(&mut self) -> usize {
        let params = self.params.lock().map(|g| g.clone()).unwrap_or_default();
        let dt = self
            .last_pump_at
            .elapsed()
            .as_secs_f32()
            .clamp(0.001, 0.1);
        let ring_before = self.measure_iq_buffer();

        self.drain.clear();
        let drain_cap = self.max_drain();
        if let Some(pb) = &mut self.playback {
            while self.drain.len() < drain_cap {
                match pb.pop() {
                    Some(s) => self.drain.push(s),
                    None => break,
                }
            }
            if pb.finished() && self.drain.is_empty() {
                self.playback = None;
                self.set_state(ConnState::Disconnected);
                log::info("IQ playback finished");
            }
        } else if let Some(conn) = &mut self.conn {
            // Never discard ring samples while recording — every sample must reach the file.
            if self.recorder.is_none() {
                let cap = conn.iq_ring_capacity.max(1);
                let slots = conn.iq.slots();
                if let Some(target) = ring_catchup_target_slots(slots, cap, false) {
                    while conn.iq.slots() > target {
                        let _ = conn.iq.pop();
                    }
                }
            }
            while self.drain.len() < drain_cap {
                match conn.iq.pop() {
                    Ok(s) => self.drain.push(s),
                    Err(_) => break,
                }
            }
        }
        let (device_rate, center_hz, _is_kiwi) = if let Some(pb) = &self.playback {
            let m = pb.meta();
            (m.sample_rate as f32, m.center_hz, false)
        } else {
            self.conn
                .as_ref()
                .map(|c| (c.device_sample_rate, c.center_hz, c.is_kiwi))
                .unwrap_or((12_000.0, 0.0, false))
        };
        let ingress_decim = self
            .conn
            .as_ref()
            .map(|c| c.iq_ingress_decim)
            .unwrap_or(1)
            .max(1);
        let got = self.drain.len();
        self.last_pump_got = got;
        self.update_ring_utilization(device_rate, ring_before, got, dt);
        self.last_pump_at = Instant::now();
        if got == 0 {
            self.publish_stats(0);
            return 0;
        }
        if let Some(rec) = &self.recorder {
            rec.push(&self.drain);
            self.recorder_samples += got as u64;
        }
        // Yaesu-style software RF gain: scale the IQ after recording (so captures stay raw)
        // but before spectrum/S-meter/AGC see it. One chokepoint → identical behavior on
        // every source and on playback, even when hardware/RF AGC is active.
        apply_software_rf_gain(&mut self.drain, params.rf_gain_db);
        if !self.first_iq_received {
            self.first_iq_received = true;
            self.rate_window_start = Instant::now();
            self.rate_window_count = 0;
            self.set_state(ConnState::Streaming);
        }
        self.last_data = Instant::now();
        self.rate_window_count += got as u64;

        let spectrum_input_rate = if let Some(pb) = &self.playback {
            pb.meta().sample_rate as f32
        } else {
            self.conn
                .as_ref()
                .map(|c| c.sample_rate)
                .unwrap_or(device_rate)
        };

        self.sync_spectrum_chain(spectrum_input_rate, &params);
        self.touch_skimmer_center(center_hz);

        let cw = params.cw.clone();
        let wideband = is_wideband_rate(device_rate);
        let batch = Arc::new(std::mem::take(&mut self.drain));
        self.drain = Vec::with_capacity(self.max_drain());

        if ingress_decim > 1 {
            let rebuild_ingress = ingress_decim != self.spectrum_ingress_factor
                || (device_rate - self.spectrum_ingress_rate).abs() > 1.0
                || cw.decim_filter != self.spectrum_ingress_filter;
            if rebuild_ingress {
                self.spectrum_ingress = FirDecimator::with_factor(
                    device_rate,
                    ingress_decim,
                    true,
                    cw.decim_filter,
                );
                self.spectrum_ingress_factor = ingress_decim;
                self.spectrum_ingress_rate = device_rate;
                self.spectrum_ingress_filter = cw.decim_filter;
            } else {
                self.spectrum_ingress.sync_filter(device_rate, cw.decim_filter);
            }
        }

        let use_ingress_worker = wideband
            && ingress_decim > 1
            && self.spectrum_decim <= 1
            && self.ingress_worker.as_ref().is_some_and(|w| {
                w.start(
                    Arc::clone(&batch),
                    device_rate,
                    ingress_decim,
                    cw.decim_filter,
                )
            });

        if use_ingress_worker {
            self.demod.process(
                self.demod_input(batch.as_slice(), device_rate),
                device_rate,
                &cw,
                &mut self.audio_scratch,
            );
            if let Some(decimated) = self.ingress_worker.as_ref().and_then(|w| w.finish()) {
                self.drain_decim = decimated;
            } else {
                self.spectrum_ingress
                    .decimate_block(batch.as_slice(), &mut self.drain_decim, false);
            }
        } else if wideband && ingress_decim > 1 && self.spectrum_decim <= 1 {
            let batch_demod = Arc::clone(&batch);
            let demod_input = self.demod_input(batch_demod.as_slice(), device_rate);
            let (demod, audio_scratch, ingress, decim_buf) = (
                &mut self.demod,
                &mut self.audio_scratch,
                &mut self.spectrum_ingress,
                &mut self.drain_decim,
            );
            join(
                || demod.process(demod_input, device_rate, &cw, audio_scratch),
                || ingress.decimate_block(batch.as_slice(), decim_buf, false),
            );
        } else {
            if ingress_decim > 1 {
                self.spectrum_ingress
                    .decimate_block(batch.as_slice(), &mut self.drain_decim, false);
            }
            if wideband && self.spectrum_decim > 1 {
                let ingress_base: &[Complex32] = if ingress_decim > 1 {
                    &self.drain_decim
                } else {
                    batch.as_slice()
                };
                let fft_base = self.spectrum_fft_slice(
                    ingress_base,
                    device_rate,
                    params.full_drain_spectrum,
                );
                let batch_demod = Arc::clone(&batch);
                let demod_input = self.demod_input(batch_demod.as_slice(), device_rate);
                let (demod, spectrum_front) = (&mut self.demod, &mut self.spectrum_front);
                let (audio_scratch, spectrum_scratch) =
                    (&mut self.audio_scratch, &mut self.spectrum_scratch);
                join(
                    || demod.process(demod_input, device_rate, &cw, audio_scratch),
                    || spectrum_front.process(fft_base, spectrum_scratch),
                );
            } else {
                self.demod.process(
                    self.demod_input(batch.as_slice(), device_rate),
                    device_rate,
                    &cw,
                    &mut self.audio_scratch,
                );
            }
        }

        let ingress_base: &[Complex32] = if ingress_decim > 1 {
            &self.drain_decim
        } else {
            batch.as_slice()
        };
        let fft_base = self.spectrum_fft_slice(
            ingress_base,
            device_rate,
            params.full_drain_spectrum,
        );
        if self.spectrum_decim > 1 {
            if !(wideband && self.spectrum_decim > 1) {
                self.spectrum_front
                    .process(fft_base, &mut self.spectrum_scratch);
            }
        } else {
            self.spectrum_scratch.clear();
            self.spectrum_scratch.extend_from_slice(fft_base);
        }

        if params.audio_enabled {
            if self.audio.is_none() {
                self.audio_device_open(0);
            }
            if let Some(audio) = &mut self.audio {
                let audio_rate = hfsdr::audio_sample_rate(device_rate, params.cw.decimation);
                audio.push(&self.audio_scratch, audio_rate as u32, params.volume);
            }
        }
        if !self.audio_scratch.is_empty() {
            self.audio_scope.push_block(&self.audio_scratch);
            self.level_audio_scope = self.audio_scope.ordered();
        }

        let agc_gain = if params.cw.agc.enabled {
            self.demod.agc_gain()
        } else {
            params.cw.agc.manual_gain
        };
        self.level_agc_gain = agc_gain;
        self.level_agc_envelope = self.demod.agc_envelope();
        self.level_iq_rf = self.demod.iq_rf_level();
        self.level_audio_peak = self.audio_scope.peak;
        self.level_audio_rms = self.audio_scope.rms;

        let fft_input: &[Complex32] = &self.spectrum_scratch;
        let max_rows = self.adaptive_spectrum_rows(device_rate);
        self.last_spectrum_rows = max_rows;
        let playback = self.playback.is_some();
        let analyzer = &mut self.analyzer;
        let latest = &mut self.latest;
        let skimmer_peak_hold = &mut self.skimmer_peak_hold;
        let row_pool = &mut self.row_pool;
        let mut produced: Vec<Vec<f32>> = Vec::new();
        analyzer.process_limited(fft_input, max_rows, |row| {
            latest.copy_from_slice(row);
            if skimmer_peak_hold.len() != row.len() {
                skimmer_peak_hold.resize(row.len(), -120.0);
            }
            if playback {
                for (hold, &sample) in skimmer_peak_hold.iter_mut().zip(row.iter()) {
                    *hold = hold.max(sample);
                }
            } else {
                for (hold, &sample) in skimmer_peak_hold.iter_mut().zip(row.iter()) {
                    *hold = (*hold - SKIMMER_PEAK_HOLD_DECAY_DB).max(sample);
                }
            }
            let mut buf = row_pool
                .pop()
                .unwrap_or_else(|| vec![-120.0; row.len()]);
            if buf.len() != row.len() {
                buf.resize(row.len(), -120.0);
            }
            buf.copy_from_slice(row);
            produced.push(buf);
        });

        self.pump_serial = self.pump_serial.wrapping_add(1);
        let run_skimmer = params.skimmer_enabled;
        self.skimmer.set_enabled(run_skimmer);
        if run_skimmer {
            let spectrum_iq_rate = if let Some(pb) = &self.playback {
                pb.meta().sample_rate as f32
            } else if let Some(c) = &self.conn {
                c.sample_rate
            } else {
                device_rate
            };
            let (skimmer_iq, skimmer_iq_rate) = if ingress_decim > 1 && !self.drain_decim.is_empty() {
                (self.drain_decim.as_slice(), spectrum_iq_rate)
            } else {
                (batch.as_slice(), device_rate)
            };
            let is_kiwi = self.conn.as_ref().is_some_and(|c| c.is_kiwi);
            let throttle = skimmer_throttle(is_kiwi, skimmer_iq_rate);
            if self.pump_serial % throttle == 0 {
                let mut cfg = params.skimmer.clone();
                cfg.source_label = "rx".to_string();
                self.skimmer.set_config(cfg);
                self.skimmer.submit(
                    skimmer_iq,
                    &self.skimmer_peak_hold,
                    skimmer_iq_rate,
                    self.spectrum_rate,
                    self.spectrum_pan_hz,
                    center_hz,
                );
            }
        }

        let snr = self.demod.snr_db();
        self.publish_rows(produced, snr, got);
        got
    }
    pub(super) fn measure_iq_buffer(&self) -> (f32, f32) {
        if let Some(pb) = &self.playback {
            let fill = pb.buffer_fill();
            let secs = pb.buffer_secs();
            (fill, secs)
        } else if let Some(conn) = &self.conn {
            let cap = conn.iq_ring_capacity.max(1);
            let slots = conn.iq.slots();
            let fill = slots as f32 / cap as f32;
            let secs = slots as f32 / conn.device_sample_rate.max(1.0);
            (fill, secs)
        } else {
            (0.0, 0.0)
        }
    }

    pub(super) fn update_ring_utilization(
        &mut self,
        sample_rate: f32,
        ring_before: (f32, f32),
        got: usize,
        dt: f32,
    ) {
        let (ring_fill, ring_secs) = ring_before;
        self.iq_buffer_peak = (self.iq_buffer_peak * 0.985).max(ring_fill);

        if self.playback.is_some() {
            // Disk playback: bar tracks ring occupancy (should stay high).
            let util = if got > 0 {
                ring_fill.max(0.75)
            } else {
                ring_fill * 0.5
            };
            self.iq_buffer_fill = self.iq_buffer_fill * 0.55 + util * 0.45;
            self.iq_buffer_secs = self.iq_buffer_secs * 0.55 + ring_secs * 0.45;
            return;
        }

        if self.conn.is_none() || !self.first_iq_received {
            self.iq_buffer_fill *= 0.8;
            self.iq_buffer_secs *= 0.8;
            return;
        }

        let nominal = sample_rate.max(1.0);
        let expected = nominal * dt;
        let throughput = if got == 0 {
            0.0
        } else {
            (got as f32 / expected).min(1.0)
        };

        // High when we consume a full pump batch; 0 when starved (got == 0).
        let util = if got == 0 {
            0.0
        } else {
            throughput
                .max(ring_fill)
                .max(self.iq_buffer_peak * 0.6)
        };

        if got == 0 {
            self.iq_buffer_fill *= 0.45;
        } else {
            self.iq_buffer_fill = self.iq_buffer_fill * 0.5 + util * 0.5;
        }
        let queued_secs = if got > 0 {
            ring_secs.max(got as f32 / nominal)
        } else {
            ring_secs
        };
        self.iq_buffer_secs = self.iq_buffer_secs * 0.5 + queued_secs * 0.5;
    }
    pub(super) fn publish_rows(&mut self, rows: Vec<Vec<f32>>, snr: f32, got: usize) {
        let spots = self.skimmer.spots();
        let scp = self.skimmer.scp_status();
        let channels = self.skimmer.active_channels();
        let dropped = self.conn.as_ref().map(|c| c.device.dropped_samples()).unwrap_or(0);
        let rssi = self.conn.as_ref().and_then(|c| c.device.rssi_dbm());
        let (sample_rate, _, is_kiwi) = self.link_meta();
        let (iq_recording, iq_playback, iq_capture_samples, iq_capture_path) = self.capture_ui();
        let effective = self.effective_rate(sample_rate);
        let slow = self.update_slow_flag(sample_rate, effective);
        let (audio_device, audio_rate) = self
            .audio
            .as_ref()
            .map(|a| (Some(a.device_name().to_string()), a.output_rate()))
            .unwrap_or((None, 0));
        let (iq_buffer_fill, iq_buffer_secs) = self.iq_buffer_stats();
        let (kiwi_has_rf_attn, kiwi_rf_attn_db) = self.kiwi_rf_stats();
        let hw_rf_gain = self.hw_rf_gain();

        if let Ok(mut guard) = self.shared.lock() {
            if guard.latest.len() == self.latest.len() {
                guard.latest.copy_from_slice(&self.latest);
            } else {
                guard.latest = self.latest.clone();
            }
            for row in rows {
                if guard.new_rows.len() >= WATERFALL_ROWS {
                    guard.new_rows.pop_front();
                }
                guard.new_rows.push_back(row);
                guard.rows_seq = guard.rows_seq.wrapping_add(1);
            }
            guard.spots = spots;
            guard.stats = EngineStats {
                sample_rate: self
                    .conn
                    .as_ref()
                    .map(|c| c.sample_rate)
                    .unwrap_or(sample_rate),
                iq_passband_hz: self.iq_passband_hz(),
                effective_sps: effective,
                last_drain: got,
                dropped,
                rssi_dbm: rssi,
                snr_db: snr,
                audio_device,
                audio_rate,
                slow,
                is_kiwi,
                skimmer_channels: channels,
                spectrum_rate: self.spectrum_rate,
                spectrum_fft: self.fft_size,
                spectrum_decim: self.spectrum_decim,
                spectrum_zoomed: self.spectrum_decim > 1,
                spectrum_rows_per_pump: self.last_spectrum_rows,
                scp,
                iq_recording,
                iq_playback,
                iq_capture_samples,
                iq_capture_path,
                iq_buffer_fill,
                iq_buffer_secs,
                audio_peak: self.level_audio_peak,
                audio_rms: self.level_audio_rms,
                agc_gain: self.level_agc_gain,
                agc_envelope: self.level_agc_envelope,
                iq_rf_level: self.level_iq_rf,
                kiwi_has_rf_attn,
                kiwi_rf_attn_db,
                hw_rf_gain,
            };
            guard.audio_scope = self.level_audio_scope.clone();
        }
    }

    pub(super) fn capture_ui(&self) -> (bool, bool, u64, Option<String>) {
        (
            self.recorder.is_some(),
            self.playback.is_some(),
            self.recorder_samples,
            self.recorder
                .as_ref()
                .map(|r| r.path().display().to_string()),
        )
    }

    pub(super) fn publish_stats(&mut self, got: usize) {
        let scp = self.skimmer.scp_status();
        let dropped = self.conn.as_ref().map(|c| c.device.dropped_samples()).unwrap_or(0);
        let rssi = self.conn.as_ref().and_then(|c| c.device.rssi_dbm());
        let (sample_rate, _, is_kiwi) = self.link_meta();
        let (iq_recording, iq_playback, iq_capture_samples, iq_capture_path) = self.capture_ui();
        let effective = self.effective_rate(sample_rate);
        let slow = self.update_slow_flag(sample_rate, effective);
        let (audio_device, audio_rate) = self
            .audio
            .as_ref()
            .map(|a| (Some(a.device_name().to_string()), a.output_rate()))
            .unwrap_or((None, 0));
        let (iq_buffer_fill, iq_buffer_secs) = self.iq_buffer_stats();
        let (kiwi_has_rf_attn, kiwi_rf_attn_db) = self.kiwi_rf_stats();
        let hw_rf_gain = self.hw_rf_gain();
        if let Ok(mut guard) = self.shared.lock() {
            guard.stats = EngineStats {
                sample_rate: self
                    .conn
                    .as_ref()
                    .map(|c| c.sample_rate)
                    .unwrap_or(sample_rate),
                iq_passband_hz: self.iq_passband_hz(),
                effective_sps: effective,
                last_drain: got,
                dropped,
                rssi_dbm: rssi,
                snr_db: guard.stats.snr_db,
                audio_device,
                audio_rate,
                slow,
                is_kiwi,
                skimmer_channels: self.skimmer.active_channels(),
                spectrum_rate: self.spectrum_rate,
                spectrum_fft: self.fft_size,
                spectrum_decim: self.spectrum_decim,
                spectrum_zoomed: self.spectrum_decim > 1,
                spectrum_rows_per_pump: self.last_spectrum_rows,
                scp,
                iq_recording,
                iq_playback,
                iq_capture_samples,
                iq_capture_path,
                iq_buffer_fill,
                iq_buffer_secs,
                audio_peak: self.level_audio_peak,
                audio_rms: self.level_audio_rms,
                agc_gain: self.level_agc_gain,
                agc_envelope: self.level_agc_envelope,
                iq_rf_level: self.level_iq_rf,
                kiwi_has_rf_attn,
                kiwi_rf_attn_db,
                hw_rf_gain,
            };
        }
    }

    pub(super) fn iq_buffer_stats(&self) -> (f32, f32) {
        (self.iq_buffer_fill, self.iq_buffer_secs)
    }

    pub(super) fn kiwi_rf_stats(&self) -> (bool, f32) {
        self.conn
            .as_ref()
            .map(|c| c.device.kiwi_rf_stats())
            .unwrap_or((false, 0.0))
    }

    pub(super) fn hw_rf_gain(&self) -> Option<u8> {
        self.conn.as_ref().and_then(|c| c.device.hw_rf_gain())
    }
}

/// Scale IQ in place by a software RF gain in dB (no-op at 0 dB).
fn apply_software_rf_gain(iq: &mut [Complex32], gain_db: f32) {
    if gain_db.abs() < 1e-3 {
        return;
    }
    let g = 10f32.powf(gain_db / 20.0);
    for s in iq.iter_mut() {
        s.re *= g;
        s.im *= g;
    }
}

#[cfg(test)]
mod rf_gain_tests {
    use super::apply_software_rf_gain;
    use hfsdr::Complex32;

    #[test]
    fn zero_db_is_noop() {
        let mut iq = vec![Complex32::new(0.3, -0.4)];
        apply_software_rf_gain(&mut iq, 0.0);
        assert!((iq[0].re - 0.3).abs() < 1e-9);
        assert!((iq[0].im + 0.4).abs() < 1e-9);
    }

    #[test]
    fn twenty_db_is_ten_times_amplitude() {
        let mut iq = vec![Complex32::new(0.1, 0.0)];
        apply_software_rf_gain(&mut iq, 20.0);
        assert!((iq[0].re - 1.0).abs() < 1e-5);
    }

    #[test]
    fn negative_db_attenuates() {
        let mut iq = vec![Complex32::new(1.0, 0.0)];
        apply_software_rf_gain(&mut iq, -20.0);
        assert!((iq[0].re - 0.1).abs() < 1e-5);
    }
}
