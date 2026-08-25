//! Waterfall colour ramp (deep violet → cyan → amber → white) with a lookup table.
//!
//! Frontend-agnostic: colours are plain RGBA bytes so a GUI, a PNG exporter or a
//! CLI can share one ramp. The GUI wraps [`WaterfallPalette`] to produce its own
//! pixel type.
//!
//! The ramp itself costs a `powf` plus a stop interpolation per pixel, which at
//! 1580×360 is ~10 ms — the dominant cost of a full waterfall recompose. Since
//! it depends only on the reference level and dynamic range, both of which change
//! rarely, the mapping is precomputed into [`LUT_LEN`] entries and indexed per
//! pixel instead.

/// RGBA colour ramp stops, low signal to high.
const STOPS: [(f32, f32, f32); 8] = [
    (0.03, 0.02, 0.08),
    (0.08, 0.05, 0.28),
    (0.05, 0.22, 0.55),
    (0.04, 0.55, 0.72),
    (0.15, 0.78, 0.55),
    (0.85, 0.72, 0.12),
    (0.98, 0.88, 0.35),
    (1.0, 1.0, 0.98),
];

/// Perceptual gamma applied to the normalized level before the ramp.
const RAMP_GAMMA: f32 = 1.15;

/// LUT entries. 2048 keeps the worst per-channel error at 1/255 — invisible —
/// for 8 KiB and a ~50 µs rebuild.
pub const LUT_LEN: usize = 2048;

/// Exact ramp evaluation. Reference implementation for the LUT and for tests.
pub fn db_to_rgba(db: f32, ref_db: f32, range_db: f32) -> [u8; 4] {
    let range = if range_db.abs() < f32::EPSILON {
        1.0
    } else {
        range_db
    };
    let floor = ref_db - range;
    let t = ((db - floor) / range).clamp(0.0, 1.0).powf(RAMP_GAMMA);
    ramp_at(t)
}

/// Sample the stop ramp at `t` in `[0, 1]`.
fn ramp_at(t: f32) -> [u8; 4] {
    let scaled = t.clamp(0.0, 1.0) * (STOPS.len() as f32 - 1.0);
    let i = (scaled.floor() as usize).min(STOPS.len() - 1);
    let j = (i + 1).min(STOPS.len() - 1);
    let f = scaled - i as f32;
    let lerp = |a: f32, b: f32| ((a + (b - a) * f) * 255.0) as u8;
    [
        lerp(STOPS[i].0, STOPS[j].0),
        lerp(STOPS[i].1, STOPS[j].1),
        lerp(STOPS[i].2, STOPS[j].2),
        255,
    ]
}

/// Precomputed dB → RGBA ramp for one (reference level, range) pair.
#[derive(Clone, Debug)]
pub struct WaterfallPalette {
    lut: Vec<[u8; 4]>,
    ref_db: f32,
    range_db: f32,
    floor: f32,
    inv_range: f32,
}

impl WaterfallPalette {
    pub fn new(ref_db: f32, range_db: f32) -> Self {
        let range = if range_db.abs() < f32::EPSILON {
            1.0
        } else {
            range_db
        };
        let lut = (0..LUT_LEN)
            .map(|k| ramp_at((k as f32 / (LUT_LEN - 1) as f32).powf(RAMP_GAMMA)))
            .collect();
        Self {
            lut,
            ref_db,
            range_db,
            floor: ref_db - range,
            inv_range: 1.0 / range,
        }
    }

    /// Rebuild only when the level mapping actually changed.
    pub fn sync(&mut self, ref_db: f32, range_db: f32) {
        if self.ref_db != ref_db || self.range_db != range_db {
            *self = Self::new(ref_db, range_db);
        }
    }

    pub fn ref_db(&self) -> f32 {
        self.ref_db
    }

    pub fn range_db(&self) -> f32 {
        self.range_db
    }

    /// Map one dB value to RGBA — an index and a load, no transcendentals.
    #[inline]
    pub fn rgba(&self, db: f32) -> [u8; 4] {
        let t = ((db - self.floor) * self.inv_range).clamp(0.0, 1.0);
        let idx = (t * (LUT_LEN - 1) as f32) as usize;
        // Safety net for a NaN dB, which would make `t` and `idx` unreliable.
        self.lut[idx.min(LUT_LEN - 1)]
    }
}

impl Default for WaterfallPalette {
    fn default() -> Self {
        Self::new(-20.0, 80.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lut_matches_direct_evaluation() {
        let (ref_db, range_db) = (-20.0f32, 80.0f32);
        let pal = WaterfallPalette::new(ref_db, range_db);
        let mut worst = 0i32;
        for k in 0..20_000 {
            let db = -140.0 + k as f32 * 0.008;
            let exact = db_to_rgba(db, ref_db, range_db);
            let lut = pal.rgba(db);
            for c in 0..3 {
                worst = worst.max((exact[c] as i32 - lut[c] as i32).abs());
            }
        }
        assert!(worst <= 1, "LUT should be visually exact, worst channel err {worst}");
    }

    #[test]
    fn floor_is_dark_and_ceiling_is_bright() {
        let pal = WaterfallPalette::new(-20.0, 80.0);
        let floor = pal.rgba(-100.0);
        let ceiling = pal.rgba(0.0);
        assert!(floor[0] < 30 && floor[2] < 80);
        assert!(ceiling[0] > 200 && ceiling[1] > 200);
    }

    #[test]
    fn clamps_outside_the_display_range() {
        let pal = WaterfallPalette::new(-20.0, 80.0);
        assert_eq!(pal.rgba(-100.0), pal.rgba(-200.0));
        assert_eq!(pal.rgba(-20.0), pal.rgba(10.0));
        // A NaN level must not panic or index out of bounds.
        let _ = pal.rgba(f32::NAN);
    }

    #[test]
    fn sync_rebuilds_only_on_change() {
        let mut pal = WaterfallPalette::new(-20.0, 80.0);
        let before = pal.rgba(-60.0);
        pal.sync(-20.0, 80.0);
        assert_eq!(pal.rgba(-60.0), before);
        pal.sync(-40.0, 60.0);
        assert_eq!(pal.ref_db(), -40.0);
        assert_eq!(pal.range_db(), 60.0);
    }

    #[test]
    fn zero_range_does_not_divide_by_zero() {
        let pal = WaterfallPalette::new(-20.0, 0.0);
        let c = pal.rgba(-20.0);
        assert_eq!(c[3], 255);
        let _ = db_to_rgba(-20.0, -20.0, 0.0);
    }
}
