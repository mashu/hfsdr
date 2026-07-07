//! CW squelch with hang — mutes audio between transmissions without chopping dits.

/// Hang-time squelch for demodulated CW audio.
#[derive(Clone, Debug)]
pub struct CwSquelch {
    envelope: f32,
    open: bool,
    hang_left: u32,
}

impl Default for CwSquelch {
    fn default() -> Self {
        Self::new()
    }
}

impl CwSquelch {
    pub fn new() -> Self {
        Self {
            envelope: 0.0,
            open: false,
            hang_left: 0,
        }
    }

    pub fn reset_state(&mut self) {
        self.envelope = 0.0;
        self.open = false;
        self.hang_left = 0;
    }

    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Gate `sample` using `level` (typically |audio| or IQ magnitude).
    pub fn process(
        &mut self,
        sample: f32,
        level: f32,
        sample_rate: f32,
        open_threshold: f32,
        close_threshold: f32,
        hang_ms: f32,
    ) -> f32 {
        if sample_rate <= 0.0 {
            return sample;
        }
        let inst = level.abs().max(0.0);
        if inst > self.envelope {
            self.envelope += 0.25 * (inst - self.envelope);
        } else {
            self.envelope += 0.002 * (inst - self.envelope);
        }

        let open_thr = open_threshold.max(1e-5);
        let close_thr = close_threshold.min(open_thr).max(1e-6);
        let hang_samples = (sample_rate * hang_ms / 1000.0).round().max(1.0) as u32;

        if self.envelope > open_thr {
            self.open = true;
            self.hang_left = hang_samples;
        } else if self.envelope < close_thr {
            if self.hang_left > 0 {
                self.hang_left -= 1;
            } else {
                self.open = false;
            }
        }

        let target = if self.open { 1.0 } else { 0.0 };
        let gate = if target > 0.5 { 1.0 } else { 0.0 };
        sample * gate
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mutes_quiet_audio() {
        let mut sq = CwSquelch::new();
        let rate = 12_000.0;
        for _ in 0..rate as usize {
            let _ = sq.process(0.0, 0.0, rate, 0.02, 0.01, 50.0);
        }
        let out = sq.process(0.5, 0.001, rate, 0.02, 0.01, 50.0);
        assert!(out.abs() < 1e-3);
    }

    #[test]
    fn opens_on_signal_and_hangs() {
        let mut sq = CwSquelch::new();
        let rate = 12_000.0;
        for _ in 0..200 {
            let _ = sq.process(0.4, 0.3, rate, 0.02, 0.01, 80.0);
        }
        assert!(sq.is_open());
        for _ in 0..100 {
            let _ = sq.process(0.0, 0.005, rate, 0.02, 0.01, 80.0);
        }
        assert!(sq.is_open(), "hang should keep squelch open");
    }
}
