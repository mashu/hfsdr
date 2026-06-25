//! Spectrum chain sync and wideband slicing.

use std::time::Instant;

use hfsdr::{Complex32, spectrum_hop, spectrum_plan, SpectrumAnalyzer};

use super::Engine;
use crate::engine::policy::{
    adaptive_spectrum_rows as policy_adaptive_rows, max_drain_for, max_fft_input_for,
    wideband_tail_len, SLOW_FRACTION, SLOW_HOLD, WIDEBAND_IQ_THRESHOLD, MAX_AUDIO_SAMPLES_WB,
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
        if fft != self.fft_size || hop != self.spectrum_hop {
            self.fft_size = fft;
            self.spectrum_hop = hop;
            self.analyzer = SpectrumAnalyzer::new(fft, hop);
            self.latest = vec![-120.0; fft];
            self.reset_skimmer_peak_hold(fft);
        }
    }

    pub(super) fn spectrum_fft_slice<'a>(
        &self,
        samples: &'a [Complex32],
        rate: f32,
        full_drain: bool,
    ) -> &'a [Complex32] {
        if full_drain || rate <= WIDEBAND_IQ_THRESHOLD {
            samples
        } else {
            self.wideband_tail(samples, rate, self.max_fft_input())
        }
    }

    pub(super) fn adaptive_spectrum_rows(&self, device_rate: f32) -> usize {
        policy_adaptive_rows(device_rate, self.cached_rate, self.iq_buffer_fill)
    }

    pub(super) fn max_drain(&self) -> usize {
        max_drain_for(self.link_meta().0)
    }

    pub(super) fn max_fft_input(&self) -> usize {
        max_fft_input_for(self.link_meta().0, self.spectrum_hop, self.fft_size)
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

    pub(super) fn demod_input<'a>(&self, samples: &'a [Complex32], rate: f32) -> &'a [Complex32] {
        if rate > WIDEBAND_IQ_THRESHOLD {
            self.wideband_tail(samples, rate, MAX_AUDIO_SAMPLES_WB)
        } else {
            samples
        }
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
        if effective < SLOW_FRACTION * nominal {
            let since = *self.slow_since.get_or_insert_with(Instant::now);
            since.elapsed() >= SLOW_HOLD
        } else {
            self.slow_since = None;
            false
        }
    }
}
