//! Runtime availability of optional native SDR libraries.
//!
//! On Windows the build uses `/DELAYLOAD` so missing `airspyhf.dll` / `rtlsdr.dll`
//! do not prevent startup; call [`init`] once at launch, then gate UI and connect
//! paths with [`airspy_available`] / [`rtlsdr_available`].

use std::sync::OnceLock;

struct State {
    #[cfg(feature = "airspy")]
    airspy: bool,
    #[cfg(feature = "rtlsdr")]
    rtlsdr: bool,
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

fn state() -> &'static State {
    STATE.get_or_init(|| State {
        #[cfg(feature = "airspy")]
        airspy: probe_lib("airspyhf"),
        #[cfg(feature = "rtlsdr")]
        rtlsdr: probe_lib("rtlsdr"),
    })
}

/// On Windows, check whether the DLL is loadable from the exe directory / PATH.
/// Elsewhere, compile-time linking already succeeded when the feature is enabled.
#[cfg_attr(not(any(feature = "airspy", feature = "rtlsdr")), allow(dead_code))]
fn probe_lib(stem: &str) -> bool {
    #[cfg(windows)]
    {
        for name in [format!("{stem}.dll"), format!("lib{stem}.dll")] {
            if dll_loadable(&name) {
                return true;
            }
        }
        false
    }
    #[cfg(not(windows))]
    {
        let _ = stem;
        true
    }
}

#[cfg(windows)]
fn dll_loadable(name: &str) -> bool {
    use std::ffi::c_void;
    use std::os::raw::c_int;
    use std::os::windows::ffi::OsStrExt;

    #[link(name = "kernel32")]
    extern "system" {
        fn LoadLibraryW(name: *const u16) -> *mut c_void;
        fn FreeLibrary(module: *mut c_void) -> c_int;
    }

    let wide: Vec<u16> = std::ffi::OsStr::new(name)
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    unsafe {
        let handle = LoadLibraryW(wide.as_ptr());
        if handle.is_null() {
            return false;
        }
        FreeLibrary(handle);
        true
    }
}
