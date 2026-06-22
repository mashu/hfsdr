//! Super Check Partial (`MASTER.SCP`) callsign dictionary.
//!
//! Plain-text file: one callsign per line (N1MM+/Win-Test format). Used to reject
//! decoder garbage and complete unique partial matches.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

pub const MASTER_SCP_URL: &str = "https://www.supercheckpartial.com/MASTER.SCP";

/// Loaded SCP database plus lookup indexes.
#[derive(Clone, Debug)]
pub struct MasterScp {
    calls: HashSet<String>,
    /// First three characters → all calls with that prefix (for partial match).
    prefix3: HashMap<String, Vec<String>>,
    version: Option<String>,
    path: Option<PathBuf>,
}

impl Default for MasterScp {
    fn default() -> Self {
        Self::empty()
    }
}

impl MasterScp {
    pub fn empty() -> Self {
        Self {
            calls: HashSet::new(),
            prefix3: HashMap::new(),
            version: None,
            path: None,
        }
    }

    pub fn is_loaded(&self) -> bool {
        !self.calls.is_empty()
    }

    pub fn is_empty(&self) -> bool {
        self.calls.is_empty()
    }

    pub fn len(&self) -> usize {
        self.calls.len()
    }

    pub fn version(&self) -> Option<&str> {
        self.version.as_deref()
    }

    pub fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// Parse SCP file contents (one call per line).
    pub fn from_text(text: &str) -> Self {
        let mut scp = Self::empty();
        for line in text.lines() {
            scp.ingest_line(line);
        }
        scp.build_prefix_index();
        scp
    }

    pub fn from_file(path: &Path) -> std::io::Result<Self> {
        let text = std::fs::read_to_string(path)?;
        let mut scp = Self::from_text(&text);
        scp.path = Some(path.to_path_buf());
        Ok(scp)
    }

    /// Search common install locations; returns the first readable file.
    pub fn discover() -> Self {
        for path in Self::search_paths() {
            if let Ok(scp) = Self::from_file(&path) {
                if scp.is_loaded() {
                    return scp;
                }
            }
        }
        Self::empty()
    }

    pub fn search_paths() -> Vec<PathBuf> {
        let mut paths = Vec::new();
        if let Ok(custom) = std::env::var("HFSDR_MASTER_SCP") {
            paths.push(PathBuf::from(custom));
        }
        if let Some(dir) = dirs::config_dir() {
            paths.push(dir.join("hfsdr").join("MASTER.SCP"));
            paths.push(dir.join("hamradio").join("MASTER.SCP"));
        }
        if let Some(docs) = dirs::document_dir() {
            paths.push(
                docs.join("N1MM Logger+")
                    .join("UserFiles")
                    .join("MASTER.SCP"),
            );
            paths.push(docs.join("N1MM Logger").join("UserFiles").join("MASTER.SCP"));
        }
        if let Ok(home) = std::env::var("HOME") {
            let home = PathBuf::from(home);
            paths.push(home.join("MASTER.SCP"));
            paths.push(home.join(".config").join("hamradio").join("MASTER.SCP"));
        }
        paths.push(PathBuf::from("MASTER.SCP"));
        paths
    }

    /// Default install path for downloads: `~/.config/hfsdr/MASTER.SCP` (Linux).
    pub fn default_install_path() -> Option<PathBuf> {
        dirs::config_dir().map(|d| d.join("hfsdr").join("MASTER.SCP"))
    }

    /// Validate a decoded token against SCP.
    ///
    /// Returns the canonical callsign from the database (exact or unique prefix
    /// completion). Portable calls use the longest `/` segment before lookup.
    pub fn resolve(&self, token: &str) -> Option<String> {
        let core = normalize_call_token(token)?;
        if self.calls.contains(&core) {
            return Some(core);
        }
        if core.len() < 3 {
            return None;
        }
        let key = prefix_key(&core);
        let bucket = self.prefix3.get(&key)?;
        let matches: Vec<&String> = bucket
            .iter()
            .filter(|c| c.starts_with(&core))
            .collect();
        if matches.len() == 1 {
            return Some(matches[0].clone());
        }
        None
    }

    fn ingest_line(&mut self, line: &str) {
        let t = line.trim();
        if t.is_empty() {
            return;
        }
        let upper = t.to_ascii_uppercase();
        if upper.starts_with('#') || upper.starts_with("!!") {
            return;
        }
        if upper.starts_with("VER") && upper.len() >= 7 && upper[3..].chars().all(|c| c.is_ascii_digit())
        {
            self.version = Some(upper);
            return;
        }
        if let Some(call) = normalize_call_token(&upper) {
            self.calls.insert(call);
        }
    }

    fn build_prefix_index(&mut self) {
        self.prefix3.clear();
        for call in &self.calls {
            if call.len() >= 3 {
                self.prefix3
                    .entry(prefix_key(call))
                    .or_default()
                    .push(call.clone());
            }
        }
        for bucket in self.prefix3.values_mut() {
            bucket.sort();
        }
    }
}

fn normalize_call_token(token: &str) -> Option<String> {
    let t = token.trim().to_ascii_uppercase();
    if t.is_empty() {
        return None;
    }
    let core = t
        .split('/')
        .max_by_key(|s| s.len())
        .unwrap_or(t.as_str())
        .to_string();
    if core.len() < 3 || core.len() > 12 {
        return None;
    }
    if !core.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    let mut has_alpha = false;
    let mut has_digit = false;
    for c in core.chars() {
        if c.is_ascii_alphabetic() {
            has_alpha = true;
        }
        if c.is_ascii_digit() {
            has_digit = true;
        }
    }
    if has_alpha && has_digit {
        Some(core)
    } else {
        None
    }
}

fn prefix_key(call: &str) -> String {
    let end = call.len().min(3);
    call[..end].to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
VER20260202
W1AW
OH2BH
K3LR
G0ABC
";

    #[test]
    fn loads_calls_and_version() {
        let scp = MasterScp::from_text(SAMPLE);
        assert_eq!(scp.len(), 4);
        assert_eq!(scp.version(), Some("VER20260202"));
    }

    #[test]
    fn exact_match() {
        let scp = MasterScp::from_text(SAMPLE);
        assert_eq!(scp.resolve("oh2bh"), Some("OH2BH".into()));
        assert_eq!(scp.resolve("W1AW/4"), Some("W1AW".into()));
    }

    #[test]
    fn unique_prefix_completion() {
        let scp = MasterScp::from_text(SAMPLE);
        assert_eq!(scp.resolve("OH2"), Some("OH2BH".into()));
        assert_eq!(scp.resolve("W1A"), Some("W1AW".into()));
    }

    #[test]
    fn ambiguous_prefix_rejected() {
        let scp = MasterScp::from_text("VER20240101\nK1AA\nK1AB\n");
        assert!(scp.resolve("K1A").is_none());
    }

    #[test]
    fn garbage_rejected() {
        let scp = MasterScp::from_text(SAMPLE);
        assert!(scp.resolve("5B63BO").is_none());
        assert!(scp.resolve("TI6I6").is_none());
    }
}
