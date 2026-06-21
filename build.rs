//! Locate libairspyhf when the `airspy` feature is enabled.

fn main() {
    if std::env::var_os("CARGO_FEATURE_AIRSPY").is_none() {
        return;
    }

    if pkg_config::Config::new()
        .cargo_metadata(true)
        .probe("libairspyhf")
        .is_ok()
    {
        return;
    }

    #[cfg(target_os = "macos")]
    if link_macos_brew_lib() {
        return;
    }

    panic!(
        "libairspyhf not found. Install the system library (e.g. libairspyhf-dev on Linux, \
         `brew install airspyhf` on macOS) or build with --no-default-features."
    );
}

#[cfg(target_os = "macos")]
fn link_macos_brew_lib() -> bool {
    for args in [&["--prefix", "airspyhf"][..], &["--prefix"][..]] {
        let Ok(output) = std::process::Command::new("brew").args(args).output() else {
            continue;
        };
        if !output.status.success() {
            continue;
        }
        let prefix = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let lib_dir = format!("{prefix}/lib");
        let has_lib = ["libairspyhf.dylib", "libairspyhf.a"]
            .iter()
            .any(|name| std::path::Path::new(&lib_dir).join(name).exists());
        if has_lib {
            println!("cargo:rustc-link-search=native={lib_dir}");
            return true;
        }
    }
    false
}
