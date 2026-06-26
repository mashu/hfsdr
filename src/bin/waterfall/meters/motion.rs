//! Frame-rate–independent smoothing for analog meters (needle + level bars).

/// Spring settle time (~95% of step). Real S-meters are heavily damped — no bounce.
const NEEDLE_PERIOD_S: f32 = 0.62;
const NEEDLE_ZETA: f32 = 1.28;
const BAR_PERIOD_S: f32 = 0.34;
const BAR_ZETA: f32 = 1.18;
const SCOPE_PEAK_PERIOD_S: f32 = 0.55;
const SCOPE_PEAK_ZETA: f32 = 1.15;

#[derive(Clone, Copy, Debug, Default)]
struct Spring {
    value: f32,
    velocity: f32,
}

impl Spring {
    fn step(&mut self, target: f32, dt: f32, period_s: f32, zeta: f32) -> f32 {
        let dt = dt.clamp(0.0, 0.1);
        if dt <= 0.0 {
            return self.value;
        }
        let omega = std::f32::consts::TAU / period_s.max(0.02);
        let diff = target - self.value;
        let accel = omega * omega * diff - 2.0 * zeta.max(0.5) * omega * self.velocity;
        self.velocity += accel * dt;
        self.value += self.velocity * dt;
        self.value
    }

    fn snap(&mut self, value: f32) {
        self.value = value;
        self.velocity = 0.0;
    }
}

/// UI-side meter animation state (needle + bar fills).
#[derive(Clone, Debug, Default)]
pub struct MeterDisplayState {
    needle: Spring,
    if_fill: Spring,
    af_fill: Spring,
    af_scope_peak: Spring,
    pub af_scope: super::af_scope_display::AfScopeDisplayState,
    pub display: MeterSmoothed,
}

/// Raw meter targets for one frame.
#[derive(Clone, Copy)]
pub struct MeterTargets {
    pub needle_t: f32,
    pub if_fill: f32,
    pub af_fill: f32,
    pub af_scope_peak: f32,
    pub live: bool,
}

/// Smoothed values ready for painting.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeterSmoothed {
    pub needle_t: f32,
    pub if_fill: f32,
    pub af_fill: f32,
    pub af_scope_peak: f32,
}

impl MeterDisplayState {
    pub fn tick(&mut self, dt: f32, targets: MeterTargets) -> MeterSmoothed {
        if !targets.live {
            self.needle.snap(0.0);
            self.if_fill.snap(0.0);
            self.af_fill.snap(0.0);
            self.af_scope_peak.snap(0.0);
            self.af_scope.reset();
            self.display = MeterSmoothed {
                needle_t: 0.0,
                if_fill: 0.0,
                af_fill: 0.0,
                af_scope_peak: 0.0,
            };
            return self.display;
        }

        let needle_t = self.needle.step(
            targets.needle_t.clamp(0.0, 1.0),
            dt,
            NEEDLE_PERIOD_S,
            NEEDLE_ZETA,
        );
        let if_fill = self.if_fill.step(
            targets.if_fill.clamp(0.0, 1.0),
            dt,
            BAR_PERIOD_S,
            BAR_ZETA,
        );
        let af_fill = self.af_fill.step(
            targets.af_fill.clamp(0.0, 1.0),
            dt,
            BAR_PERIOD_S,
            BAR_ZETA,
        );
        let af_scope_peak = self.af_scope_peak.step(
            targets.af_scope_peak.max(0.0),
            dt,
            SCOPE_PEAK_PERIOD_S,
            SCOPE_PEAK_ZETA,
        );
        self.display = MeterSmoothed {
            needle_t,
            if_fill,
            af_fill,
            af_scope_peak,
        };
        self.display
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn spring_moves_toward_target() {
        let mut spring = Spring::default();
        let mut v = 0.0;
        for _ in 0..200 {
            v = spring.step(1.0, 1.0 / 60.0, 0.2, 1.0);
        }
        assert!(v > 0.9);
        assert!(v < 1.05);
    }

    #[test]
    fn offline_snap_resets() {
        let mut state = MeterDisplayState::default();
        let live = MeterTargets {
            needle_t: 0.8,
            if_fill: 0.5,
            af_fill: 0.4,
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
