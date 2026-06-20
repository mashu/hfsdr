//! Perceptual colour ramp for waterfall display (black → blue → magenta → orange → white).

use eframe::egui::Color32;

const STOPS: [(f32, f32, f32); 5] = [
    (0.0, 0.0, 0.0),
    (0.15, 0.1, 0.6),
    (0.75, 0.1, 0.55),
    (1.0, 0.6, 0.05),
    (1.0, 1.0, 0.9),
];

/// Map a dB value to a colour using reference level and dynamic range.
pub fn db_to_colour(db: f32, ref_db: f32, range_db: f32) -> Color32 {
    let floor = ref_db - range_db;
    let t = ((db - floor) / range_db).clamp(0.0, 1.0);
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
    fn floor_is_black() {
        let c = db_to_colour(-100.0, -20.0, 80.0);
        assert_eq!(c, Color32::from_rgb(0, 0, 0));
    }

    #[test]
    fn above_range_is_whiteish() {
        let c = db_to_colour(50.0, -20.0, 80.0);
        assert!(c.r() > 200 && c.g() > 200);
    }
}
