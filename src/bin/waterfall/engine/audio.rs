//! AF scope ring buffer (engine thread only).

/// Samples retained for the AF scope strip (matches [`crate::meters::SCOPE_LEN`]).
pub(crate) const SCOPE_LEN: usize = 320;

pub(super) struct AudioScopeRing {
    buf: Vec<f32>,
    write: usize,
    pub(super) peak: f32,
    pub(super) rms: f32,
}

impl AudioScopeRing {
    pub(super) fn new() -> Self {
        Self {
            buf: vec![0.0; SCOPE_LEN],
            write: 0,
            peak: 0.0,
            rms: 0.0,
        }
    }

    pub(super) fn push_block(&mut self, samples: &[f32]) {
        if samples.is_empty() {
            return;
        }
        let stride = (samples.len() / 48).max(1);
        let mut block_peak = 0.0f32;
        let mut block_sq = 0.0f32;
        let mut n = 0u32;
        for &s in samples.iter().step_by(stride) {
            self.buf[self.write] = s;
            self.write = (self.write + 1) % self.buf.len();
            let a = s.abs();
            block_peak = block_peak.max(a);
            block_sq += a * a;
            n += 1;
        }
        if n > 0 {
            let block_rms = (block_sq / n as f32).sqrt();
            self.peak = self.peak * 0.9 + block_peak * 0.1;
            self.rms = self.rms * 0.9 + block_rms * 0.1;
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
