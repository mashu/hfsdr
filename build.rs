//! Locate native SDR libraries when optional features are enabled.

use std::path::{Path, PathBuf};
use std::process::Command;

fn main() {
    println!("cargo::rustc-check-cfg=cfg(airspyhf_extended_api)");
    println!("cargo:rerun-if-env-changed=HFSDR_DEPS_PREFIX");
    println!("cargo:rerun-if-env-changed=VCPKG_ROOT");
    println!("cargo:rerun-if-env-changed=VCPKG_INSTALLED_DIR");
    println!("cargo:rerun-if-env-changed=PKG_CONFIG_PATH");

    if std::env::var_os("CARGO_FEATURE_GUI_CORE").is_some() {
        let manifest_dir = PathBuf::from(std::env::var("CARGO_MANIFEST_DIR").unwrap_or_default());
        let methods = manifest_dir.join("src/bin/waterfall/app/methods");
        println!("cargo:rerun-if-changed={}", methods.display());
    }

    if std::env::var_os("CARGO_FEATURE_AIRSPY").is_some() {
        probe_or_panic(
            "libairspyhf",
            "libairspyhf not found. Install the system library (e.g. libairspyhf-dev on Linux, \
             `brew install airspyhf` on macOS, or `pwsh scripts/install-windows-sdr-deps.ps1` \
             on Windows) or build with --no-default-features.",
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
             `brew install librtlsdr` on macOS, or `pwsh scripts/install-windows-sdr-deps.ps1` \
             on Windows) or disable the `rtlsdr` feature.",
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

    #[cfg(target_os = "windows")]
    if link_windows_lib(pkg) {
        return;
    }

    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = (macos_brew_formulas, macos_lib_names);

    panic!("{panic_msg}");
}

#[cfg(target_os = "windows")]
fn link_windows_lib(pkg: &str) -> bool {
    let lib_name = match pkg {
        "libairspyhf" => "airspyhf",
        "librtlsdr" => "rtlsdr",
        _ => return false,
    };
    for lib_dir in windows_lib_dirs() {
        if find_lib_in_dir(&lib_dir, lib_name).is_some() {
            println!("cargo:rustc-link-search=native={}", lib_dir.display());
            println!("cargo:rustc-link-lib=dylib={lib_name}");
            return true;
        }
    }
    false
}

#[cfg(target_os = "windows")]
fn windows_lib_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(deps) = std::env::var("HFSDR_DEPS_PREFIX") {
        let prefix = PathBuf::from(&deps);
        dirs.push(prefix.join("lib"));
        dirs.push(prefix.join("bin"));
    }
    if let Ok(vcpkg) = std::env::var("VCPKG_ROOT") {
        let installed = PathBuf::from(vcpkg).join("installed/x64-windows");
        dirs.push(installed.join("lib"));
        dirs.push(installed.join("bin"));
    }
    if let Ok(installed) = std::env::var("VCPKG_INSTALLED_DIR") {
        let root = PathBuf::from(installed);
        dirs.push(root.join("lib"));
        dirs.push(root.join("bin"));
    }
    dirs
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

    #[cfg(target_os = "windows")]
    for dir in windows_lib_dirs() {
        if let Some(path) = find_lib_in_dir(&dir, "airspyhf") {
            return Some(path);
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
    for stem in [format!("lib{name}"), name.to_string()] {
        for suffix in [".so", ".dylib", ".dll", ".a", ".lib"] {
            let path = dir.as_ref().join(format!("{stem}{suffix}"));
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

fn has_symbol(lib: &Path, sym: &str) -> bool {
    #[cfg(target_os = "windows")]
    {
        return has_symbol_dumpbin(lib, sym);
    }

    #[cfg(not(target_os = "windows"))]
    {
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
                    && (line.contains(" T ")
                        || line.contains(" W ")
                        || line.ends_with(&format!(" T {sym}")))
            }) {
                return true;
            }
        }
        false
    }
}

#[cfg(target_os = "windows")]
fn has_symbol_dumpbin(lib: &Path, sym: &str) -> bool {
    let lib_path = lib.to_str().unwrap_or_default();
    let Ok(output) = Command::new("dumpbin").args(["/exports", lib_path]).output() else {
        return false;
    };
    if !output.status.success() {
        return false;
    }
    String::from_utf8_lossy(&output.stdout).contains(sym)
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
