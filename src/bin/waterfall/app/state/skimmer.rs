use std::sync::mpsc::Receiver;
use std::time::Instant;

use hfsdr::{SkimmerConfig, Spot, SpotSort};

#[derive(Debug)]
pub struct SkimmerUiState {
    pub skimmer_enabled: bool,
    pub skimmer: SkimmerConfig,
    pub skimmer_channels: usize,
    pub skimmer_spots: Vec<Spot>,
    pub spot_sort: SpotSort,
    pub continent_filter: bool,
    pub show_continents: [bool; 7],
    pub min_spot_snr: f32,
    pub spot_cq_only: bool,
    pub spot_hide_heard_labels: bool,
    pub spot_max_age_secs: f32,
    pub spot_callsign_filter: String,
    pub spot_label_limit: usize,
    pub scp_notice: Option<String>,
    pub scp_download_rx: Option<Receiver<Result<std::path::PathBuf, String>>>,
    pub scp_reload_pending: bool,
    pub scp_reload_deadline: Option<Instant>,
    pub last_scp_loaded: bool,
    pub frame_visible_spots: Vec<Spot>,
    pub skimmer_decode_channels: Vec<hfsdr::DecodeChannel>,
    /// New spots arrived from the engine since the last visible-spot refresh.
    pub spots_dirty: bool,
    /// Filter config used for the cached `frame_visible_spots`.
    pub last_spot_filter: Option<crate::spot_filter::SpotFilterConfig>,
    /// Last visible-spot refresh (age-based filters need periodic re-runs).
    pub last_spot_refresh: Option<Instant>,
    /// Cached callsign log (refreshed with the visible spots).
    pub callsign_log_cache: Vec<Spot>,
    /// Cached plot labels (offsets are center-relative, so retunes refresh).
    pub spot_label_cache: Vec<crate::widgets::SpotLabel>,
    pub last_label_center_hz: f64,
}
