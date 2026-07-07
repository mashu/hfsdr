//! Adaptive keying-speed (WPM) estimator for the listen channel.
//!
//! Tracks mark durations on the filtered IQ envelope so matched-filter demod
//! and BW hints follow the operator's actual fist — no manual WPM entry.

use super::filter_plan::{dit_duration_s, passband_hz_for_wpm};

/// PARIS dot length → WPM.
pub fn wpm_from_dot_seconds(dot_seconds: f32) -> f32 {
    if dot_seconds <= 0.0 {
        return 0.0;
    }
    1.2 / dot_seconds
}

/// Lightweight adaptive WPM tracker (same dot-length law as the skimmer decoder).
#[derive(Clone, Debug)]
pub struct KeyerSpeedEstimator {
    sample_rate: f32,
    dot_samples: f32,
    keyed: bool,
    run: u32,
    env: f32,
    peak: f32,
    noise: f32,
    confident: bool,
}

impl Default for KeyerSpeedEstimator {
    fn default() -> Self {
        Self::new(12_000.0)
    }
}

impl KeyerSpeedEstimator {
    pub fn new(sample_rate: f32) -> Self {
        let rate = sample_rate.max(1.0);
        Self {
            sample_rate: rate,
            dot_samples: default_dot_samples(rate),
            keyed: false,
            run: 0,
            env: 0.0,
            peak: 0.0,
            noise: 0.0,
            confident: false,
        }
    }

    pub fn reset_state(&mut self) {
        let rate = self.sample_rate;
        *self = Self::new(rate);
    }

    pub fn sync_rate(&mut self, sample_rate: f32) {
        let rate = sample_rate.max(1.0);
        if (rate - self.sample_rate).abs() > 1.0 {
            self.sample_rate = rate;
            self.dot_samples = self.clamp_dot(self.dot_samples);
        }
    }

    /// Feed one IQ magnitude sample (post channel filter).
    pub fn feed(&mut self, level: f32, sample_rate: f32) {
        self.sync_rate(sample_rate);
        let inst = level.max(0.0);
        if inst > self.env {
            self.env += 0.08 * (inst - self.env);
        } else {
            self.env += 0.004 * (inst - self.env);
        }
        if self.env > self.peak {
            self.peak = self.env;
        } else {
            self.peak *= 0.9999;
        }
        if self.env < self.noise {
            self.noise += 0.03 * (self.env - self.noise);
        } else {
            self.noise += 0.0003 * (self.env - self.noise);
        }

        let span = (self.peak - self.noise).max(0.0);
        let thr_on = self.noise + 0.12 * span + 0.002;
        let thr_off = self.noise + 0.06 * span + 0.001;
        let want_keyed = if self.keyed {
            self.env > thr_off
        } else {
            self.env > thr_on
        };

        if want_keyed == self.keyed {
            self.run = self.run.saturating_add(1);
            return;
        }

        if self.keyed {
            self.end_mark();
        }
        self.keyed = want_keyed;
        self.run = 1;
    }

    fn end_mark(&mut self) {
        let run = self.run as f32;
        if run < 0.35 * self.dot_samples {
            return;
        }
        if run < 2.2 * self.dot_samples {
            self.dot_samples = self.clamp_dot(0.85 * self.dot_samples + 0.15 * run);
            self.confident = true;
        } else if run < 6.0 * self.dot_samples {
            self.dot_samples = self.clamp_dot(0.85 * self.dot_samples + 0.15 * (run / 3.0));
            self.confident = true;
        }
    }

    fn clamp_dot(&self, dot: f32) -> f32 {
        dot.clamp(0.02 * self.sample_rate, 0.20 * self.sample_rate)
    }

    /// Estimated speed in WPM (default ~20 until keying is observed).
    pub fn wpm(&self) -> f32 {
        let wpm = wpm_from_dot_seconds(self.dot_samples / self.sample_rate);
        wpm.clamp(5.0, 60.0)
    }

    pub fn confident(&self) -> bool {
        self.confident
    }

    pub fn suggested_passband_hz(&self) -> f32 {
        passband_hz_for_wpm(self.wpm())
    }

    pub fn dit_samples(&self, sample_rate: f32) -> usize {
        let rate = sample_rate.max(1.0);
        (self.dot_samples * rate / self.sample_rate)
            .round()
            .clamp(1.0, 0.25 * rate) as usize
    }
}

fn default_dot_samples(sample_rate: f32) -> f32 {
    sample_rate * dit_duration_s(20.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn feed_keyed(est: &mut KeyerSpeedEstimator, rate: f32, wpm: f32, text: &str) {
        let dot = (dit_duration_s(wpm) * rate) as u32;
        let dash = dot * 3;
        let igap = dot;
        let cgap = dot * 3;
        let wgap = dot * 7;
        let push = |est: &mut KeyerSpeedEstimator, on: bool, len: u32| {
            let lvl = if on { 0.25 } else { 0.002 };
            for _ in 0..len {
                est.feed(lvl, rate);
            }
        };
        push(est, false, dot * 8);
        for (wi, word) in text.split(' ').enumerate() {
            if wi > 0 {
                push(est, false, wgap);
            }
            for (ci, ch) in word.chars().enumerate() {
                if ci > 0 {
                    push(est, false, cgap);
                }
                for el in morse_for_char(ch).chars() {
                    if el == '-' {
                        push(est, true, dash);
                    } else {
                        push(est, true, dot);
                    }
                    push(est, false, igap);
                }
            }
        }
        push(est, false, dot * 10);
    }

    fn morse_for_char(ch: char) -> &'static str {
        match ch {
            'C' => "-.-.",
            'Q' => "--.-",
            'P' => ".--.",
            'A' => ".-",
            'R' => ".-.",
            'I' => "..",
            'S' => "...",
            'T' => "-",
            'E' => ".",
            _ => ".",
        }
    }

    #[test]
    fn tracks_paris_speed() {
        let rate = 12_000.0;
        let mut est = KeyerSpeedEstimator::new(rate);
        feed_keyed(&mut est, rate, 25.0, "PARIS PARIS");
        let wpm = est.wpm();
        assert!(est.confident());
        assert!((wpm - 25.0).abs() < 8.0, "wpm={wpm}");
    }

    #[test]
    fn passband_tracks_wpm() {
        let est = KeyerSpeedEstimator::new(12_000.0);
        let bw = est.suggested_passband_hz();
        assert!((bw - 80.0).abs() < 20.0);
    }
}
