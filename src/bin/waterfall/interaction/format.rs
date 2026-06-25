//! Frequency label and axis tick formatting.

/// Relative offset label for the axis when no absolute carrier is set.
pub fn format_offset_label(offset_hz: f64) -> String {
    if offset_hz.abs() >= 1000.0 {
        format!("{:.2} kHz", offset_hz / 1000.0)
    } else {
        format!("{:.0} Hz", offset_hz)
    }
}

/// Absolute RF frequency for status bar / cursor (Hz).
pub fn format_absolute_freq_hz(freq_hz: f64) -> String {
    if freq_hz.abs() >= 1_000_000.0 {
        format!("{:.6} MHz", freq_hz / 1_000_000.0)
    } else if freq_hz.abs() >= 1000.0 {
        format!("{:.3} kHz", freq_hz / 1000.0)
    } else {
        format!("{:.0} Hz", freq_hz)
    }
}

/// Absolute carrier frequency for axis labels.
pub fn format_freq_hz(freq_hz: f64) -> String {
    if freq_hz.abs() >= 1_000_000.0 {
        format!("{:.3}", freq_hz / 1_000_000.0)
    } else if freq_hz.abs() >= 10_000.0 {
        format!("{:.1}k", freq_hz / 1_000.0)
    } else {
        format!("{:.0}", freq_hz)
    }
}

/// Nice major tick spacing for a frequency axis of the given span (Hz).
pub fn nice_freq_step_hz(span_hz: f32) -> f32 {
    if span_hz <= 0.0 {
        return 1000.0;
    }
    let targets = [5.0_f32, 4.0, 6.0];
    let mut best = span_hz / 5.0;
    for &n in &targets {
        let raw = span_hz / n;
        let exp = raw.log10().floor();
        let mag = 10f32.powf(exp);
        let norm = raw / mag;
        let nice = if norm <= 1.0 {
            1.0
        } else if norm <= 2.0 {
            2.0
        } else if norm <= 5.0 {
            5.0
        } else {
            10.0
        };
        let step = nice * mag;
        if (span_hz / step).round() >= 3.0 {
            best = step;
            break;
        }
        best = step;
    }
    best.max(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn nice_freq_step_splits_span() {
        let step = nice_freq_step_hz(3_000.0);
        assert!(step >= 200.0 && step <= 1_500.0);
        assert!((3_000.0 / step).round() >= 2.0);
    }

    #[test]
    fn format_offset_uses_khz_above_threshold() {
        assert_eq!(format_offset_label(1500.0), "1.50 kHz");
        assert_eq!(format_offset_label(250.0), "250 Hz");
    }

    #[test]
    fn format_absolute_freq_mhz() {
        assert_eq!(format_absolute_freq_hz(14_030_000.0), "14.030000 MHz");
        assert_eq!(format_absolute_freq_hz(500.0), "500 Hz");
        assert_eq!(format_absolute_freq_hz(7030.0), "7.030 kHz");
    }

    #[test]
    fn format_freq_hz_compact_axis() {
        assert_eq!(format_freq_hz(14_030_000.0), "14.030");
        assert_eq!(format_freq_hz(703_000.0), "703.0k");
    }

    #[test]
    fn nice_freq_step_non_positive_span() {
        assert_eq!(nice_freq_step_hz(0.0), 1000.0);
        assert_eq!(nice_freq_step_hz(-5.0), 1000.0);
    }
}
