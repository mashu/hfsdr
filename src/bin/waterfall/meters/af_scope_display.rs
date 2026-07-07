//! Smoothed AF scope envelope columns (UI thread).

use super::af_scope_state::AfScopeAccuracy;

/// UI-side envelope history with momentum.
#[derive(Clone, Debug, Default)]
pub struct AfScopeDisplayState {
    smoothed: Vec<f32>,
    cols: usize,
}

/// Map ring-buffer samples into fixed display columns (peak envelope per bin).
pub fn envelope_columns(samples: &[f32], cols: usize) -> Vec<f32> {
    if cols == 0 {
        return Vec::new();
    }
    if samples.is_empty() {
        return vec![0.0; cols];
    }
    let mut out = vec![0.0f32; cols];
    let n = samples.len();
    for (col, cell) in out.iter_mut().enumerate().take(cols) {
        let i0 = col * n / cols;
        let i1 = ((col + 1) * n / cols).max(i0 + 1).min(n);
        let mut peak = 0.0f32;
        for &s in &samples[i0..i1] {
            peak = peak.max(s.abs());
        }
        *cell = peak;
    }
    out
}

fn spatial_smooth3(buf: &[f32], center_w: f32) -> Vec<f32> {
    let n = buf.len();
    if n < 3 {
        return buf.to_vec();
    }
    let side = (1.0 - center_w) * 0.5;
    let mut out = buf.to_vec();
    for i in 1..n - 1 {
        out[i] = buf[i - 1] * side + buf[i] * center_w + buf[i + 1] * side;
    }
    out
}

impl AfScopeDisplayState {
    pub fn tick(
        &mut self,
        dt: f32,
        samples: &[f32],
        live: bool,
        accuracy: AfScopeAccuracy,
    ) -> &[f32] {
        let cols = accuracy.display_cols();
        if self.smoothed.len() != cols || self.cols != cols {
            self.smoothed.resize(cols, 0.0);
            self.cols = cols;
        }
        let dt = dt.clamp(0.0, 0.1);
        if !live {
            let decay = (-dt * 3.5).exp();
            for v in &mut self.smoothed {
                *v *= decay;
            }
            return &self.smoothed;
        }

        let targets = envelope_columns(samples, cols);
        let tau = accuracy.smooth_tau_s();
        let alpha = if dt > 0.0 {
            1.0 - (-dt / tau).exp()
        } else {
            0.0
        };
        for (s, &t) in self.smoothed.iter_mut().zip(targets.iter()) {
            *s += (t - *s) * alpha;
        }
        let blended = spatial_smooth3(&self.smoothed, accuracy.spatial_center_weight());
        self.smoothed.copy_from_slice(&blended);
        &self.smoothed
    }

    pub fn envelope(&self) -> &[f32] {
        &self.smoothed
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn envelope_columns_picks_peak_in_bin() {
        let samples = vec![0.0, 0.2, 0.8, 0.1];
        let cols = envelope_columns(&samples, 2);
        assert!((cols[0] - 0.2).abs() < 1e-5);
        assert!((cols[1] - 0.8).abs() < 1e-5);
    }

    #[test]
    fn tick_smooths_toward_target() {
        let mut state = AfScopeDisplayState::default();
        let samples = vec![0.5; 32];
        for _ in 0..30 {
            state.tick(1.0 / 60.0, &samples, true, AfScopeAccuracy::Coarse);
        }
        assert!(state.smoothed[0] > 0.35);
        assert!(state.smoothed[0] < 0.55);
    }

    #[test]
    fn fine_accuracy_uses_more_columns() {
        let mut state = AfScopeDisplayState::default();
        state.tick(1.0 / 60.0, &[0.5; 64], true, AfScopeAccuracy::Fine);
        assert_eq!(state.envelope().len(), 128);
    }
}
