//! Locate and load optional SDR shared libraries at runtime (Windows, Linux, macOS).

use libloading::Library;
use std::path::PathBuf;

/// Candidate library names tried in order (newest ABI suffix first).
pub const AIRSPYHF_SONAMES: &[&str] = &[
    "libairspyhf.so.1",
    "libairspyhf.so",
    "libairspyhf.dylib",
    "libairspyhf.1.dylib",
    "airspyhf.dll",
    "libairspyhf.dll",
];

pub const RTLSDR_SONAMES: &[&str] = &[
    "librtlsdr.so.0",
    "librtlsdr.so",
    "librtlsdr.dylib",
    "librtlsdr.0.dylib",
    "rtlsdr.dll",
    "librtlsdr.dll",
];

pub const SOAPYSDR_SONAMES: &[&str] = &[
    "libSoapySDR.so.0.8",
    "libSoapySDR.so.0",
    "libSoapySDR.so",
    "libSoapySDR.dylib",
    "libSoapySDR.0.8.dylib",
    "SoapySDR.dll",
    "libSoapySDR.dll",
];

/// Return whether any candidate path can be opened.
pub fn can_load(sonames: &[&str]) -> bool {
    load(sonames).is_some()
}

/// Open the first matching library, searching bundled paths then the system loader.
pub fn load(sonames: &[&str]) -> Option<Library> {
    for path in search_paths(sonames) {
        if let Ok(lib) = unsafe { Library::new(&path) } {
            return Some(lib);
        }
    }
    None
}

fn search_paths(sonames: &[&str]) -> Vec<PathBuf> {
    let mut paths = Vec::new();
    for env in ["HFSDR_LIB_DIR", "HFSDR_DEPS_PREFIX"] {
        if let Ok(dir) = std::env::var(env) {
            let root = PathBuf::from(&dir);
            for name in sonames {
                paths.push(root.join(name));
                paths.push(root.join("lib").join(name));
                paths.push(root.join("bin").join(name));
            }
        }
    }
    if let Ok(exe) = std::env::current_exe() {
        if let Some(bin_dir) = exe.parent() {
            for name in sonames {
                paths.push(bin_dir.join(name));
                paths.push(bin_dir.join("lib").join(name));
                paths.push(bin_dir.join("../lib").join(name));
            }
        }
    }
    for name in sonames {
        paths.push(PathBuf::from(name));
    }
    paths
}

/// Load a required symbol from an opened library.
pub fn required_sym<T: Copy>(lib: &Library, name: &str) -> Option<T> {
    unsafe { lib.get(name.as_bytes()).ok().map(|s| *s) }
}

/// Load an optional symbol (e.g. libairspyhf >= 1.8 extensions).
pub fn optional_sym<T: Copy>(lib: &Library, name: &str) -> Option<T> {
    required_sym(lib, name)
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;

    #[test]
    fn search_paths_include_platform_sonames() {
        let paths = search_paths(AIRSPYHF_SONAMES);
        assert!(paths.iter().any(|p| p == Path::new("libairspyhf.so.1")));
        assert!(paths.iter().any(|p| p == Path::new("airspyhf.dll")));
    }
}
