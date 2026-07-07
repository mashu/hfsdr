//! Frame-rate–independent smoothing for analog meters (needle + level bars).

/// Asymmetric rise/fall on the needle target (classic analog S-meter ballistics).
const NEEDLE_ATTACK_TAU_S: f32 = 0.45;
const NEEDLE_DECAY_TAU_S: f32 = 2.8;
/// Light spring on the smoothed target — visible momentum without bounce.
const NEEDLE_PERIOD_S: f32 = 0.72;
const NEEDLE_ZETA: f32 = 0.92;
const BAR_SPRING_PERIOD_S: f32 = 0.72;
const BAR_SPRING_ZETA: f32 = 1.05;
/// IF IQ AGC bar — gain moves slowly; avoids flicker between CW elements.
const IF_BAR_ATTACK_TAU_S: f32 = 0.45;
const IF_BAR_DECAY_TAU_S: f32 = 2.2;
/// AF peak bar — quick rise on key-down, slow fall between dits (VU-style).
const AF_BAR_ATTACK_TAU_S: f32 = 0.22;
const AF_BAR_DECAY_TAU_S: f32 = 2.6;
const SCOPE_PEAK_PERIOD_S: f32 = 0.55;
const SCOPE_PEAK_ZETA: f32 = 1.15;

fn ballistic_step(current: f32, target: f32, dt: f32, attack_tau: f32, decay_tau: f32) -> f32 {
    ballistic_step_range(current, target, dt, attack_tau, decay_tau, 0.0, 1.0)
}

fn ballistic_step_range(
    current: f32,
    target: f32,
    dt: f32,
    attack_tau: f32,
    decay_tau: f32,
    lo: f32,
    hi: f32,
) -> f32 {
    let dt = dt.clamp(0.0, 0.1);
    if dt <= 0.0 {
        return current;
    }
    let target = if target.is_finite() {
        target
    } else {
        current
    };
    let tau = if target > current {
        attack_tau
    } else {
        decay_tau
    };
    let alpha = 1.0 - (-dt / tau.max(0.02)).exp();
    (current + alpha * (target - current)).clamp(lo, hi)
}

#[derive(Clone, Copy, Debug, Default)]
struct Spring {
    value: f32,
    velocity: f32,
}

impl Spring {
    fn step(&mut self, target: f32, dt: f32, period_s: f32, zeta: f32, lo: f32, hi: f32) -> f32 {
        let dt = dt.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return self.value.clamp(lo, hi);
        }
        let target = if target.is_finite() {
            target.clamp(lo, hi)
        } else {
            self.value
        };
        let omega = std::f32::consts::TAU / period_s.max(0.02);
        let diff = target - self.value;
        let accel = omega * omega * diff - 2.0 * zeta.max(0.5) * omega * self.velocity;
        self.velocity += accel * dt;
        self.value += self.velocity * dt;
        self.value = self.value.clamp(lo, hi);
        if self.value <= lo && self.velocity < 0.0 {
            self.velocity = 0.0;
        }
        if self.value >= hi && self.velocity > 0.0 {
            self.velocity = 0.0;
        }
        self.value
    }

    fn snap(&mut self, value: f32) {
        self.value = if value.is_finite() { value } else { 0.0 };
        self.velocity = 0.0;
    }
}

/// UI-side meter animation state (needle + bar fills).
#[derive(Clone, Debug, Default)]
pub struct MeterDisplayState {
    needle: Spring,
    needle_ballistic: f32,
    if_fill_ballistic: f32,
    af_fill_ballistic: f32,
    if_gain_ballistic: f32,
    af_peak_ballistic: f32,
    if_fill: Spring,
    af_fill: Spring,
    af_scope_peak: Spring,
    pub af_scope: super::af_scope_display::AfScopeDisplayState,
    pub af_scope_view: super::af_scope_state::AfScopeViewState,
    pub display: MeterSmoothed,
}

/// Raw meter targets for one frame.
#[derive(Clone, Copy)]
pub struct MeterTargets {
    pub needle_t: f32,
    pub if_fill: f32,
    pub af_fill: f32,
    pub if_gain: f32,
    pub af_peak: f32,
    pub af_scope_peak: f32,
    pub live: bool,
}

/// Smoothed values ready for painting.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeterSmoothed {
    pub needle_t: f32,
    pub if_fill: f32,
    pub af_fill: f32,
    pub if_gain_display: f32,
    pub af_peak_display: f32,
    pub af_scope_peak: f32,
}

impl MeterDisplayState {
    pub fn tick(&mut self, dt: f32, targets: MeterTargets) -> MeterSmoothed {
        if !targets.live {
            self.needle.snap(0.0);
            self.needle_ballistic = 0.0;
            self.if_fill_ballistic = 0.0;
            self.af_fill_ballistic = 0.0;
            self.if_gain_ballistic = 0.0;
            self.af_peak_ballistic = 0.0;
            self.if_fill.snap(0.0);
            self.af_fill.snap(0.0);
            self.af_scope_peak.snap(0.0);
            self.display = MeterSmoothed {
                needle_t: 0.0,
                if_fill: 0.0,
                af_fill: 0.0,
                if_gain_display: 0.0,
                af_peak_display: 0.0,
                af_scope_peak: 0.0,
            };
            return self.display;
        }

        let raw_needle = targets.needle_t.clamp(0.0, 1.0);
        self.needle_ballistic = ballistic_step(
            self.needle_ballistic,
            raw_needle,
            dt,
            NEEDLE_ATTACK_TAU_S,
            NEEDLE_DECAY_TAU_S,
        );
        let needle_t = self.needle.step(
            self.needle_ballistic,
            dt,
            NEEDLE_PERIOD_S,
            NEEDLE_ZETA,
            0.0,
            1.0,
        );

        self.if_fill_ballistic = ballistic_step(
            self.if_fill_ballistic,
            targets.if_fill.clamp(0.0, 1.0),
            dt,
            IF_BAR_ATTACK_TAU_S,
            IF_BAR_DECAY_TAU_S,
        );
        let if_fill = self.if_fill.step(
            self.if_fill_ballistic,
            dt,
            BAR_SPRING_PERIOD_S,
            BAR_SPRING_ZETA,
            0.0,
            1.0,
        );

        self.af_fill_ballistic = ballistic_step(
            self.af_fill_ballistic,
            targets.af_fill.clamp(0.0, 1.0),
            dt,
            AF_BAR_ATTACK_TAU_S,
            AF_BAR_DECAY_TAU_S,
        );
        let af_fill = self.af_fill.step(
            self.af_fill_ballistic,
            dt,
            BAR_SPRING_PERIOD_S,
            BAR_SPRING_ZETA,
            0.0,
            1.0,
        );

        self.if_gain_ballistic = ballistic_step_range(
            self.if_gain_ballistic,
            targets.if_gain.max(0.0),
            dt,
            IF_BAR_ATTACK_TAU_S,
            IF_BAR_DECAY_TAU_S,
            0.0,
            128.0,
        );
        self.af_peak_ballistic = ballistic_step_range(
            self.af_peak_ballistic,
            targets.af_peak.max(0.0),
            dt,
            AF_BAR_ATTACK_TAU_S,
            AF_BAR_DECAY_TAU_S,
            0.0,
            2.0,
        );

        let af_scope_peak = self.af_scope_peak.step(
            targets.af_scope_peak,
            dt,
            SCOPE_PEAK_PERIOD_S,
            SCOPE_PEAK_ZETA,
            0.0,
            f32::INFINITY,
        );
        self.display = MeterSmoothed {
            needle_t,
            if_fill,
            af_fill,
            if_gain_display: self.if_gain_ballistic,
            af_peak_display: self.af_peak_ballistic,
            af_scope_peak,
        };
        self.display
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ballistic_rises_faster_than_it_falls() {
        let mut up = 0.0;
        let mut down = 1.0;
        for _ in 0..30 {
            up = ballistic_step(up, 1.0, 1.0 / 60.0, NEEDLE_ATTACK_TAU_S, NEEDLE_DECAY_TAU_S);
            down = ballistic_step(down, 0.0, 1.0 / 60.0, NEEDLE_ATTACK_TAU_S, NEEDLE_DECAY_TAU_S);
        }
        assert!(up > 1.0 - down, "rise delta {up} should exceed fall delta {}", 1.0 - down);
    }

    #[test]
    fn spring_moves_toward_target() {
        let mut spring = Spring::default();
        let mut v = 0.0;
        for _ in 0..200 {
            v = spring.step(1.0, 1.0 / 60.0, 0.2, 1.0, 0.0, 1.0);
        }
        assert!(v > 0.9);
        assert!(v <= 1.0);
    }

    #[test]
    fn needle_spring_stays_on_scale_under_large_steps() {
        let mut state = MeterDisplayState::default();
        for _ in 0..30 {
            let out = state.tick(
                1.0 / 30.0,
                MeterTargets {
                    needle_t: 1.0,
                    if_fill: 0.0,
                    af_fill: 0.0,
                    if_gain: 1.0,
                    af_peak: 0.0,
                    af_scope_peak: 0.0,
                    live: true,
                },
            );
            assert!(out.needle_t.is_finite());
            assert!((0.0..=1.0).contains(&out.needle_t));
        }
        for _ in 0..30 {
            let out = state.tick(
                1.0 / 30.0,
                MeterTargets {
                    needle_t: 0.0,
                    if_fill: 0.0,
                    af_fill: 0.0,
                    if_gain: 1.0,
                    af_peak: 0.0,
                    af_scope_peak: 0.0,
                    live: true,
                },
            );
            assert!((0.0..=1.0).contains(&out.needle_t));
        }
    }

    #[test]
    fn offline_snap_resets() {
        let mut state = MeterDisplayState::default();
        let live = MeterTargets {
            needle_t: 0.8,
            if_fill: 0.5,
            af_fill: 0.4,
            if_gain: 12.0,
            af_peak: 0.35,
            af_scope_peak: 0.3,
            live: true,
        };
        for _ in 0..120 {
            state.tick(1.0 / 60.0, live);
        }
        let off = state.tick(1.0 / 60.0, MeterTargets { live: false, ..live });
        assert_eq!(off.needle_t, 0.0);
        assert_eq!(off.if_fill, 0.0);
    }
}
