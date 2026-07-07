//! AF scope display mode, accuracy, and waveform/phosphor state.

/// Envelope bars for RF gain overview.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AfScopeMode {
    #[default]
    Level,
    Waveform,
}

/// Time/detail preset — applies to both level and waveform views.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AfScopeAccuracy {
    #[default]
    Coarse,
    Medium,
    Fine,
}

impl AfScopeMode {
    /// Compact overlay chip label.
    pub fn chip(self) -> &'static str {
        match self {
            Self::Level => "ENV",
            Self::Waveform => "WAV",
        }
    }

    pub fn toggle(self) -> Self {
        match self {
            Self::Level => Self::Waveform,
            Self::Waveform => Self::Level,
        }
    }
}

impl AfScopeAccuracy {
    pub fn chip(self) -> &'static str {
        match self {
            Self::Coarse => "C",
            Self::Medium => "M",
            Self::Fine => "F",
        }
    }

    pub fn next(self) -> Self {
        match self {
            Self::Coarse => Self::Medium,
            Self::Medium => Self::Fine,
            Self::Fine => Self::Coarse,
        }
    }

    /// Display columns for the level envelope strip.
    pub fn display_cols(self) -> usize {
        match self {
            Self::Coarse => 64,
            Self::Medium => 96,
            Self::Fine => 128,
        }
    }

    /// UI temporal smoothing time constant (seconds).
    pub fn smooth_tau_s(self) -> f32 {
        match self {
            Self::Coarse => 0.14,
            Self::Medium => 0.08,
            Self::Fine => 0.04,
        }
    }

    /// Spatial blend weight for centre column (level mode).
    pub fn spatial_center_weight(self) -> f32 {
        match self {
            Self::Coarse => 0.56,
            Self::Medium => 0.68,
            Self::Fine => 0.82,
        }
    }

    /// How many raw samples to show in waveform mode.
    pub fn waveform_samples(self, buffer_len: usize) -> usize {
        let cap = match self {
            Self::Coarse => 1536,
            Self::Medium => 2560,
            Self::Fine => buffer_len,
        };
        cap.min(buffer_len.max(1))
    }
}

const PHOSPHOR_COLS: usize = 220;
const PHOSPHOR_ROWS: usize = 52;
const TRIGGER_THRESHOLD: f32 = 0.018;
const PHOSPHOR_DECAY: f32 = 0.90;

/// Waveform hold, phosphor accumulation, and auto-trigger.
#[derive(Clone, Debug)]
pub struct AfScopeViewState {
    pub mode: AfScopeMode,
    pub accuracy: AfScopeAccuracy,
    pub hold: bool,
    pub phosphor: bool,
    held_waveform: Vec<f32>,
    phosphor_buf: Vec<f32>,
    last_env: f32,
}

impl Default for AfScopeViewState {
    fn default() -> Self {
        Self {
            mode: AfScopeMode::Level,
            accuracy: AfScopeAccuracy::Coarse,
            hold: false,
            phosphor: false,
            held_waveform: Vec::new(),
            phosphor_buf: vec![0.0; PHOSPHOR_COLS * PHOSPHOR_ROWS],
            last_env: 0.0,
        }
    }
}

impl AfScopeViewState {
    pub fn set_hold(&mut self, hold: bool, live: &[f32]) {
        self.hold = hold;
        if hold {
            self.held_waveform = live.to_vec();
        } else {
            self.held_waveform.clear();
        }
    }

    pub fn clear_phosphor(&mut self) {
        self.phosphor_buf.fill(0.0);
    }

    pub fn waveform_for_display<'a>(&'a self, live: &'a [f32]) -> &'a [f32] {
        if self.hold && !self.held_waveform.is_empty() {
            &self.held_waveform
        } else {
            live
        }
    }

    pub fn phosphor_grid(&self) -> (&[f32], usize, usize) {
        (&self.phosphor_buf, PHOSPHOR_COLS, PHOSPHOR_ROWS)
    }

    /// Advance phosphor/trigger state; returns whether a new sweep was triggered.
    pub fn tick_waveform(&mut self, live: &[f32], dt: f32, live_streaming: bool) -> bool {
        if !live_streaming {
            self.last_env = 0.0;
            return false;
        }
        let window = self.accuracy.waveform_samples(live.len());
        let start = live.len().saturating_sub(window);
        let slice = &live[start..];

        if self.hold {
            return false;
        }

        let env = short_envelope(slice);
        let triggered = env > TRIGGER_THRESHOLD && self.last_env <= TRIGGER_THRESHOLD;
        self.last_env = env;

        if !self.phosphor {
            return triggered;
        }

        let decay = PHOSPHOR_DECAY.powf((dt * 60.0).clamp(0.0, 3.0));
        for cell in &mut self.phosphor_buf {
            *cell *= decay;
        }

        if self.phosphor && !triggered {
            return false;
        }

        accumulate_trace(&mut self.phosphor_buf, slice, PHOSPHOR_COLS, PHOSPHOR_ROWS);
        triggered
    }
}

fn short_envelope(samples: &[f32]) -> f32 {
    if samples.is_empty() {
        return 0.0;
    }
    let tail = samples.len().min(256).max(samples.len() / 8);
    let start = samples.len().saturating_sub(tail);
    let mut peak = 0.0f32;
    for &s in &samples[start..] {
        peak = peak.max(s.abs());
    }
    peak
}

fn accumulate_trace(buf: &mut [f32], samples: &[f32], cols: usize, rows: usize) {
    if samples.is_empty() || cols == 0 || rows == 0 {
        return;
    }
    let n = samples.len();
    let half_rows = (rows / 2) as f32;
    for (col, chunk) in samples.chunks((n + cols - 1) / cols).enumerate().take(cols) {
        let mut peak = 0.0f32;
        for &s in chunk {
            peak = peak.max(s.abs());
        }
        let env_row = ((peak.clamp(0.0, 1.15) * half_rows).round() as usize).min(rows / 2);
        let mid = rows / 2;
        for row in (mid.saturating_sub(env_row))..=(mid + env_row).min(rows - 1) {
            let idx = row * cols + col;
            if let Some(cell) = buf.get_mut(idx) {
                *cell = (*cell + 1.0).min(12.0);
            }
        }
        for &s in chunk {
            let y = mid as f32 - (s.clamp(-1.15, 1.15) * half_rows);
            let row = y.round().clamp(0.0, (rows - 1) as f32) as usize;
            let idx = row * cols + col;
            if let Some(cell) = buf.get_mut(idx) {
                *cell = (*cell + 0.85).min(12.0);
            }
        }
    }
}

pub fn waveform_envelope_overlay(samples: &[f32], out_cols: usize) -> Vec<f32> {
    if out_cols == 0 || samples.is_empty() {
        return vec![0.0; out_cols.max(1)];
    }
    let mut out = vec![0.0f32; out_cols];
    for (col, cell) in out.iter_mut().enumerate() {
        let i0 = col * samples.len() / out_cols;
        let i1 = ((col + 1) * samples.len() / out_cols).max(i0 + 1).min(samples.len());
        let mut peak = 0.0f32;
        for &s in &samples[i0..i1] {
            peak = peak.max(s.abs());
        }
        *cell = peak;
    }
    out
}

pub fn downsample_waveform(samples: &[f32], out_cols: usize) -> Vec<f32> {
    if out_cols == 0 || samples.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(out_cols);
    for col in 0..out_cols {
        let i0 = col * samples.len() / out_cols;
        let i1 = ((col + 1) * samples.len() / out_cols).max(i0 + 1).min(samples.len());
        let mut min_v = f32::INFINITY;
        let mut max_v = f32::NEG_INFINITY;
        for &s in &samples[i0..i1] {
            min_v = min_v.min(s);
            max_v = max_v.max(s);
        }
        if min_v.is_finite() && max_v.is_finite() {
            out.push(if col % 2 == 0 { min_v } else { max_v });
        } else {
            out.push(0.0);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accuracy_cols_increase_with_fine() {
        assert!(AfScopeAccuracy::Fine.display_cols() > AfScopeAccuracy::Coarse.display_cols());
    }

    #[test]
    fn phosphor_accumulates_on_trigger() {
        let mut state = AfScopeViewState {
            phosphor: true,
            ..Default::default()
        };
        let mut samples = vec![0.0f32; 512];
        for i in 0..128 {
            samples[384 + i] = (i as f32 * 0.08).sin() * 0.5;
        }
        state.last_env = 0.0;
        let triggered = state.tick_waveform(&samples, 1.0 / 60.0, true);
        let sum: f32 = state.phosphor_buf.iter().sum();
        assert!(triggered || sum > 0.0);
    }

    #[test]
    fn hold_captures_waveform() {
        let mut state = AfScopeViewState::default();
        let live = vec![0.1, 0.2, 0.3];
        state.set_hold(true, &live);
        assert!(state.hold);
        assert_eq!(state.waveform_for_display(&[0.0]), &[0.1, 0.2, 0.3]);
    }
}
