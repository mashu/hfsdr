//! Locate native SDR libraries when optional features are enabled.

fn main() {
    if std::env::var_os("CARGO_FEATURE_AIRSPY").is_some() {
        probe_or_panic(
            "libairspyhf",
            "libairspyhf not found. Install the system library (e.g. libairspyhf-dev on Linux, \
             `brew install airspyhf` on macOS) or build with --no-default-features.",
            &["libairspyhf.dylib", "libairspyhf.a"],
        );
    }

    if std::env::var_os("CARGO_FEATURE_RTLSDR").is_some() {
        probe_or_panic(
            "librtlsdr",
            "librtlsdr not found. Install the system library (e.g. librtlsdr-dev on Linux, \
             `brew install librtlsdr` on macOS) or disable the `rtlsdr` feature.",
            &["librtlsdr.dylib", "librtlsdr.a"],
        );
    }
}

fn probe_or_panic(pkg: &str, panic_msg: &str, macos_lib_names: &[&str]) {
    if pkg_config::Config::new()
        .cargo_metadata(true)
        .probe(pkg)
        .is_ok()
    {
        return;
    }

    #[cfg(target_os = "macos")]
    if link_macos_brew_lib(macos_lib_names) {
        return;
    }

    panic!("{panic_msg}");
}

#[cfg(target_os = "macos")]
fn link_macos_brew_lib(lib_names: &[&str]) -> bool {
    for args in [&["--prefix"][..]] {
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
