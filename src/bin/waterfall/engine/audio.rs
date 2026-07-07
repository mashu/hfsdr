//! AF scope ring buffers (engine thread only).

/// Envelope slots in the ring (~6 s at 50 ms/slot).
pub(crate) const SCOPE_LEN: usize = 120;
/// One scope column per this much audio time — near a CW dit at ~20–25 WPM.
const SCOPE_SLOT_MS: f32 = 50.0;

/// Raw audio samples for waveform / phosphor view (~85 ms at 48 kHz).
pub(crate) const WAVEFORM_LEN: usize = 4096;

pub(super) struct AudioScopeRing {
    buf: Vec<f32>,
    write: usize,
    carry: Vec<f32>,
    samples_per_slot: usize,
    pub(super) peak: f32,
    pub(super) rms: f32,
}

impl AudioScopeRing {
    pub(super) fn new() -> Self {
        Self {
            buf: vec![0.0; SCOPE_LEN],
            write: 0,
            carry: Vec::new(),
            samples_per_slot: 32,
            peak: 0.0,
            rms: 0.0,
        }
    }

    pub(super) fn push_block(&mut self, samples: &[f32], audio_sample_rate: f32) {
        if samples.is_empty() {
            return;
        }
        self.samples_per_slot = (audio_sample_rate * SCOPE_SLOT_MS / 1000.0)
            .round()
            .clamp(16.0, 2048.0) as usize;

        let mut block_peak = 0.0f32;
        let mut block_sq = 0.0f32;
        for &s in samples {
            let a = s.abs();
            block_peak = block_peak.max(a);
            block_sq += s * s;
            self.carry.push(s);
            if self.carry.len() >= self.samples_per_slot {
                let slot_env = self
                    .carry
                    .iter()
                    .map(|x| x.abs())
                    .fold(0.0f32, f32::max);
                self.buf[self.write] = slot_env;
                self.write = (self.write + 1) % self.buf.len();
                self.carry.clear();
            }
        }
        let n = samples.len() as f32;
        let block_rms = (block_sq / n).sqrt();
        self.peak = self.peak * 0.88 + block_peak * 0.12;
        self.rms = self.rms * 0.88 + block_rms * 0.12;
    }

    pub(super) fn ordered(&self) -> Vec<f32> {
        let len = self.buf.len();
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(self.buf[(self.write + i) % len]);
        }
        out
    }
}

pub(super) struct AudioWaveformRing {
    buf: Vec<f32>,
    write: usize,
}

impl AudioWaveformRing {
    pub(super) fn new() -> Self {
        Self {
            buf: vec![0.0; WAVEFORM_LEN],
            write: 0,
        }
    }

    pub(super) fn push_block(&mut self, samples: &[f32]) {
        for &s in samples {
            self.buf[self.write] = s;
            self.write = (self.write + 1) % self.buf.len();
        }
    }

    pub(super) fn ordered(&self) -> Vec<f32> {
        let len = self.buf.len();
        let mut out = Vec::with_capacity(len);
        for i in 0..len {
            out.push(self.buf[(self.write + i) % len]);
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn waveform_ring_preserves_order() {
        let mut ring = AudioWaveformRing::new();
        let samples: Vec<f32> = (0..100).map(|i| i as f32 * 0.01).collect();
        ring.push_block(&samples);
        let out = ring.ordered();
        assert_eq!(out.len(), WAVEFORM_LEN);
        assert!((out[WAVEFORM_LEN - 100] - 0.0).abs() < 1e-6);
        assert!((out[WAVEFORM_LEN - 1] - 0.99).abs() < 1e-6);
    }
}
