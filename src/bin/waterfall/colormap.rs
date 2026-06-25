//! Perceptual colour ramp for waterfall (deep violet → cyan → amber → white).

use eframe::egui::Color32;

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

/// Map a dB value to a colour using reference level and dynamic range.
pub fn db_to_colour(db: f32, ref_db: f32, range_db: f32) -> Color32 {
    let floor = ref_db - range_db;
    let t = ((db - floor) / range_db).clamp(0.0, 1.0);
    let t = t.powf(1.15);
    let scaled = t * (STOPS.len() as f32 - 1.0);
    let i = scaled.floor() as usize;
    let j = (i + 1).min(STOPS.len() - 1);
    let f = scaled - i as f32;
    let lerp = |a: f32, b: f32| ((a + (b - a) * f) * 255.0) as u8;
    Color32::from_rgb(
        lerp(STOPS[i].0, STOPS[j].0),
        lerp(STOPS[i].1, STOPS[j].1),
        lerp(STOPS[i].2, STOPS[j].2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_is_dark() {
        let c = db_to_colour(-100.0, -20.0, 80.0);
        assert!(c.r() < 30 && c.b() < 80);
    }
}
