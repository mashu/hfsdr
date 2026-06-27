//! Spectrum chain sync and wideband slicing.

use std::time::Instant;

use hfsdr::{Complex32, spectrum_hop, spectrum_plan, SpectrumAnalyzer, MIN_KAISER_BETA, MAX_KAISER_BETA};

use super::Engine;
use crate::engine::policy::{
    adaptive_spectrum_rows as policy_adaptive_rows, demod_uses_full_batch, demod_tail_max,
    max_drain_for, slow_link, spectrum_aligned_len, wideband_tail_len,
};
use crate::engine::types::EngineParams;

impl Engine {
    pub(super) fn sync_spectrum_chain(&mut self, iq_rate: f32, params: &EngineParams) {
        // Always FFT the full passband; UI zoom/pan is a viewport crop on the waterfall.
        let _ = params;
        let (decim, fft, eff) = spectrum_plan(iq_rate, params.fft_size, params.fft_auto, iq_rate);
        self.spectrum_rate = eff;
        self.spectrum_decim = decim;
        self.spectrum_pan_hz = 0.0;
        self.spectrum_front.sync(iq_rate, decim, 0.0);
        let hop = spectrum_hop(fft, iq_rate);
        let window = params.spectrum_window;
        let beta = params.spectrum_kaiser_beta.clamp(MIN_KAISER_BETA, MAX_KAISER_BETA);
        if fft != self.fft_size
            || hop != self.spectrum_hop
            || window != self.spectrum_window
            || beta != self.spectrum_kaiser_beta
        {
            self.fft_size = fft;
            self.spectrum_hop = hop;
            self.spectrum_window = window;
            self.spectrum_kaiser_beta = beta;
            self.analyzer = SpectrumAnalyzer::with_window(fft, hop, window, beta);
            self.latest = vec![-120.0; fft];
            self.reset_skimmer_peak_hold(fft);
        }
    }

    pub(super) fn spectrum_fft_slice<'a>(
        &self,
        samples: &'a [Complex32],
        device_batch_len: usize,
        device_rate: f32,
        ingress_decim: usize,
        full_drain: bool,
    ) -> &'a [Complex32] {
        if full_drain {
            return samples;
        }
        let len = spectrum_aligned_len(device_batch_len, samples.len(), device_rate, ingress_decim);
        if len >= samples.len() {
            samples
        } else {
            &samples[samples.len() - len..]
        }
    }

    pub(super) fn adaptive_spectrum_rows(&self, device_rate: f32) -> usize {
        policy_adaptive_rows(device_rate, self.cached_rate, self.iq_buffer_fill)
    }

    pub(super) fn max_drain(&self) -> usize {
        max_drain_for(self.link_meta().0)
    }

    pub(super) fn wideband_tail<'a>(
        &self,
        samples: &'a [Complex32],
        rate: f32,
        max: usize,
    ) -> &'a [Complex32] {
        let len = wideband_tail_len(samples.len(), rate, max);
        if len == samples.len() {
            samples
        } else {
            &samples[samples.len() - len..]
        }
    }

    pub(super) fn demod_input<'a>(
        &self,
        samples: &'a [Complex32],
        rate: f32,
        full_demod: bool,
    ) -> &'a [Complex32] {
        let recording = self.recorder.is_some();
        if demod_uses_full_batch(recording, full_demod) {
            return samples;
        }
        let max = demod_tail_max(rate);
        self.wideband_tail(samples, rate, max)
    }
    pub(super) fn link_meta(&self) -> (f32, f64, bool) {
        if let Some(pb) = &self.playback {
            let m = pb.meta();
            (m.sample_rate as f32, m.center_hz, false)
        } else if let Some(c) = &self.conn {
            (c.device_sample_rate, c.center_hz, c.is_kiwi)
        } else {
            (12_000.0, 0.0, false)
        }
    }

    pub(super) fn iq_passband_hz(&self) -> f32 {
        if let Some(pb) = &self.playback {
            return pb.meta().sample_rate as f32;
        }
        if let Some(c) = &self.conn {
            if c.is_kiwi {
                hfsdr::kiwi_iq_half_hz(c.device_sample_rate as u32) as f32 * 2.0
            } else {
                c.device_sample_rate
            }
        } else {
            12_000.0
        }
    }
    pub(super) fn effective_rate(&mut self, _nominal: f32) -> f32 {
        let elapsed = self.rate_window_start.elapsed().as_secs_f32();
        if elapsed >= 0.5 {
            let rate = self.rate_window_count as f32 / elapsed;
            self.rate_window_count = 0;
            self.rate_window_start = Instant::now();
            self.cached_rate = rate;
        }
        self.cached_rate
    }

    pub(super) fn update_slow_flag(&mut self, nominal: f32, effective: f32) -> bool {
        if self.conn.is_none() || !self.first_iq_received {
            self.slow_since = None;
            return false;
        }
        let slow_since_secs = if effective < 0.7 * nominal {
            let since = *self.slow_since.get_or_insert_with(Instant::now);
            Some(since.elapsed().as_secs_f32())
        } else {
            self.slow_since = None;
            None
        };
        slow_link(effective, nominal, slow_since_secs)
    }
}
