//! Runtime availability of optional native SDR libraries.
//!
//! libairspyhf, librtlsdr, and libSoapySDR are loaded at runtime on every
//! supported platform (Windows, Linux, macOS). Call [`init`] once at launch,
//! then gate UI and connect paths with [`airspy_available`], [`rtlsdr_available`],
//! and [`soapy_available`].

use std::sync::OnceLock;

use crate::sdr_ffi::dylib;
#[cfg(feature = "airspy")]
use crate::sdr_ffi::dylib::AIRSPYHF_SONAMES;
#[cfg(feature = "rtlsdr")]
use crate::sdr_ffi::dylib::RTLSDR_SONAMES;
#[cfg(feature = "soapy")]
use crate::sdr_ffi::dylib::SOAPYSDR_SONAMES;

struct State {
    #[cfg(feature = "airspy")]
    airspy: bool,
    #[cfg(feature = "rtlsdr")]
    rtlsdr: bool,
    #[cfg(feature = "soapy")]
    soapy: bool,
}

static STATE: OnceLock<State> = OnceLock::new();

/// Probe native libraries once. Idempotent; safe to call from `main`.
pub fn init() {
    let _ = state();
}

#[cfg(feature = "airspy")]
pub fn airspy_available() -> bool {
    state().airspy
}

#[cfg(not(feature = "airspy"))]
pub fn airspy_available() -> bool {
    false
}

#[cfg(feature = "rtlsdr")]
pub fn rtlsdr_available() -> bool {
    state().rtlsdr
}

#[cfg(not(feature = "rtlsdr"))]
pub fn rtlsdr_available() -> bool {
    false
}

#[cfg(feature = "soapy")]
pub fn soapy_available() -> bool {
    state().soapy
}

#[cfg(not(feature = "soapy"))]
pub fn soapy_available() -> bool {
    false
}

fn state() -> &'static State {
    STATE.get_or_init(|| State {
        #[cfg(feature = "airspy")]
        airspy: dylib::can_load(AIRSPYHF_SONAMES),
        #[cfg(feature = "rtlsdr")]
        rtlsdr: dylib::can_load(RTLSDR_SONAMES),
        #[cfg(feature = "soapy")]
        soapy: dylib::can_load(SOAPYSDR_SONAMES),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn init_is_idempotent() {
        init();
        init();
    }

    #[cfg(feature = "airspy")]
    #[test]
    fn airspy_probe_uses_dylib_search() {
        init();
        assert_eq!(airspy_available(), dylib::can_load(AIRSPYHF_SONAMES));
    }

    #[cfg(feature = "rtlsdr")]
    #[test]
    fn rtlsdr_probe_uses_dylib_search() {
        init();
        assert_eq!(rtlsdr_available(), dylib::can_load(RTLSDR_SONAMES));
    }

    #[cfg(feature = "soapy")]
    #[test]
    fn soapy_probe_uses_dylib_search() {
        init();
        assert_eq!(soapy_available(), dylib::can_load(SOAPYSDR_SONAMES));
    }
}
