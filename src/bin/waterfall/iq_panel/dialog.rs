//! Native folder/file pickers for IQ capture and playback.

use std::path::{Path, PathBuf};

#[cfg(feature = "gui-core")]
fn dialog_start_dir(path: &Path) -> &Path {
    if path.is_dir() {
        path
    } else if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        parent
    } else {
        path
    }
}

/// Pick a directory for new IQ captures.
///
/// Browser builds have no filesystem to point at, so the pickers return None
/// and the caller keeps its existing path.
#[cfg(not(feature = "gui-core"))]
pub fn pick_capture_dir(_current: &Path) -> Option<PathBuf> {
    None
}

#[cfg(not(feature = "gui-core"))]
pub fn pick_playback_file(_start_dir: &Path) -> Option<PathBuf> {
    None
}

#[cfg(feature = "gui-core")]
pub fn pick_capture_dir(current: &Path) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("IQ capture folder")
        .set_directory(dialog_start_dir(current))
        .pick_folder()
}

/// Pick a `.hiq.gz` capture file for playback.
#[cfg(feature = "gui-core")]
pub fn pick_playback_file(start_dir: &Path) -> Option<PathBuf> {
    rfd::FileDialog::new()
        .set_title("Open IQ capture")
        .add_filter("IQ capture", &["hiq.gz", "gz"])
        .set_directory(dialog_start_dir(start_dir))
        .pick_file()
}
