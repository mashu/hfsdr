// Shared imports for `WaterfallApp` impl blocks.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::Color32;
use egui_extras::{Column, TableBuilder};
use hfsdr::{
    decimation_factor, compose_panadapter_row, panadapter_output_bins, stretch_row_to_width,
    strongest_offset_hz, Continent,
    ContinentResolver, AgcMode, ChannelFilterKind, ChannelOffsetHz, CwChannelSettings, RowFold, SlowWaterfall, SpectrumViewMapping, Spot,
    SpotKind, SpotSort, SkimmerConfig, SkimmerDecoderKind, channel_group_delay_ms, WindowKind,
    MAX_NOTCHES,
};
use hfsdr::kiwi::protocol::{man_gain_db_below_max, man_gain_from_db_below_max};

use crate::meters::{
    self, AfScopeParams, DualAgcParams, classify_level, rf_level_dbm, show_dual_agc_loop,
    show_status_rf_meter,
};
use crate::audio::AudioOutput;
use crate::colormap::db_to_colour;
use crate::controls::{
    preset_combo_f64, preset_combo_u32, scroll_slider_f32, scroll_slider_f32_step,
    scroll_slider_log_f32, vfo_wheel_khz,
};
use crate::display_levels::{
    display_levels_initialized_after_settings_load, estimate_levels, estimate_levels_from_rows,
    lock_display_levels_for_rf_tuning, should_auto_adjust_display_levels,
};
use crate::engine::{
    ConnState, EngineCommand, EngineHandle, EngineParams, EngineStats, FFT_SIZE, WATERFALL_ROWS,
};
use crate::ham_bands;
use crate::rf_view;
use crate::popup::{
    alert_banner, chip_row, configure_popup_window, ghost_button, inline_stats, list_row,
    popup_header, popup_scroll_body, popup_section, primary_button, secondary_button,
    segment_choice, PopupHeader,
};
use crate::iq_panel::{IqPanel, IqPanelCmd, IqPanelView};
use crate::interaction::{
    PlotAction, PlotFreqMapping, PlotInteraction, PlotViewState, RIT_MAX_HZ, RIT_MIN_HZ, NOTCH_WIDTH_MAX_HZ,
    NOTCH_WIDTH_MIN_HZ, suggest_notch_offset_hz,
};
use crate::interaction::{CW_PASSBAND_MAX_HZ, CW_PASSBAND_MIN_HZ, CW_PASSBAND_NARROW_MAX_HZ};
use crate::kiwi_directory::{GeoLocation, KiwiReceiver};
use crate::log;
use crate::pipeline_flow::{PipelineFlow, PipelineSnapshot, PipelineStage};
use crate::settings::{AppSettings, NotchData};
use crate::source::{AirspySettings, ConnectRequest, KiwiSettings, QmxSettings, RtlSdrSettings, SourceKind};
use crate::spot_filter::{
    build_spot_labels, continent_index, filter_spots, SpotFilterConfig, SpotLabelConfig,
};
use crate::theme::{
    apply, attach_rich_tooltip, band_lock_toggle, clickable_badge, collapsible_section,
    panel_toggle, section_card, section_frame, section_heading, section_heading_with_tip, section_hint,
    side_panel_frame, stat_row, stage_toggle, status_panel_frame, toggle, ACCENT, MUTED, OK, WARN,
};
use crate::widgets::{
    update_trace, PanadapterPlot, SpotLabel, TraceViewKey, SCOPE_HEIGHT,
};

use crate::source::{
    is_local_source, source_kind_from_index, source_kind_index, source_kind_label,
    source_kind_labels,
};
use crate::app::codec::{
    agc_mode_from_u8, agc_mode_to_u8, channel_filter_from_u8, channel_filter_to_u8,
    normalize_waterfall_avg, plot_action_changes_view, skimmer_config_from_settings,
    skimmer_decoder_from_u8, skimmer_decoder_to_u8, spot_sort_from_u8, spot_sort_to_u8,
    window_choice, window_from_u8, window_to_u8,
};
use crate::app::constants::*;
