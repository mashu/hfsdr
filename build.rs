//! Locate native SDR libraries when optional features are enabled.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(airspyhf_extended_api)");

    if std::env::var_os("CARGO_FEATURE_AIRSPY").is_some() {
        probe_or_panic(
            "libairspyhf",
            "libairspyhf not found. Install the system library (e.g. libairspyhf-dev on Linux, \
             `brew install airspyhf` on macOS) or build with --no-default-features.",
            &["airspyhf", ""],
            &["libairspyhf.dylib", "libairspyhf.a"],
        );
        if std::env::var_os("DOCS_RS").is_some() || airspyhf_has_extended_api() {
            println!("cargo:rustc-cfg=airspyhf_extended_api");
        }
    }

    if std::env::var_os("CARGO_FEATURE_RTLSDR").is_some() {
        probe_or_panic(
            "librtlsdr",
            "librtlsdr not found. Install the system library (e.g. librtlsdr-dev on Linux, \
             `brew install librtlsdr` on macOS) or disable the `rtlsdr` feature.",
            &["librtlsdr", ""],
            &["librtlsdr.dylib", "librtlsdr.a"],
        );
    }
}

fn probe_or_panic(
    pkg: &str,
    panic_msg: &str,
    macos_brew_formulas: &[&str],
    macos_lib_names: &[&str],
) {
    if pkg_config::Config::new()
        .cargo_metadata(true)
        .probe(pkg)
        .is_ok()
    {
        return;
    }

    #[cfg(target_os = "macos")]
    if link_macos_brew_lib(macos_brew_formulas, macos_lib_names) {
        return;
    }

    #[cfg(not(target_os = "macos"))]
    let _ = (macos_brew_formulas, macos_lib_names);

    panic!("{panic_msg}");
}

#[cfg(target_os = "macos")]
fn link_macos_brew_lib(formulas: &[&str], lib_names: &[&str]) -> bool {
    for formula in formulas {
        let args: &[&str] = if formula.is_empty() {
            &["--prefix"]
        } else {
            &["--prefix", formula]
        };
        let Ok(output) = std::process::Command::new("brew").args(args).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let lib_dir = format!("{prefix}/lib");
        let has_lib = lib_names
            .iter()
            .any(|name| std::path::Path::new(&lib_dir).join(name).exists());
        if has_lib {
            println!("cargo:rustc-link-search=native={lib_dir}");
            return true;
        }
    }
    false
}

/// `airspyhf_set_bias_tee` / `airspyhf_set_frontend_options` were added in libairspyhf 1.8.
/// Older distro/brew packages (e.g. 1.6.x) lack them; probe so we can link without those symbols.
fn airspyhf_has_extended_api() -> bool {
    airspyhf_lib_path().is_some_and(|path| {
        has_symbol(&path, "airspyhf_set_bias_tee")
            && has_symbol(&path, "airspyhf_set_frontend_options")
    })
}

fn airspyhf_lib_path() -> Option<PathBuf> {
    if let Ok(lib) = pkg_config::Config::new().probe("libairspyhf") {
        for dir in lib.link_paths {
            if let Some(path) = find_lib_in_dir(&dir, "airspyhf") {
                return Some(path);
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        for formula in ["airspyhf", ""] {
            if let Some(dir) = brew_lib_dir(formula) {
                if let Some(path) = find_lib_in_dir(&dir, "airspyhf") {
                    return Some(path);
                }
            }
        }
    }

    for dir in [
        "/usr/lib/x86_64-linux-gnu",
        "/usr/lib/aarch64-linux-gnu",
        "/usr/lib",
        "/usr/local/lib",
    ] {
        if let Some(path) = find_lib_in_dir(dir, "airspyhf") {
            return Some(path);
        }
    }

    None
}

fn find_lib_in_dir(dir: impl AsRef<Path>, name: &str) -> Option<PathBuf> {
    for suffix in [".so", ".dylib", ".dll", ".a"] {
        let path = dir.as_ref().join(format!("lib{name}{suffix}"));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

fn has_symbol(lib: &Path, sym: &str) -> bool {
    for args in [
        vec!["-D", lib.to_str().unwrap_or_default()],
        vec!["-gU", lib.to_str().unwrap_or_default()],
        vec![lib.to_str().unwrap_or_default()],
    ] {
        let Ok(output) = Command::new("nm").args(&args).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let text = String::from_utf8_lossy(&output.stdout);
        if text.lines().any(|line| {
            line.contains(sym)
                && (line.contains(" T ") || line.contains(" W ") || line.ends_with(&format!(" T {sym}")))
        }) {
            return true;
        }
    }
    false
}

#[cfg(target_os = "macos")]
fn brew_lib_dir(formula: &str) -> Option<PathBuf> {
    let args: &[&str] = if formula.is_empty() {
        &["--prefix"]
    } else {
        &["--prefix", formula]
    };
    let output = Command::new("brew").args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
    Some(PathBuf::from(format!("{prefix}/lib")))
}
