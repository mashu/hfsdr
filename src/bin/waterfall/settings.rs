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
    pub kaiser_beta: f32,
    pub passband_flatten: bool,
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
    pub display_auto_track: bool,
    pub smooth_alpha: f32,
    pub target_fps: u32,
    pub fft_size: usize,
    pub fft_auto: bool,

    // Audio.
    pub audio_enabled: bool,
    pub volume: f32,

    // Skimmer / panels.
    pub skimmer_enabled: bool,
    pub skimmer_min_snr_db: f32,
    pub skimmer_min_decode_snr_db: f32,
    pub skimmer_decode_gate_ms: f32,
    pub skimmer_max_channels: usize,
    pub skimmer_bucket_hz: f32,
    pub skimmer_min_separation_bins: usize,
    pub skimmer_decoder: u8,
    pub skimmer_beam_width: usize,
    pub skimmer_lpf_cutoff_hz: f32,
    pub skimmer_target_audio_rate_hz: f32,
    pub skimmer_initial_wpm: f32,
    pub skimmer_thr_low: f32,
    pub skimmer_thr_high: f32,
    pub skimmer_channel_timeout_secs: f32,
    pub skimmer_store_max_age_secs: f32,
    pub skimmer_max_decode_chars: usize,
    pub min_spot_snr: f32,
    pub spot_cq_only: bool,
    pub spot_hide_heard_labels: bool,
    pub spot_max_age_secs: f32,
    pub spot_callsign_filter: String,
    pub spot_label_limit: usize,
    pub scp_require: bool,
    pub spot_sort: u8,
    pub continent_filter: bool,
    pub show_continents: [bool; 7],
    pub show_console: bool,
    pub filter_wide: bool,
    pub show_history: bool,
    pub show_left: bool,
    pub show_right: bool,

    // Connection memory.
    pub recent_hosts: Vec<ConnectRequest>,
    pub last_center_mhz: f64,

    // IQ capture / playback.
    pub iq_capture_dir: String,
    pub iq_playback_path: String,
}

impl Default for AppSettings {
    fn default() -> Self {
        Self {
            bfo_hz: 650.0,
            passband_hz: 200.0,
            window: 0,
            kaiser_beta: 6.0,
            passband_flatten: false,
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
            ref_db: -65.0,
            range_db: 17.0,
            display_auto_track: false,
            smooth_alpha: 0.09,
            target_fps: 30,
            fft_size: 2048,
            fft_auto: true,
            audio_enabled: true,
            volume: 1.0,
            skimmer_enabled: true,
            skimmer_min_snr_db: 10.0,
            skimmer_min_decode_snr_db: 8.0,
            skimmer_decode_gate_ms: 45.0,
            skimmer_max_channels: 16,
            skimmer_bucket_hz: 80.0,
            skimmer_min_separation_bins: 8,
            skimmer_decoder: 1,
            skimmer_beam_width: 12,
            skimmer_lpf_cutoff_hz: 120.0,
            skimmer_target_audio_rate_hz: 12_000.0,
            skimmer_initial_wpm: 22.0,
            skimmer_thr_low: 0.55,
            skimmer_thr_high: 0.72,
            skimmer_channel_timeout_secs: 8.0,
            skimmer_store_max_age_secs: 120.0,
            skimmer_max_decode_chars: 64,
            min_spot_snr: 12.0,
            spot_cq_only: false,
            spot_hide_heard_labels: true,
            spot_max_age_secs: 90.0,
            spot_callsign_filter: String::new(),
            spot_label_limit: 40,
            scp_require: true,
            spot_sort: 0,
            continent_filter: false,
            show_continents: [true; 7],
            show_console: false,
            filter_wide: false,
            show_history: true,
            show_left: true,
            show_right: true,
            recent_hosts: Vec::new(),
            last_center_mhz: 7.03,
            iq_capture_dir: String::new(),
            iq_playback_path: String::new(),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_roundtrip_json() {
        let s = AppSettings::default();
        let json = serde_json::to_string(&s).expect("serialize");
        let back: AppSettings = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.spot_sort, s.spot_sort);
        assert_eq!(back.continent_filter, s.continent_filter);
        assert_eq!(back.show_continents, s.show_continents);
        assert!(!back.show_console);
    }
}
