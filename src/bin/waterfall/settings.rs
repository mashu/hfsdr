//! Persisted application settings (DSP + display + performance + recent hosts).
//!
//! Stored as JSON at `dirs::config_dir()/hfsdr/settings.json`. Every field is
//! `#[serde(default)]` so older/newer files load gracefully. The conversions to
//! and from live app/DSP state live in `app.rs` (which owns the private fields).

use serde::{Deserialize, Serialize};

use crate::source::ConnectRequest;

const APP_DIR: &str = "hfsdr";
const FILE: &str = "settings.json";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct NotchData {
    pub enabled: bool,
    pub offset_hz: f32,
    pub width_hz: f32,
}

impl Default for NotchData {
    fn default() -> Self {
        Self {
            enabled: false,
            offset_hz: 0.0,
            width_hz: 50.0,
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct AppSettings {
    // CW demod / filter chain.
    pub bfo_hz: f32,
    pub passband_hz: f32,
    pub window: u8,
    pub decimation: u32,
    pub squelch: f32,
    pub nb_enabled: bool,
    pub nb_threshold: f32,
    pub nb_width: u32,
    pub an_enabled: bool,
    pub an_guard_hz: f32,
    pub an_rate: f32,
    pub apf_enabled: bool,
    pub apf_width_hz: f32,
    pub apf_gain: f32,
    pub nr_enabled: bool,
    pub nr_level: f32,
    pub agc_enabled: bool,
    pub agc_target: f32,
    pub agc_attack_ms: f32,
    pub agc_decay_ms: f32,
    pub agc_manual_gain: f32,
    pub notches: Vec<NotchData>,

    // Receiver controls.
    pub rit_hz: f32,
    pub pitch_lock: bool,
    pub agc_rf_on: bool,

    // Display + performance.
    pub ref_db: f32,
    pub range_db: f32,
    pub smooth_alpha: f32,
    pub target_fps: u32,
    pub fft_size: usize,

    // Audio.
    pub audio_enabled: bool,
    pub volume: f32,

    // Skimmer / panels.
    pub skimmer_enabled: bool,
    pub min_spot_snr: f32,
    pub show_history: bool,
    pub show_left: bool,
    pub show_right: bool,

    // Connection memory.
    pub recent_hosts: Vec<ConnectRequest>,
    pub last_center_mhz: f64,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            bfo_hz: 650.0,
            passband_hz: 200.0,
            window: 0,
            decimation: 0,
            squelch: 0.0,
            nb_enabled: false,
            nb_threshold: 6.0,
            nb_width: 6,
            an_enabled: false,
            an_guard_hz: 120.0,
            an_rate: 0.02,
            apf_enabled: false,
            apf_width_hz: 120.0,
            apf_gain: 1.5,
            nr_enabled: false,
            nr_level: 0.3,
            agc_enabled: true,
            agc_target: 0.25,
            agc_attack_ms: 3.0,
            agc_decay_ms: 120.0,
            agc_manual_gain: 1.0,
            notches: vec![NotchData::default(); 4],
            rit_hz: 0.0,
            pitch_lock: false,
            agc_rf_on: true,
            ref_db: -70.0,
            range_db: 55.0,
            smooth_alpha: 0.09,
            target_fps: 30,
            fft_size: 2048,
            audio_enabled: true,
            volume: 1.0,
            skimmer_enabled: false,
            min_spot_snr: 0.0,
            show_history: false,
            show_left: true,
            show_right: true,
            recent_hosts: Vec::new(),
            last_center_mhz: 7.03,
        }
    }
}

fn settings_path() -> Option<std::path::PathBuf> {
    let mut dir = dirs::config_dir()?;
    dir.push(APP_DIR);
    Some(dir.join(FILE))
}

impl AppSettings {
    /// Load persisted settings, falling back to defaults on any error.
    pub fn load() -> Self {
        let Some(path) = settings_path() else {
            return Self::default();
        };
        match std::fs::read_to_string(&path) {
            Ok(text) => serde_json::from_str(&text).unwrap_or_default(),
            Err(_) => Self::default(),
        }
    }

    /// Persist settings; errors are swallowed (best-effort).
    pub fn save(&self) {
        let Some(path) = settings_path() else {
            return;
        };
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(text) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, text);
        }
    }
}
