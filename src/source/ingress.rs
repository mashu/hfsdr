//! Client-side IQ ingress rate helpers (shared by GUI settings and connect).

/// Default spectrum/process IQ rate for wideband local SDRs (Hz).
pub const DEFAULT_WIDEBAND_PROCESS_HZ: u32 = 192_000;

/// Device rates at or above this get [`effective_iq_process_hz`] when unset.
pub const WIDEBAND_PROCESS_THRESHOLD_HZ: u32 = 384_000;

/// Effective client process rate: explicit setting, or 192 kHz on wideband when divisible.
pub fn effective_iq_process_hz(configured: u32, device_rate: u32) -> u32 {
    if configured != 0 {
        return configured;
    }
    if device_rate < WIDEBAND_PROCESS_THRESHOLD_HZ {
        return 0;
    }
    for &target in &[DEFAULT_WIDEBAND_PROCESS_HZ, 96_000, 48_000] {
        if device_rate > target && device_rate.is_multiple_of(target) {
            return target;
        }
    }
    0
}

/// `(decimation factor, spectrum IQ rate)` for connect / dual-ring setup.
pub fn ingress_decimation_from_hz(configured: u32, device_rate: u32) -> (usize, f32) {
    let eff = effective_iq_process_hz(configured, device_rate);
    if eff == 0 || eff >= device_rate {
        return (1, device_rate as f32);
    }
    if device_rate.is_multiple_of(eff) {
        let factor = (device_rate / eff) as usize;
        (factor.max(1), eff as f32)
    } else {
        (1, device_rate as f32)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_192k_for_768k_native_setting() {
        assert_eq!(effective_iq_process_hz(0, 768_000), 192_000);
        assert_eq!(ingress_decimation_from_hz(0, 768_000), (4, 192_000.0));
    }

    #[test]
    fn auto_192k_for_384k() {
        assert_eq!(ingress_decimation_from_hz(0, 384_000), (2, 192_000.0));
    }

    #[test]
    fn narrow_stays_native() {
        assert_eq!(ingress_decimation_from_hz(0, 96_000), (1, 96_000.0));
    }

    #[test]
    fn explicit_overrides_auto() {
        assert_eq!(ingress_decimation_from_hz(48_000, 768_000), (16, 48_000.0));
    }
}
