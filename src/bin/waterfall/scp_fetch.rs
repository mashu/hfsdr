//! Download MASTER.SCP from supercheckpartial.com into the app config dir.

use std::io::Read;
use std::path::PathBuf;

use hfsdr::{MasterScp, MASTER_SCP_URL};

pub fn download_master_scp() -> Result<PathBuf, String> {
    let dest = MasterScp::default_install_path().ok_or("config directory not found")?;
    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let agent = ureq::AgentBuilder::new().timeout(std::time::Duration::from_secs(30)).build();
    let resp = agent
        .get(MASTER_SCP_URL)
        .call()
        .map_err(|e| format!("download failed: {e}"))?;
    if !(200..300).contains(&resp.status()) {
        return Err(format!("HTTP {}", resp.status()));
    }
    let mut bytes = Vec::new();
    resp.into_reader()
        .read_to_end(&mut bytes)
        .map_err(|e| e.to_string())?;
    std::fs::write(&dest, &bytes).map_err(|e| e.to_string())?;
    Ok(dest)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_install_path_under_config() {
        let path = MasterScp::default_install_path();
        assert!(path.is_some());
        assert!(path.unwrap().ends_with("MASTER.SCP"));
    }
}
