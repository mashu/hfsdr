//! Per-pump timing and drop counters for the IQ processing pipeline.

/// Nanosecond timings and counters from one engine pump (or bench iteration).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct PipelineMetrics {
    pub drain_ns: u64,
    pub gain_ns: u64,
    pub record_ns: u64,
    pub demod_ns: u64,
    /// Ingress FIR decimation (engine thread) or dual-ring decim drain.
    pub ingress_ns: u64,
    pub spectrum_front_ns: u64,
    pub fft_ns: u64,
    pub skimmer_submit_ns: u64,
    pub publish_ns: u64,
    pub got_samples: usize,
    /// Engine catch-up pops on the raw IQ ring this pump.
    pub iq_dropped_catchup: u64,
    /// Bridge thread drops on the decimated ring (dual-ring path).
    pub decim_ring_dropped: u64,
    pub fft_rows: usize,
    /// `true` when spectrum IQ comes from a pre-decimated ring.
    pub dual_ring: bool,
}

impl PipelineMetrics {
    pub fn measured_total_ns(&self) -> u64 {
        self.drain_ns
            + self.gain_ns
            + self.record_ns
            + self.demod_ns
            + self.ingress_ns
            + self.spectrum_front_ns
            + self.fft_ns
            + self.skimmer_submit_ns
            + self.publish_ns
    }

    /// Exponential moving average (α ∈ (0, 1]).
    pub fn blend(&mut self, sample: &Self, alpha: f32) {
        let a = alpha.clamp(0.0, 1.0);
        let b = 1.0 - a;
        self.drain_ns = ema_u64(self.drain_ns, sample.drain_ns, a, b);
        self.gain_ns = ema_u64(self.gain_ns, sample.gain_ns, a, b);
        self.record_ns = ema_u64(self.record_ns, sample.record_ns, a, b);
        self.demod_ns = ema_u64(self.demod_ns, sample.demod_ns, a, b);
        self.ingress_ns = ema_u64(self.ingress_ns, sample.ingress_ns, a, b);
        self.spectrum_front_ns = ema_u64(self.spectrum_front_ns, sample.spectrum_front_ns, a, b);
        self.fft_ns = ema_u64(self.fft_ns, sample.fft_ns, a, b);
        self.skimmer_submit_ns = ema_u64(self.skimmer_submit_ns, sample.skimmer_submit_ns, a, b);
        self.publish_ns = ema_u64(self.publish_ns, sample.publish_ns, a, b);
        self.got_samples = ema_usize(self.got_samples, sample.got_samples, a, b);
        self.iq_dropped_catchup =
            ema_u64(self.iq_dropped_catchup, sample.iq_dropped_catchup, a, b);
        self.decim_ring_dropped =
            ema_u64(self.decim_ring_dropped, sample.decim_ring_dropped, a, b);
        self.fft_rows = ema_usize(self.fft_rows, sample.fft_rows, a, b);
        if sample.dual_ring {
            self.dual_ring = true;
        }
    }

    pub fn stage_rows(&self) -> [(&'static str, u64); 9] {
        [
            ("drain", self.drain_ns),
            ("gain", self.gain_ns),
            ("record", self.record_ns),
            ("demod", self.demod_ns),
            ("ingress", self.ingress_ns),
            ("spec front", self.spectrum_front_ns),
            ("fft", self.fft_ns),
            ("skimmer", self.skimmer_submit_ns),
            ("publish", self.publish_ns),
        ]
    }
}

fn ema_u64(prev: u64, sample: u64, a: f32, b: f32) -> u64 {
    (prev as f32 * b + sample as f32 * a).round() as u64
}

fn ema_usize(prev: usize, sample: usize, a: f32, b: f32) -> usize {
    (prev as f32 * b + sample as f32 * a).round() as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn total_sums_stages() {
        let m = PipelineMetrics {
            drain_ns: 100,
            fft_ns: 50,
            ..Default::default()
        };
        assert_eq!(m.measured_total_ns(), 150);
    }

    #[test]
    fn blend_moves_toward_sample() {
        let mut avg = PipelineMetrics {
            drain_ns: 1000,
            ..Default::default()
        };
        let sample = PipelineMetrics {
            drain_ns: 100,
            ..Default::default()
        };
        avg.blend(&sample, 0.5);
        assert!(avg.drain_ns > 100 && avg.drain_ns < 1000);
    }
}
