//! Bayesian CW decoder: HMM key detection, online-EM timing, posterior beams.
//!
//! Every hard threshold in the classic chain has a probabilistic counterpart
//! here, so the decoder adapts itself to band conditions instead of relying
//! on tuned rules:
//!
//! * **Key detection** — a two-state hidden Markov model over the
//!   log-envelope. Emission means (noise level, mark level) are re-estimated
//!   online from the state posterior (EM-style responsibility weighting) and
//!   transition hazards come from the current dit period, so evidence
//!   integrates over time: weak keying is detected by accumulation rather
//!   than a fixed threshold crossing, and QSB dropouts are bridged by the
//!   transition prior.
//! * **Element timing** — mark durations are a log-normal mixture (dit, dah,
//!   outlier); gaps are a log-normal mixture (element, char, word, outlier).
//!   Classification is the component posterior; the dit period and gap
//!   centres adapt by responsibility-weighted updates, so flutter fragments
//!   fall into the outlier component instead of poisoning the clock. Speed
//!   jumps and cold starts are handled by periodic likelihood-based model
//!   selection over candidate dit periods.
//! * **Text** — beam search keeps both readings of an ambiguous mark alive,
//!   weighted by the mixture posterior, and scores hypotheses with the
//!   callsign-oriented bigram prior shared with [`super::bigram`].
//!
//! Cost per envelope sample is a handful of multiplies (two-state forward
//! update); per keying event it is a few mixture likelihoods and beam
//! bookkeeping — cheaper than the channel FIR that precedes it.

use std::collections::VecDeque;

use super::bigram::bigram_log;
use super::config::DecoderParams;
use super::decoder::{wpm_from_dot_seconds, CwDecoder};
use super::morse::decode_elements;
use super::timing::{KeyEvent, Keyer, SpaceKind};

/// Longest real Morse pattern in the table is 6 elements.
const MAX_ELEMENTS: usize = 7;
/// Collapse even without a word gap once the best text grows this long.
const MAX_PENDING_CHARS: usize = 24;

// --- Envelope statistics (log domain) ---

/// Pre-smoothing of the rectified envelope before taking logs.
const ENV_ATTACK_S: f32 = 0.002;
const ENV_RELEASE_S: f32 = 0.004;
const LOG_FLOOR: f32 = 1e-9;
/// Key-down log-envelope spread around the mark level.
const SIGMA_ON: f32 = 0.40;
/// Key-up emission is asymmetric: broad above the noise mean (bandlimited
/// keying leaves shallow gap dips well above the floor at high WPM), narrow
/// below it. This puts the on/off decision boundary ~1 neper below the mark
/// level instead of midway down to the noise.
const SIGMA_OFF_UP: f32 = 1.10;
const SIGMA_OFF_DN: f32 = 0.50;
/// ln(2 / ((σ_up + σ_dn) √2π)) — piecewise-Gaussian normalisation.
const LN_NORM_OFF: f32 = -0.696;
/// Emission z-score clamp — one impulse cannot swamp the state posterior.
const Z_CLAMP: f32 = 3.5;
/// Minimum mark/noise separation in nepers (~3 dB) before keying is believed.
const MIN_SEP: f32 = 0.35;
/// Idle mark level decays toward this separation so a weaker sender that
/// appears later on the channel is still detected.
const LEAK_SEP: f32 = 1.5;
/// Log-envelope dynamic range below the tracked peak (~35 dB). Keeps both
/// emission means within a span the clamped z-scores can discriminate — a
/// wider range leaves a dead zone between the rails where the likelihood
/// ratio carries no OFF evidence and short inter-element gaps are lost.
const RANGE_NEPERS: f32 = 4.0;
/// An observation this far above the mark level is outside the model —
/// re-prime quickly instead of waiting for the one-pole attack.
const SURPRISE_NEPERS: f32 = 1.5;
const SURPRISE_GAIN: f32 = 0.2;
/// Level-tracker time constants (seconds).
const OFF_FALL_S: f32 = 0.060;
const OFF_RISE_S: f32 = 0.700;
const ON_RISE_S: f32 = 0.050;
const ON_FALL_S: f32 = 0.900;
const LEAK_S: f32 = 6.0;
/// Faster convergence right after construction.
const WARMUP_S: f32 = 0.30;
const WARMUP_BOOST: f32 = 8.0;
/// MAP key decision hysteresis on the smoothed posterior.
const KEY_ON_P: f32 = 0.60;
const KEY_OFF_P: f32 = 0.40;
/// Fixed-lag smoothing window (seconds): key decisions see this much future
/// evidence, so a brief noise dip inside a dah is outvoted by the samples
/// around it instead of splitting the mark.
const SMOOTH_LAG_S: f32 = 0.024;
const SMOOTH_LAG_MAX: usize = 192;
/// Expected run lengths (in dits) behind the transition hazards.
const MEAN_MARK_DITS: f32 = 2.0;
const MEAN_GAP_DITS: f32 = 2.5;

// --- Duration mixtures ---

/// Log-normal spread of mark durations around dit / dah.
const SIGMA_MARK: f32 = 0.33;
/// Mixture priors for marks: dit, dah, outlier.
const W_DIT: f32 = 0.45;
const W_DAH: f32 = 0.45;
const W_MARK_OUT: f32 = 0.10;
/// Constant outlier log-density over log-duration (broad, uninformative).
const LN_OUT_DENS: f32 = -3.0;
/// Gap mixture spreads (element, char, word) and priors.
const SIGMA_GAPS: [f32; 3] = [0.35, 0.42, 0.55];
const W_GAPS: [f32; 3] = [0.50, 0.30, 0.15];
const W_GAP_OUT: f32 = 0.05;
/// EM learning rates (log domain).
const ETA_DIT: f32 = 0.12;
const ETA_GAP: f32 = 0.15;
/// 8 WPM / 60 WPM dit bounds, matching [`super::timing`].
const MAX_DOT_S: f32 = 0.15;
const MIN_DOT_S: f32 = 0.02;
/// Speed model selection: window, cadence and adoption margin (nats). The
/// margin protects a well-fitting incumbent; when the incumbent explains the
/// data poorly (low evidence), any better candidate wins immediately.
const RECENT_MARKS: usize = 16;
const RESELECT_EVERY: u32 = 6;
const SELECT_MARGIN: f32 = 2.5;
const SELECT_MARGIN_LOST: f32 = 0.5;
/// Per-event evidence (log Bayes factor vs a flat null) EMA and gate.
const LN_NULL: f32 = -1.5;
const EVIDENCE_ALPHA: f32 = 0.25;
const EVIDENCE_CLAMP: f32 = 1.5;
const CONFIDENT_EVIDENCE: f32 = 0.10;
const CONFIDENT_MIN_EVENTS: u32 = 4;
/// Spawn both mark readings when the weaker posterior exceeds exp(-2.3) ≈ 10%.
const BRANCH_MIN_LOG_POST: f32 = -2.3;

/// Dot period handed to the debouncing [`Keyer`]. The HMM's transition prior
/// already suppresses noise flips, so the keyer bridge stays short and nearly
/// absolute — a dit-proportional bridge under a still-wrong (too slow) clock
/// would swallow the very gaps that let the clock correct itself.
fn keyer_dot_s(dit_s: f32) -> f32 {
    dit_s.min(0.025)
}

/// One-pole coefficient for a time constant at a sample rate.
fn alpha(tau_s: f32, sample_rate: f32) -> f32 {
    if tau_s <= 0.0 || sample_rate <= 0.0 {
        return 1.0;
    }
    1.0 - (-1.0 / (tau_s * sample_rate)).exp()
}

/// Adaptive log-domain level model: noise (key-up) and mark (key-down) means
/// re-estimated from the HMM posterior — the EM half of the detector.
#[derive(Clone, Debug)]
struct LevelModel {
    env: f32,
    mu_off: f32,
    mu_on: f32,
    /// Slowly decaying log-envelope peak — anchors the dynamic-range floor.
    peak: f32,
    primed: bool,
    warmup_left: u32,
    a_env_up: f32,
    a_env_dn: f32,
    a_off_fall: f32,
    a_off_rise: f32,
    a_on_rise: f32,
    a_on_fall: f32,
    a_leak: f32,
    a_peak: f32,
}

impl LevelModel {
    fn new(sample_rate: f32) -> Self {
        let sample_rate = sample_rate.max(1.0);
        Self {
            env: 0.0,
            mu_off: 0.0,
            mu_on: 0.0,
            peak: 0.0,
            primed: false,
            warmup_left: (WARMUP_S * sample_rate) as u32,
            a_env_up: alpha(ENV_ATTACK_S, sample_rate),
            a_env_dn: alpha(ENV_RELEASE_S, sample_rate),
            a_off_fall: alpha(OFF_FALL_S, sample_rate),
            a_off_rise: alpha(OFF_RISE_S, sample_rate),
            a_on_rise: alpha(ON_RISE_S, sample_rate),
            a_on_fall: alpha(ON_FALL_S, sample_rate),
            a_leak: alpha(LEAK_S, sample_rate),
            a_peak: alpha(5.0, sample_rate),
        }
    }

    /// Smooth the rectified input and return its range-limited log-envelope.
    fn log_env(&mut self, x: f32) -> f32 {
        let inst = x.abs();
        if !self.primed {
            self.primed = true;
            self.env = inst;
            let l = (inst + LOG_FLOOR).ln();
            self.peak = l;
            self.mu_off = l;
            self.mu_on = l + LEAK_SEP;
        }
        let a = if inst > self.env { self.a_env_up } else { self.a_env_dn };
        self.env += a * (inst - self.env);
        let l = (self.env + LOG_FLOOR).ln();
        if l > self.peak {
            self.peak = l;
        } else {
            self.peak += self.a_peak * (l - self.peak);
        }
        l.max(self.peak - RANGE_NEPERS)
    }

    /// Responsibility-weighted level update from the filtered posterior.
    fn update(&mut self, l: f32, p_on: f32) {
        let boost = if self.warmup_left > 0 {
            self.warmup_left -= 1;
            WARMUP_BOOST
        } else {
            1.0
        };
        let a_off = if l < self.mu_off { self.a_off_fall } else { self.a_off_rise };
        self.mu_off += (a_off * boost * (1.0 - p_on)).min(1.0) * (l - self.mu_off);
        if l > self.mu_on + SURPRISE_NEPERS {
            // Far outside the model — a (re)appearing or much stronger
            // sender. Re-prime instead of waiting for the one-pole attack.
            self.mu_on += SURPRISE_GAIN * (l - self.mu_on);
        } else if l > self.mu_on {
            // Rising above the mark estimate is informative regardless of the
            // current posterior — noise rarely exceeds the mark level.
            self.mu_on += (self.a_on_rise * boost).min(1.0) * (l - self.mu_on);
        } else {
            self.mu_on += (self.a_on_fall * boost * p_on).min(1.0) * (l - self.mu_on);
        }
        // The noise mean cannot sit below the observable floor, or gap
        // samples rail both emission z-scores and discrimination collapses.
        self.mu_off = self.mu_off.max(self.peak - RANGE_NEPERS);
        // Idle decay toward a nominal separation above the noise.
        let target = self.mu_off + LEAK_SEP;
        if self.mu_on > target {
            self.mu_on += self.a_leak * (target - self.mu_on);
        }
        if self.mu_on < self.mu_off + 0.05 {
            self.mu_on = self.mu_off + 0.05;
        }
    }

    fn separation(&self) -> f32 {
        self.mu_on - self.mu_off
    }
}

/// Two-state (key-up / key-down) fixed-lag smoother over the log-envelope:
/// a forward filter plus a short backward pass, so each key decision is the
/// posterior given ~[`SMOOTH_LAG_S`] of future evidence.
#[derive(Clone, Debug)]
struct KeyHmm {
    p_on: f32,
    h_up: f32,
    h_dn: f32,
    /// Per-sample (p(obs|on), p(obs|off), filtered posterior), oldest first.
    ring: VecDeque<(f32, f32, f32)>,
    lag: usize,
}

impl KeyHmm {
    fn new(sample_rate: f32) -> Self {
        let lag = ((SMOOTH_LAG_S * sample_rate) as usize).clamp(2, SMOOTH_LAG_MAX);
        Self {
            p_on: 0.1,
            h_up: 0.01,
            h_dn: 0.01,
            ring: VecDeque::with_capacity(lag + 1),
            lag,
        }
    }

    /// Transition hazards from the dit period: runs last a few dits.
    fn set_hazards(&mut self, dit_s: f32, sample_rate: f32) {
        let dit_n = (dit_s * sample_rate).max(1.0);
        self.h_up = (1.0 / (MEAN_GAP_DITS * dit_n)).clamp(1e-5, 0.2);
        self.h_dn = (1.0 / (MEAN_MARK_DITS * dit_n)).clamp(1e-5, 0.2);
    }

    /// One step; returns (filtered posterior now, smoothed posterior `lag`
    /// samples ago). The smoothed value drives keying; the filtered one
    /// feeds the level EM.
    fn step(&mut self, l: f32, levels: &LevelModel) -> (f32, f32) {
        let pred = self.p_on * (1.0 - self.h_dn) + (1.0 - self.p_on) * self.h_up;
        let z_on = ((l - levels.mu_on) / SIGMA_ON).clamp(-Z_CLAMP, Z_CLAMP);
        let s_off = if l >= levels.mu_off { SIGMA_OFF_UP } else { SIGMA_OFF_DN };
        let z_off = ((l - levels.mu_off) / s_off).clamp(-Z_CLAMP, Z_CLAMP);
        let mut dll = 0.5 * (z_off * z_off - z_on * z_on) - LN_NORM_OFF;
        // Below the noise mean can never be key-down evidence.
        if l < levels.mu_off {
            dll = dll.min(0.0);
        }
        let dll = dll.clamp(-15.0, 15.0);
        let lr = dll.exp();
        let num = pred * lr;
        self.p_on = (num / (num + (1.0 - pred))).clamp(1e-6, 1.0 - 1e-6);

        // Pairwise-normalised emission likelihoods keep the backward pass
        // well-conditioned without per-step rescaling.
        let e_on = lr / (1.0 + lr);
        if self.ring.len() > self.lag {
            self.ring.pop_front();
        }
        self.ring.push_back((e_on, 1.0 - e_on, self.p_on));

        // Backward pass over the window for the oldest sample.
        let (mut beta_on, mut beta_off) = (1.0f32, 1.0f32);
        for &(e_on, e_off, _) in self.ring.iter().skip(1).rev() {
            let b_on = (1.0 - self.h_dn) * e_on * beta_on + self.h_dn * e_off * beta_off;
            let b_off = self.h_up * e_on * beta_on + (1.0 - self.h_up) * e_off * beta_off;
            let norm = (b_on + b_off).max(1e-30);
            beta_on = b_on / norm;
            beta_off = b_off / norm;
        }
        let p0 = self.ring.front().map(|&(_, _, p)| p).unwrap_or(self.p_on);
        let num = p0 * beta_on;
        let smoothed = num / (num + (1.0 - p0) * beta_off).max(1e-30);
        (self.p_on, smoothed)
    }

}

/// Classification of one completed mark.
enum MarkClass {
    /// Neither dit nor dah — flutter fragment or stuck key.
    Outlier,
    /// Log posteriors of the two element readings (normalised over both).
    Element { w_dit: f32, w_dah: f32 },
}

/// Log-normal density (over log-duration) at `d` for a component mean.
fn ln_lognormal(d: f32, mean: f32, sigma: f32) -> f32 {
    let z = (d.max(1e-5) / mean.max(1e-5)).ln() / sigma;
    -0.5 * z * z - (sigma * 2.506_628_3).ln()
}

fn log_sum_exp(xs: &[f32]) -> f32 {
    let m = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
    m + xs.iter().map(|x| (x - m).exp()).sum::<f32>().ln()
}

/// Online-EM duration model: dit period, gap centres, coherence evidence and
/// likelihood-based speed re-selection.
#[derive(Clone, Debug)]
struct TimingModel {
    dit_s: f32,
    /// Gap centres in dit units (element, char, word).
    gaps: [f32; 3],
    /// Recent mark durations (all, including outliers) for model selection.
    recent: VecDeque<f32>,
    since_select: u32,
    /// EMA of per-mark log Bayes factor vs a flat null.
    evidence: f32,
    events: u32,
}

impl TimingModel {
    fn new(initial_wpm: f32) -> Self {
        Self {
            dit_s: (1.2 / initial_wpm.max(1.0)).clamp(MIN_DOT_S, MAX_DOT_S),
            gaps: [1.0, 3.0, 6.5],
            recent: VecDeque::with_capacity(RECENT_MARKS),
            since_select: 0,
            evidence: 0.0,
            events: 0,
        }
    }

    fn is_confident(&self) -> bool {
        self.events >= CONFIDENT_MIN_EVENTS && self.evidence > CONFIDENT_EVIDENCE
    }

    /// Element/char decision boundary in seconds.
    fn char_boundary_s(&self) -> f32 {
        (self.gaps[0] * self.gaps[1]).sqrt().clamp(1.3, 3.2) * self.dit_s
    }

    /// Char/word decision boundary in seconds.
    fn word_boundary_s(&self) -> f32 {
        (self.gaps[1] * self.gaps[2]).sqrt().clamp(3.6, 10.0) * self.dit_s
    }

    fn space_kind(&self, secs: f32) -> SpaceKind {
        if secs < self.char_boundary_s() {
            SpaceKind::Element
        } else if secs < self.word_boundary_s() {
            SpaceKind::Char
        } else {
            SpaceKind::Word
        }
    }

    /// Classify a completed mark and adapt the dit period.
    fn mark_event(&mut self, d: f32) -> MarkClass {
        if self.recent.len() == RECENT_MARKS {
            self.recent.pop_front();
        }
        self.recent.push_back(d);
        self.since_select += 1;
        if self.since_select >= RESELECT_EVERY {
            self.since_select = 0;
            self.reselect();
        }

        let s = [
            W_DIT.ln() + ln_lognormal(d, self.dit_s, SIGMA_MARK),
            W_DAH.ln() + ln_lognormal(d, 3.0 * self.dit_s, SIGMA_MARK),
            W_MARK_OUT.ln() + LN_OUT_DENS,
        ];
        let total = log_sum_exp(&s);
        let r_dit = (s[0] - total).exp();
        let r_dah = (s[1] - total).exp();
        let r_out = (s[2] - total).exp();

        self.events = self.events.saturating_add(1);
        let ev = (total - LN_NULL).clamp(-EVIDENCE_CLAMP, EVIDENCE_CLAMP);
        self.evidence += EVIDENCE_ALPHA * (ev - self.evidence);

        if r_out > 0.5 {
            return MarkClass::Outlier;
        }
        // Responsibility-weighted dit estimate from this event.
        let inlier = (r_dit + r_dah).max(1e-6);
        let est = d * (r_dit + r_dah / 3.0) / inlier;
        let eta = ETA_DIT * inlier;
        self.dit_s = (self.dit_s.ln() + eta * (est.max(1e-5).ln() - self.dit_s.ln()))
            .exp()
            .clamp(MIN_DOT_S, MAX_DOT_S);

        let norm = log_sum_exp(&s[..2]);
        MarkClass::Element {
            w_dit: s[0] - norm,
            w_dah: s[1] - norm,
        }
    }

    /// Adapt gap centres from a completed space.
    fn space_event(&mut self, d: f32) {
        let dits = d / self.dit_s.max(1e-5);
        if dits < 0.2 {
            return;
        }
        if dits > 25.0 {
            // Structureless idle gap. Real keying rarely pauses this long
            // mid-transmission; isolated noise bumps always do. Only the
            // outlier component explains it, so confidence decays.
            let ev = (W_GAP_OUT.ln() + LN_OUT_DENS - LN_NULL).clamp(-EVIDENCE_CLAMP, EVIDENCE_CLAMP);
            self.evidence += EVIDENCE_ALPHA * (ev - self.evidence);
            return;
        }
        let mut s = [0.0f32; 4];
        for (k, sk) in s.iter_mut().take(3).enumerate() {
            *sk = W_GAPS[k].ln() + ln_lognormal(dits, self.gaps[k], SIGMA_GAPS[k]);
        }
        s[3] = W_GAP_OUT.ln() + LN_OUT_DENS;
        let total = log_sum_exp(&s);
        // Gaps carry coherence evidence too: real Morse gaps cluster at
        // 1/3/7 dits of the same clock, noise gaps are unstructured.
        let ev = (total - LN_NULL).clamp(-EVIDENCE_CLAMP, EVIDENCE_CLAMP);
        self.evidence += EVIDENCE_ALPHA * (ev - self.evidence);
        for (gap, &sk) in self.gaps.iter_mut().zip(s.iter()) {
            let r = (sk - total).exp();
            *gap = (gap.ln() + ETA_GAP * r * (dits.ln() - gap.ln())).exp();
        }
        // Morse structure prior: gaps stay ordered element < char < word.
        self.gaps[0] = self.gaps[0].clamp(0.6, 2.2);
        self.gaps[1] = self.gaps[1].clamp((1.5 * self.gaps[0]).max(2.0), 7.0);
        self.gaps[2] = self.gaps[2].clamp(1.6 * self.gaps[1], 20.0);
    }

    /// Likelihood-based speed model selection over recent marks. Handles cold
    /// starts, sender changes and QLF without any reseed special cases: every
    /// candidate dit period is scored on the same mixture likelihood and the
    /// incumbent only loses to a clear margin.
    fn reselect(&mut self) {
        if self.recent.len() < 8 {
            return;
        }
        let mut sorted: Vec<f32> = self.recent.iter().copied().collect();
        sorted.sort_by(f32::total_cmp);
        let pct = |q: f32| sorted[((sorted.len() - 1) as f32 * q) as usize];
        let mut cands = vec![self.dit_s];
        // Short-mark percentiles matter most: when elements merge under a
        // too-slow clock, the few unmerged dits are the only clean evidence.
        for q in [0.08, 0.25, 0.5, 0.75] {
            let c = pct(q);
            cands.push(c.clamp(MIN_DOT_S, MAX_DOT_S));
            cands.push((c / 3.0).clamp(MIN_DOT_S, MAX_DOT_S));
        }
        let score = |dit: f32| -> f32 {
            self.recent
                .iter()
                .map(|&d| {
                    log_sum_exp(&[
                        W_DIT.ln() + ln_lognormal(d, dit, SIGMA_MARK),
                        W_DAH.ln() + ln_lognormal(d, 3.0 * dit, SIGMA_MARK),
                        W_MARK_OUT.ln() + LN_OUT_DENS,
                    ])
                })
                .sum()
        };
        let incumbent = score(self.dit_s);
        let mut best = (self.dit_s, incumbent);
        for &c in &cands[1..] {
            let sc = score(c);
            if sc > best.1 {
                best = (c, sc);
            }
        }
        let margin = if self.evidence < 0.2 { SELECT_MARGIN_LOST } else { SELECT_MARGIN };
        if best.1 > incumbent + margin {
            self.dit_s = best.0;
        }
    }
}

#[derive(Clone, Debug)]
struct Hypothesis {
    symbol: String,
    text: String,
    score: f32,
    poisoned: bool,
    last_char: Option<char>,
}

impl Hypothesis {
    fn new() -> Self {
        Self {
            symbol: String::new(),
            text: String::new(),
            score: 0.0,
            poisoned: false,
            last_char: None,
        }
    }

    fn push_element(&mut self, el: char, weight: f32) {
        if self.symbol.len() >= MAX_ELEMENTS {
            self.poisoned = true;
        } else {
            self.symbol.push(el);
        }
        self.score += weight;
    }

    fn discard_symbol(&mut self) {
        self.symbol.clear();
        self.poisoned = false;
    }

    fn flush_symbol(&mut self) {
        let ch = if self.poisoned {
            self.poisoned = false;
            self.symbol.clear();
            '?'
        } else if self.symbol.is_empty() {
            return;
        } else {
            let ch = decode_elements(&self.symbol).unwrap_or('?');
            self.symbol.clear();
            ch
        };
        if ch == '?' {
            self.score -= 1.5;
            if self.last_char == Some('?') {
                return;
            }
        }
        self.score += bigram_log(self.last_char, ch);
        self.text.push(ch);
        self.last_char = Some(ch);
    }
}

/// Statistical CW decoder: HMM key detection + EM timing + posterior beams.
#[derive(Clone, Debug)]
pub struct BayesCwDecoder {
    sample_rate: f32,
    params: DecoderParams,
    levels: LevelModel,
    hmm: KeyHmm,
    key_down: bool,
    keyer: Keyer,
    timing: TimingModel,
    beams: Vec<Hypothesis>,
    /// 0 = collecting a char, 1 = char flushed, 2 = word gap handled.
    space_flushed: u8,
}

impl Default for BayesCwDecoder {
    fn default() -> Self {
        Self::with_params(500.0, DecoderParams::default())
    }
}

impl BayesCwDecoder {
    pub fn new(sample_rate: f32) -> Self {
        Self::with_params(sample_rate, DecoderParams::default())
    }

    pub fn with_params(sample_rate: f32, params: DecoderParams) -> Self {
        let params = params.clamped();
        let sample_rate = sample_rate.max(1.0);
        let timing = TimingModel::new(params.initial_wpm);
        let mut keyer = Keyer::new(sample_rate);
        keyer.set_dot_seconds(keyer_dot_s(timing.dit_s));
        let mut hmm = KeyHmm::new(sample_rate);
        hmm.set_hazards(timing.dit_s, sample_rate);
        Self {
            sample_rate,
            levels: LevelModel::new(sample_rate),
            hmm,
            key_down: false,
            keyer,
            timing,
            beams: vec![Hypothesis::new()],
            space_flushed: 2,
            params,
        }
    }

    fn process_sample(&mut self, x: f32, out: &mut String) {
        let l = self.levels.log_env(x);
        let (p_filt, p_smooth) = self.hmm.step(l, &self.levels);
        self.levels.update(l, p_filt);

        // MAP key decision with hysteresis on the smoothed posterior; keying
        // is only believed while the level model shows genuine separation.
        self.key_down = if self.levels.separation() < MIN_SEP {
            false
        } else if self.key_down {
            p_smooth > KEY_OFF_P
        } else {
            p_smooth > KEY_ON_P
        };

        match self.keyer.step(self.key_down) {
            Some(KeyEvent::Mark(secs)) => self.on_mark(secs),
            Some(KeyEvent::Space(secs)) => self.timing.space_event(secs),
            None => {}
        }

        if self.keyer.in_space() && self.space_flushed < 2 {
            let kind = self.timing.space_kind(self.keyer.space_seconds());
            if kind >= SpaceKind::Char && self.space_flushed < 1 {
                let confident = self.timing.is_confident();
                for h in &mut self.beams {
                    if confident {
                        h.flush_symbol();
                    } else {
                        h.discard_symbol();
                    }
                }
                self.prune();
                self.space_flushed = 1;
                if self.best().text.len() >= MAX_PENDING_CHARS {
                    self.collapse(out, false);
                }
            }
            if kind == SpaceKind::Word {
                self.collapse(out, true);
                self.space_flushed = 2;
            }
        }
    }

    fn on_mark(&mut self, secs: f32) {
        let class = self.timing.mark_event(secs);
        self.keyer.set_dot_seconds(keyer_dot_s(self.timing.dit_s));
        self.hmm.set_hazards(self.timing.dit_s, self.sample_rate);
        let MarkClass::Element { w_dit, w_dah } = class else {
            return; // outlier — no element
        };
        if w_dit.min(w_dah) > BRANCH_MIN_LOG_POST {
            let mut next = Vec::with_capacity(self.beams.len() * 2);
            for h in self.beams.drain(..) {
                let mut dash = h.clone();
                dash.push_element('-', w_dah);
                next.push(dash);
                let mut dot = h;
                dot.push_element('.', w_dit);
                next.push(dot);
            }
            self.beams = next;
            self.prune();
        } else {
            let el = if w_dit >= w_dah { '.' } else { '-' };
            for h in &mut self.beams {
                h.push_element(el, 0.0);
            }
        }
        self.space_flushed = 0;
    }

    fn prune(&mut self) {
        self.beams.sort_by(|a, b| b.score.total_cmp(&a.score));
        self.beams
            .dedup_by(|a, b| a.text == b.text && a.symbol == b.symbol);
        self.beams.truncate(self.params.beam_width.max(1));
        if self.beams.is_empty() {
            self.beams.push(Hypothesis::new());
        }
    }

    fn best(&self) -> &Hypothesis {
        self.beams
            .iter()
            .max_by(|a, b| a.score.total_cmp(&b.score))
            .expect("at least one beam")
    }

    /// Emit the best hypothesis and restart the beam from it.
    fn collapse(&mut self, out: &mut String, word_gap: bool) {
        let best_idx = self
            .beams
            .iter()
            .enumerate()
            .max_by(|(_, a), (_, b)| a.score.total_cmp(&b.score))
            .map(|(i, _)| i)
            .unwrap_or(0);
        let mut best = self.beams.swap_remove(best_idx);
        if !best.text.is_empty() {
            out.push_str(&best.text);
            if word_gap {
                out.push(' ');
            }
            best.text.clear();
        }
        best.score = 0.0;
        self.beams.clear();
        self.beams.push(best);
    }
}

impl CwDecoder for BayesCwDecoder {
    fn push_audio(&mut self, audio: &[f32], sample_rate: f32) -> String {
        if (sample_rate - self.sample_rate).abs() > 1.0 && sample_rate > 0.0 {
            self.sample_rate = sample_rate;
            self.levels = LevelModel::new(sample_rate);
            self.hmm = KeyHmm::new(sample_rate);
            self.hmm.set_hazards(self.timing.dit_s, sample_rate);
            self.keyer = Keyer::new(sample_rate);
            self.keyer.set_dot_seconds(keyer_dot_s(self.timing.dit_s));
            self.key_down = false;
        }
        let mut out = String::new();
        for &x in audio {
            self.process_sample(x, &mut out);
        }
        out
    }

    fn wpm(&self) -> f32 {
        wpm_from_dot_seconds(self.timing.dit_s)
    }

    fn reset(&mut self) {
        *self = Self::with_params(self.sample_rate, self.params);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::skimmer::morse::encode_char;
    use std::f32::consts::TAU;

    fn keyed_tone(text: &str, wpm: f32, sample_rate: f32, pitch: f32) -> Vec<f32> {
        let dot = (1.2 / wpm * sample_rate) as usize;
        let mut samples = Vec::new();
        let mut phase = 0.0f32;
        let push = |on: bool, len: usize, phase: &mut f32, out: &mut Vec<f32>| {
            for _ in 0..len {
                *phase += TAU * pitch / sample_rate;
                out.push(if on { phase.sin() } else { 0.0 });
            }
        };
        push(false, dot * 8, &mut phase, &mut samples);
        for (wi, word) in text.split(' ').enumerate() {
            if wi > 0 {
                push(false, dot * 7, &mut phase, &mut samples);
            }
            for (ci, ch) in word.chars().enumerate() {
                if ci > 0 {
                    push(false, dot * 3, &mut phase, &mut samples);
                }
                for (ei, el) in encode_char(ch).unwrap_or("").chars().enumerate() {
                    if ei > 0 {
                        push(false, dot, &mut phase, &mut samples);
                    }
                    let len = if el == '-' { dot * 3 } else { dot };
                    push(true, len, &mut phase, &mut samples);
                }
            }
        }
        push(false, dot * 10, &mut phase, &mut samples);
        samples
    }

    /// Envelope at the engine rate (500 Hz): keyed gate with raised-cosine
    /// edges over a noise floor, mirroring what a DecoderChannel feeds us.
    fn keyed_env_500(text: &str, wpm: f32, snr_amp: f32, seed: u64) -> Vec<f32> {
        let sr = 500.0;
        let dot = (1.2 / wpm * sr).round() as usize;
        let mut gate: Vec<f32> = vec![0.0; dot * 8];
        for (wi, word) in text.split(' ').enumerate() {
            if wi > 0 {
                gate.extend(std::iter::repeat_n(0.0, dot * 7));
            }
            for (ci, ch) in word.chars().enumerate() {
                if ci > 0 {
                    gate.extend(std::iter::repeat_n(0.0, dot * 3));
                }
                for (ei, el) in encode_char(ch).unwrap_or("").chars().enumerate() {
                    if ei > 0 {
                        gate.extend(std::iter::repeat_n(0.0, dot));
                    }
                    let n = if el == '-' { dot * 3 } else { dot };
                    gate.extend(std::iter::repeat_n(1.0, n));
                }
            }
        }
        gate.extend(std::iter::repeat_n(0.0, dot * 10));
        // Channel-filter edge smearing + noise floor.
        let edge: f32 = std::env::var("EDGE").ok().and_then(|s| s.parse().ok()).unwrap_or(0.4);
        let mut lcg = seed.wrapping_mul(0x9E3779B97F4A7C15).wrapping_add(1);
        let mut level = 0.0f32;
        gate.iter()
            .map(|&g| {
                level += edge * (g - level);
                lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                let u = ((lcg >> 33) as f32 / (1u64 << 31) as f32).clamp(1e-6, 1.0);
                let noise = 0.06 * (-2.0 * u.ln()).sqrt();
                (level * snr_amp + noise).max(1e-6)
            })
            .collect()
    }

    #[test]
    #[ignore]
    fn debug_trace() {
        let sr = 500.0;
        let wpm: f32 = std::env::var("WPM").ok().and_then(|s| s.parse().ok()).unwrap_or(45.0);
        let text = std::env::var("TEXT").unwrap_or_else(|_| "CQ CQ DE W1AW W1AW K".into());
        let audio = keyed_env_500(&text, wpm, 1.0, 7);
        let mut dec = BayesCwDecoder::new(sr);
        let mut n_marks = 0;
        let mut n_spaces = 0;
        for (i, &x) in audio.iter().enumerate() {
            let l = dec.levels.log_env(x);
            let (p_filt, p_on) = dec.hmm.step(l, &dec.levels);
            dec.levels.update(l, p_filt);
            dec.key_down = if dec.levels.separation() < MIN_SEP {
                false
            } else if dec.key_down {
                p_on > KEY_OFF_P
            } else {
                p_on > KEY_ON_P
            };
            match dec.keyer.step(dec.key_down) {
                Some(KeyEvent::Mark(s)) => {
                    n_marks += 1;
                    let class = dec.timing.mark_event(s);
                    let cls = match class {
                        MarkClass::Outlier => "OUT".to_string(),
                        MarkClass::Element { w_dit, w_dah } => {
                            format!("dit{w_dit:.2}/dah{w_dah:.2}")
                        }
                    };
                    eprintln!(
                        "t={:.3} MARK {:.0}ms {} dit={:.0}ms ev={:.2}",
                        i as f32 / sr,
                        s * 1e3,
                        cls,
                        dec.timing.dit_s * 1e3,
                        dec.timing.evidence
                    );
                }
                Some(KeyEvent::Space(s)) => {
                    n_spaces += 1;
                    dec.timing.space_event(s);
                    eprintln!("t={:.3} SPACE {:.0}ms", i as f32 / sr, s * 1e3);
                }
                None => {}
            }
            if i % 400 == 0 {
                eprintln!(
                    "t={:.3} l={:.2} p={:.3} off={:.2} on={:.2} sep={:.2} key={}",
                    i as f32 / sr,
                    l,
                    p_on,
                    dec.levels.mu_off,
                    dec.levels.mu_on,
                    dec.levels.separation(),
                    dec.key_down
                );
            }
        }
        eprintln!("marks={n_marks} spaces={n_spaces} confident={}", dec.timing.is_confident());
        let mut fresh = BayesCwDecoder::new(sr);
        let audio2 = keyed_env_500(&text, wpm, 1.0, 7);
        eprintln!("decoded: {:?}", fresh.push_audio(&audio2, sr));
    }

    #[test]
    #[ignore]
    fn debug_noise_evidence() {
        // Noise-only envelope at the engine rate: watch the evidence EMA.
        let sr = 500.0;
        let mut lcg = 0xDEADBEEFCAFEBABEu64;
        let mut audio = Vec::with_capacity(30_000);
        for _ in 0..30_000 {
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let u = ((lcg >> 33) as f32 / (1u64 << 31) as f32).clamp(1e-6, 1.0);
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let v = ((lcg >> 33) as f32 / (1u64 << 31) as f32).clamp(1e-6, 1.0);
            // Rayleigh magnitude from two uniforms.
            audio.push(0.1 * (-2.0 * u.ln()).sqrt() * (0.5 + 0.5 * v));
        }
        let mut dec = BayesCwDecoder::new(sr);
        let mut out = String::new();
        let mut n_marks = 0u32;
        for (i, &x) in audio.iter().enumerate() {
            dec.process_sample(x, &mut out);
            if i % 2500 == 0 {
                eprintln!(
                    "t={:5.1}s evidence={:+.2} events={} conf={} marks~{} text={:?}",
                    i as f32 / sr,
                    dec.timing.evidence,
                    dec.timing.events,
                    dec.timing.is_confident(),
                    n_marks,
                    out
                );
            }
            n_marks = dec.timing.events;
        }
        eprintln!("final text: {out:?}");
    }

    #[test]
    fn decodes_cq() {
        let sr = 8_000.0;
        let audio = keyed_tone("CQ", 25.0, sr, 650.0);
        let mut dec = BayesCwDecoder::new(sr);
        let out = dec.push_audio(&audio, sr);
        assert!(out.contains("CQ"), "got {out:?}");
    }

    #[test]
    fn decodes_callsign() {
        let sr = 8_000.0;
        let audio = keyed_tone("CQ W1AW", 22.0, sr, 650.0);
        let mut dec = BayesCwDecoder::new(sr);
        let out = dec.push_audio(&audio, sr);
        assert!(out.contains("CQ"), "got {out:?}");
        assert!(out.contains("W1AW"), "got {out:?}");
    }

    #[test]
    fn decodes_across_speed_range() {
        for wpm in [10.0, 15.0, 20.0, 28.0, 36.0, 45.0] {
            let sr = 8_000.0;
            let audio = keyed_tone("CQ CQ DE OH2BH OH2BH K", wpm, sr, 600.0);
            let mut dec = BayesCwDecoder::new(sr);
            let out = dec.push_audio(&audio, sr);
            assert!(out.contains("OH2BH"), "failed at {wpm} wpm: got {out:?}");
        }
    }

    #[test]
    fn tracks_speed_change_mid_stream() {
        let sr = 8_000.0;
        let mut audio = keyed_tone("CQ CQ DE W1AW W1AW K", 28.0, sr, 600.0);
        audio.extend(keyed_tone("CQ CQ DE K3LR K3LR K", 16.0, sr, 600.0));
        let mut dec = BayesCwDecoder::new(sr);
        let out = dec.push_audio(&audio, sr);
        assert!(out.contains("W1AW"), "fast part lost: {out:?}");
        assert!(out.contains("K3LR"), "slow part lost: {out:?}");
    }

    #[test]
    fn noise_only_emits_nothing_usable() {
        // Envelope-like positive noise: the decoder must not manufacture
        // coherent text out of it.
        let sr = 500.0;
        let mut lcg = 0x2545F4914F6CDD1Du64;
        let mut audio = Vec::with_capacity(20_000);
        for _ in 0..20_000 {
            lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
            let a = ((lcg >> 33) as f32 / (1u64 << 31) as f32).clamp(1e-6, 1.0);
            let b = {
                lcg = lcg.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
                ((lcg >> 33) as f32 / (1u64 << 31) as f32).clamp(1e-6, 1.0)
            };
            // Rayleigh-ish magnitude.
            audio.push((-2.0 * a.ln()).sqrt() * 0.1 * (TAU * b).sin().abs() + 0.05);
        }
        let mut dec = BayesCwDecoder::new(sr);
        let out = dec.push_audio(&audio, sr);
        let alnum = out.chars().filter(|c| c.is_ascii_alphanumeric()).count();
        assert!(alnum < 6, "noise produced text: {out:?}");
    }
}
