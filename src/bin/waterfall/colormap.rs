//! egui adapter over the shared waterfall ramp in `hfsdr::dsp::colormap`.
//!
//! The ramp and its lookup table live in the library so any frontend can render
//! the same waterfall; this file only converts RGBA bytes to egui's pixel type.

use eframe::egui::Color32;
pub use hfsdr::WaterfallPalette;

/// Map a dB value to a colour using reference level and dynamic range.
///
/// Exact evaluation — fine for one-off UI elements such as the legend. The
/// per-pixel waterfall path uses [`palette_colour`] with a cached palette
/// instead, because this costs a `powf` per call.
pub fn db_to_colour(db: f32, ref_db: f32, range_db: f32) -> Color32 {
    let [r, g, b, _] = hfsdr::db_to_rgba(db, ref_db, range_db);
    Color32::from_rgb(r, g, b)
}

/// Map a dB value through a prebuilt palette — an index and a load.
#[inline]
pub fn palette_colour(palette: &WaterfallPalette, db: f32) -> Color32 {
    let [r, g, b, _] = palette.rgba(db);
    Color32::from_rgb(r, g, b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floor_is_dark() {
        let c = db_to_colour(-100.0, -20.0, 80.0);
        assert!(c.r() < 30 && c.b() < 80);
    }

    #[test]
    fn ceiling_is_bright() {
        let c = db_to_colour(0.0, -20.0, 80.0);
        assert!(c.r() > 200 && c.g() > 200);
    }

    #[test]
    fn mid_range_is_between_floor_and_ceiling() {
        let floor = db_to_colour(-100.0, -20.0, 80.0);
        let mid = db_to_colour(-60.0, -20.0, 80.0);
        let ceiling = db_to_colour(0.0, -20.0, 80.0);
        assert!(mid.r() > floor.r());
        assert!(mid.r() < ceiling.r());
    }

    #[test]
    fn below_floor_matches_floor_colour() {
        let floor = db_to_colour(-100.0, -20.0, 80.0);
        let below = db_to_colour(-200.0, -20.0, 80.0);
        assert_eq!(floor, below);
    }

    #[test]
    fn above_ref_matches_ceiling_colour() {
        let ceiling = db_to_colour(-20.0, -20.0, 80.0);
        let above = db_to_colour(10.0, -20.0, 80.0);
        assert_eq!(ceiling, above);
    }

    /// The cached palette must agree with exact evaluation everywhere it is used.
    #[test]
    fn palette_matches_direct_colour() {
        let pal = WaterfallPalette::new(-20.0, 80.0);
        for k in 0..500 {
            let db = -130.0 + k as f32 * 0.3;
            let a = db_to_colour(db, -20.0, 80.0);
            let b = palette_colour(&pal, db);
            let d = |x: u8, y: u8| (x as i32 - y as i32).abs();
            assert!(
                d(a.r(), b.r()) <= 1 && d(a.g(), b.g()) <= 1 && d(a.b(), b.b()) <= 1,
                "palette mismatch at {db} dB: {a:?} vs {b:?}"
            );
        }
    }
}
