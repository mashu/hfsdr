//! Spectrum analysis rate planning: auto FFT sizing and zoom-aware decimation.

/// Target FFT bin width for CW peak picking (Hz).
pub const TARGET_BIN_HZ: f32 = 8.0;

/// Maximum FFT size (memory/CPU trade-off on wideband sources).
pub const MAX_FFT_SIZE: usize = 65_536;

/// Minimum FFT size.
pub const MIN_FFT_SIZE: usize = 2048;

/// Enable pre-FFT decimation when the visible span is below this fraction of IQ rate.
pub const ZOOM_DECIM_THRESHOLD: f32 = 0.9;

/// Anti-alias oversample above the visible span when choosing decimated rate.
const OVERSAMPLE: f32 = 2.5;

/// Floor on decimated spectrum rate (keeps FFT size reasonable on narrow views).
const MIN_SPECTRUM_RATE: f32 = 4_000.0;

/// Pick a power-of-two FFT size for ~[`TARGET_BIN_HZ`] bin width at `iq_rate`.
pub fn auto_fft_size(iq_rate: f32) -> usize {
    if iq_rate <= 0.0 {
        return 2048;
    }
    let raw = (iq_rate / TARGET_BIN_HZ).ceil() as usize;
    raw.next_power_of_two().clamp(MIN_FFT_SIZE, MAX_FFT_SIZE)
}

/// Integer decimation factor when `view_span_hz` is narrower than full bandwidth.
pub fn spectrum_zoom_decimation(iq_rate: f32, view_span_hz: f32) -> usize {
    if iq_rate <= 0.0 || view_span_hz <= 0.0 || view_span_hz >= iq_rate * ZOOM_DECIM_THRESHOLD {
        return 1;
    }
    let target_rate = (view_span_hz * OVERSAMPLE).max(MIN_SPECTRUM_RATE);
    let raw = (iq_rate / target_rate).ceil() as usize;
    raw.next_power_of_two().clamp(1, 256)
}

/// Combined plan: (decimation factor, FFT size, effective IQ rate for spectrum).
pub fn spectrum_plan(
    iq_rate: f32,
    manual_fft: usize,
    fft_auto: bool,
    view_span_hz: f32,
) -> (usize, usize, f32) {
    let decim = spectrum_zoom_decimation(iq_rate, view_span_hz);
    let eff = iq_rate / decim as f32;
    let fft = if fft_auto {
        auto_fft_size(eff)
    } else {
        manual_fft.clamp(1024, MAX_FFT_SIZE)
    };
    (decim, fft, eff)
}

/// Bin width in Hz for a given IQ rate and FFT size.
pub fn bin_width_hz(iq_rate: f32, fft_size: usize) -> f32 {
    iq_rate / fft_size.max(1) as f32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kiwi_fft_stays_small() {
        let n = auto_fft_size(12_000.0);
        assert!(n <= 4096);
    }

    #[test]
    fn airspy_fft_scales_up() {
        let n = auto_fft_size(768_000.0);
        assert!(n >= 16_384);
        let bin = bin_width_hz(768_000.0, n);
        assert!(bin <= 16.0, "bin width {bin} Hz too wide");
    }

    #[test]
    fn auto_fft_is_power_of_two() {
        assert!(auto_fft_size(48_000.0).is_power_of_two());
    }

    #[test]
    fn full_span_skips_decimation() {
        assert_eq!(spectrum_zoom_decimation(768_000.0, 768_000.0), 1);
        assert_eq!(spectrum_zoom_decimation(768_000.0, 700_000.0), 1);
    }

    #[test]
    fn zoomed_airspy_decimates() {
        let decim = spectrum_zoom_decimation(768_000.0, 30_000.0);
        assert!(decim >= 8, "decim {decim}");
        let (_, fft, eff) = spectrum_plan(768_000.0, 2048, true, 30_000.0);
        assert!(eff < 100_000.0);
        assert!(fft <= MAX_FFT_SIZE);
        assert!(bin_width_hz(eff, fft) <= 20.0);
    }
}
