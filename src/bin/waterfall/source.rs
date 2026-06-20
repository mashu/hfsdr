//! CLI parsing and source construction for the waterfall binary.

use hfsdr::{AirspyHf, Complex32, Consumer, IqSource, KiwiSource};

/// Build the requested source, tune it, and start streaming.
pub fn build_source() -> Result<(Box<dyn IqSource>, Consumer<Complex32>, f32, f64, bool), String> {
    let args: Vec<String> = std::env::args().collect();
    let kind = args.get(1).map(String::as_str).unwrap_or("airspy");

    match kind {
        "kiwi" => build_kiwi(&args),
        _ => build_airspy(&args),
    }
}

fn build_kiwi(
    args: &[String],
) -> Result<(Box<dyn IqSource>, Consumer<Complex32>, f32, f64, bool), String> {
    let host = args.get(2).cloned().ok_or("kiwi: missing host")?;
    let port = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(8073u16);
    let center = args
        .get(4)
        .and_then(|s| s.parse().ok())
        .unwrap_or(7_030_000.0);
    let mut src = KiwiSource::new(host, port).with_passband(-500, 500);
    src.tune(center).map_err(|e| e.to_string())?;
    let sr = src.sample_rate() as f32;
    let iq = src.start().map_err(|e| e.to_string())?;
    Ok((Box::new(src), iq, sr, center, true))
}

fn build_airspy(
    args: &[String],
) -> Result<(Box<dyn IqSource>, Consumer<Complex32>, f32, f64, bool), String> {
    let mut src = AirspyHf::open().map_err(|e| e.to_string())?;
    let sr = args
        .get(2)
        .and_then(|s| s.parse().ok())
        .or_else(|| src.sample_rates().first().copied())
        .unwrap_or(768_000);
    let center = args
        .get(3)
        .and_then(|s| s.parse().ok())
        .unwrap_or(7_030_000.0);
    src.set_sample_rate(sr).map_err(|e| e.to_string())?;
    src.set_lib_dsp(true).ok();
    src.tune(center).map_err(|e| e.to_string())?;
    let iq = src.start().map_err(|e| e.to_string())?;
    Ok((Box::new(src), iq, sr as f32, center, false))
}
