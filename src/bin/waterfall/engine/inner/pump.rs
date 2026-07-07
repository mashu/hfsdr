//! IQ drain pump and stats publishing.

use std::sync::Arc;
use std::time::{Duration, Instant};

use hfsdr::{Complex32, FirDecimator, PipelineMetrics};
use rayon::join;

use crate::log;
use super::Engine;
use crate::engine::perf::perf_enabled;
use crate::engine::policy::{
    is_wideband_rate, ring_catchup_target_slots, skimmer_throttle, SKIMMER_PEAK_HOLD_DECAY_DB,
};
use crate::engine::types::{ConnState, EngineStats};
use crate::engine::{WATERFALL_ROWS};
use crate::source::Connection;


impl Engine {
/// Drain and process available IQ; returns sample count processed.
    pub(crate) fn pump_stream(&mut self) -> usize {
        self.last_iq_dropped = 0;
        let params = self.params.lock().map(|g| g.clone()).unwrap_or_default();
        let perf = perf_enabled(&params);
        let mut metrics = PipelineMetrics::default();
        let t_pump = Instant::now();

        let dt = self
            .last_pump_at
            .elapsed()
            .as_secs_f32()
            .clamp(0.001, 0.1);
        let ring_before = self.measure_iq_buffer();

        let t_drain = Instant::now();
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
            let mut iq_dropped = 0u64;
            if self.recorder.is_none() {
                let cap = conn.iq_ring_capacity.max(1);
                let slots = conn.iq.slots();
                if let Some(target) = ring_catchup_target_slots(slots, cap, false) {
                    while conn.iq.slots() > target {
                        let _ = conn.iq.pop();
                        iq_dropped += 1;
                    }
                }
            }
            while self.drain.len() < drain_cap {
                match conn.iq.pop() {
                    Ok(s) => self.drain.push(s),
                    Err(_) => break,
                }
            }
            self.last_iq_dropped = iq_dropped;
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
        let dual_ring = self.conn.as_ref().is_some_and(|c| c.dual_ring_active());
        metrics.dual_ring = dual_ring;

        let got = self.drain.len();
        if dual_ring && got > 0 {
            self.drain_decim.clear();
            if let Some(conn) = &mut self.conn {
                if let Some(decim) = conn.iq_spectrum.as_mut() {
                    let want = (got / ingress_decim).max(1) + self.spectrum_hop;
                    while self.drain_decim.len() < want {
                        match decim.pop() {
                            Ok(s) => self.drain_decim.push(s),
                            Err(_) => break,
                        }
                    }
                }
                metrics.decim_ring_dropped = conn.bridge_decim_dropped();
                metrics.raw_ring_dropped = conn.bridge_raw_dropped();
            }
        }
        if perf {
            metrics.drain_ns = t_drain.elapsed().as_nanos() as u64;
            metrics.got_samples = got;
            metrics.iq_dropped_catchup = self.last_iq_dropped;
        }

        self.last_pump_got = got;
        self.update_ring_utilization(device_rate, ring_before, got, dt);
        self.last_pump_at = Instant::now();
        if got == 0 {
            self.finish_pipeline_metrics(perf, &metrics, false);
            self.publish_stats(0);
            return 0;
        }

        let t_record = Instant::now();
        if let Some(rec) = &self.recorder {
            rec.push(&self.drain);
            self.recorder_samples += got as u64;
        }
        if perf {
            metrics.record_ns = t_record.elapsed().as_nanos() as u64;
        }

        let t_gain = Instant::now();
        // Yaesu-style software RF gain: scale the IQ after recording (so captures stay raw)
        // but before spectrum/S-meter/AGC see it. One chokepoint → identical behavior on
        // every source and on playback, even when hardware/RF AGC is active.
        let gain_db = effective_rf_gain_db(params.rf_gain_db, self.conn.as_ref());
        apply_software_rf_gain(&mut self.drain, gain_db);
        if dual_ring && !self.drain_decim.is_empty() {
            apply_software_rf_gain(&mut self.drain_decim, gain_db);
        }
        if perf {
            metrics.gain_ns = t_gain.elapsed().as_nanos() as u64;
        }
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
        if let Some(conn) = &self.conn {
            conn.sync_bridge_decim_filter(cw.decim_filter);
        }
        let wideband = is_wideband_rate(device_rate);
        let batch = Arc::new(std::mem::take(&mut self.drain));
        // `self.drain` is left empty here; the batch Vec is reclaimed from the
        // Arc at the end of the pump instead of allocating a fresh one.

        // Samples skipped by the demod tail cap (full_demod off) — the audio
        // stream jumps in time, so the seam gets a short fade-in below.
        let demod_skipped = batch.len()
            - self
                .demod_input(batch.as_slice(), device_rate, cw.full_demod)
                .len();

        let t_demod = Instant::now();
        if !dual_ring && ingress_decim > 1 {
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

        let want_ingress_worker = !dual_ring
            && wideband
            && ingress_decim > 1
            && self.spectrum_decim <= 1
            && self.ingress_worker.is_some();
        let use_ingress_worker = want_ingress_worker && {
            let reuse = std::mem::take(&mut self.drain_decim);
            self.ingress_worker.as_ref().is_some_and(|w| {
                w.start(
                    Arc::clone(&batch),
                    device_rate,
                    ingress_decim,
                    cw.decim_filter,
                    reuse,
                )
            })
        };

        if use_ingress_worker {
            self.demod.process(
                self.demod_input(batch.as_slice(), device_rate, cw.full_demod),
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
        } else if wideband && ingress_decim > 1 && self.spectrum_decim <= 1 && !dual_ring {
            let batch_demod = Arc::clone(&batch);
            let demod_input = self.demod_input(batch_demod.as_slice(), device_rate, cw.full_demod);
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
            if !dual_ring && ingress_decim > 1 {
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
                    batch.len(),
                    device_rate,
                    ingress_decim,
                    params.full_drain_spectrum,
                );
                let batch_demod = Arc::clone(&batch);
                let demod_input = self.demod_input(batch_demod.as_slice(), device_rate, cw.full_demod);
                let (demod, spectrum_front) = (&mut self.demod, &mut self.spectrum_front);
                let (audio_scratch, spectrum_scratch) =
                    (&mut self.audio_scratch, &mut self.spectrum_scratch);
                join(
                    || demod.process(demod_input, device_rate, &cw, audio_scratch),
                    || spectrum_front.process(fft_base, spectrum_scratch),
                );
            } else {
                self.demod.process(
                    self.demod_input(batch.as_slice(), device_rate, cw.full_demod),
                    device_rate,
                    &cw,
                    &mut self.audio_scratch,
                );
            }
        }
        if perf {
            metrics.demod_ns = t_demod.elapsed().as_nanos() as u64;
        }

        let t_ingress = Instant::now();
        let ingress_base: &[Complex32] =
            if (dual_ring || ingress_decim > 1) && !self.drain_decim.is_empty() {
                &self.drain_decim
            } else {
                batch.as_slice()
            };
        if perf {
            metrics.ingress_ns = t_ingress.elapsed().as_nanos() as u64;
        }

        let t_spec_front = Instant::now();
        let fft_base = self.spectrum_fft_slice(
            ingress_base,
            batch.len(),
            device_rate,
            ingress_decim,
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
        if perf {
            metrics.spectrum_front_ns = t_spec_front.elapsed().as_nanos() as u64;
        }

        if demod_skipped > 0 && !self.audio_scratch.is_empty() {
            // The demod input jumped forward in time — soften the seam.
            let audio_rate = hfsdr::audio_sample_rate(device_rate, params.cw.decimation);
            fade_in_seam(&mut self.audio_scratch, audio_rate);
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
        if self.last_iq_dropped > 0 {
            if let Some(audio) = &mut self.audio {
                let skip_secs = self.last_iq_dropped as f32 / device_rate.max(1.0);
                audio.skip_seconds(skip_secs);
            }
            self.last_iq_dropped = 0;
        }
        if !self.audio_scratch.is_empty() {
            let audio_rate = hfsdr::audio_sample_rate(device_rate, params.cw.decimation);
            self.audio_scope
                .push_block(&self.audio_scratch, audio_rate);
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
        self.level_estimated_wpm = self.demod.estimated_wpm();
        self.level_keying_confident = self.demod.keying_speed_confident();
        self.level_audio_peak = self.audio_scope.peak;
        self.level_audio_rms = self.audio_scope.rms;

        let t_fft = Instant::now();
        let fft_input: &[Complex32] = &self.spectrum_scratch;
        let max_rows = self.adaptive_spectrum_rows(device_rate);
        self.last_spectrum_rows = max_rows;
        let playback = self.playback.is_some();
        let analyzer = &mut self.analyzer;
        let latest = &mut self.latest;
        let skimmer_peak_hold = &mut self.skimmer_peak_hold;
        let row_pool = &mut self.row_pool;
        let mut produced: Vec<Vec<f32>> = Vec::new();
        let fft_rows = analyzer.process_limited(fft_input, max_rows, |row| {
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
        if perf {
            metrics.fft_ns = t_fft.elapsed().as_nanos() as u64;
            metrics.fft_rows = fft_rows;
        }

        self.pump_serial = self.pump_serial.wrapping_add(1);
        let run_skimmer = params.skimmer_enabled;
        self.skimmer.set_enabled(run_skimmer);
        let t_skimmer = Instant::now();
        if run_skimmer {
            let spectrum_iq_rate = if let Some(pb) = &self.playback {
                pb.meta().sample_rate as f32
            } else if let Some(c) = &self.conn {
                c.sample_rate
            } else {
                device_rate
            };
            let (skimmer_iq, skimmer_iq_rate) =
                if (dual_ring || ingress_decim > 1) && !self.drain_decim.is_empty() {
                (self.drain_decim.as_slice(), spectrum_iq_rate)
            } else {
                (batch.as_slice(), device_rate)
            };
            let is_kiwi = self.conn.as_ref().is_some_and(|c| c.is_kiwi);
            let throttle = skimmer_throttle(is_kiwi, skimmer_iq_rate);
            if self.pump_serial.is_multiple_of(throttle) {
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
        if perf {
            metrics.skimmer_submit_ns = t_skimmer.elapsed().as_nanos() as u64;
        }

        // Reclaim the drained batch buffer for the next pump (no per-pump alloc).
        if let Ok(mut v) = Arc::try_unwrap(batch) {
            v.clear();
            self.drain = v;
        }

        let snr = self.demod.snr_db();
        let t_publish = Instant::now();
        self.publish_rows(produced, snr, got);
        if perf {
            metrics.publish_ns = t_publish.elapsed().as_nanos() as u64;
        }
        let slow = self
            .shared
            .lock()
            .map(|g| g.stats.slow)
            .unwrap_or(false);
        self.finish_pipeline_metrics(perf, &metrics, slow);
        let _ = t_pump;
        got
    }

    fn finish_pipeline_metrics(&mut self, perf: bool, sample: &PipelineMetrics, slow: bool) {
        if !perf {
            return;
        }
        self.last_pipeline = sample.clone();
        self.pipeline_avg.blend(sample, 0.15);
        if slow && self.last_perf_log.elapsed() >= Duration::from_secs(5) {
            log::warn(pipeline_perf_summary("slow link", &self.pipeline_avg));
            self.last_perf_log = Instant::now();
        }
    }

    pub(super) fn attach_pipeline_stats(&self, stats: &mut EngineStats) {
        if self.last_pipeline.measured_total_ns() > 0 {
            stats.pipeline = self.last_pipeline.clone();
            stats.pipeline_avg = self.pipeline_avg.clone();
        }
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
            let mut stats = EngineStats {
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
                estimated_wpm: self.level_estimated_wpm,
                keying_confident: self.level_keying_confident,
                kiwi_has_rf_attn,
                kiwi_rf_attn_db,
                hw_rf_gain,
                pipeline: PipelineMetrics::default(),
                pipeline_avg: PipelineMetrics::default(),
            };
            self.attach_pipeline_stats(&mut stats);
            guard.stats = stats;
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
            let mut stats = EngineStats {
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
                estimated_wpm: self.level_estimated_wpm,
                keying_confident: self.level_keying_confident,
                kiwi_has_rf_attn,
                kiwi_rf_attn_db,
                hw_rf_gain,
                pipeline: PipelineMetrics::default(),
                pipeline_avg: PipelineMetrics::default(),
            };
            self.attach_pipeline_stats(&mut stats);
            guard.stats = stats;
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

/// Total software RF gain: user knob + Kiwi `manGain` when Kiwi RF AGC hides it from firmware.
fn effective_rf_gain_db(rf_gain_db: f32, conn: Option<&Connection>) -> f32 {
    let extra = conn.map(Connection::kiwi_software_man_gain_db).unwrap_or(0.0);
    (rf_gain_db + extra).clamp(-80.0, 80.0)
}

/// Half-cosine fade-in (~4 ms) over the start of `audio` — masks the click
/// when the demod input skipped ahead in time (full_demod off on catch-up).
fn fade_in_seam(audio: &mut [f32], audio_rate: f32) {
    let n = ((audio_rate * 0.004) as usize).clamp(1, audio.len());
    for (k, sample) in audio[..n].iter_mut().enumerate() {
        let t = k as f32 / n as f32;
        *sample *= 0.5 - 0.5 * (std::f32::consts::PI * t).cos();
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

fn pipeline_perf_summary(label: &str, m: &PipelineMetrics) -> String {
    let total = m.measured_total_ns().max(1) as f64;
    let mut parts: Vec<String> = m
        .stage_rows()
        .into_iter()
        .filter(|(_, ns)| *ns > 0)
        .map(|(name, ns)| format!("{name} {:.0}%", ns as f64 / total * 100.0))
        .collect();
    if m.dual_ring {
        parts.push("dual-ring".into());
    }
    if m.iq_dropped_catchup > 0 {
        parts.push(format!("iq-drop {}", m.iq_dropped_catchup));
    }
    if m.raw_ring_dropped > 0 {
        parts.push(format!("raw-drop {}", m.raw_ring_dropped));
    }
    if m.decim_ring_dropped > 0 {
        parts.push(format!("decim-drop {}", m.decim_ring_dropped));
    }
    let head = format!(
        "pipeline [{label}]: {:.0} µs/pump, {} rows",
        total / 1000.0,
        m.fft_rows
    );
    if parts.is_empty() {
        head
    } else {
        format!("{head} ({})", parts.join(", "))
    }
}

#[cfg(test)]
mod rf_gain_tests {
    use super::{apply_software_rf_gain, effective_rf_gain_db};
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

    #[test]
    fn effective_rf_gain_clamps_extremes() {
        assert!((effective_rf_gain_db(100.0, None) - 80.0).abs() < 1e-6);
        assert!((effective_rf_gain_db(-100.0, None) + 80.0).abs() < 1e-6);
    }
}
