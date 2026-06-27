// Shared imports for `WaterfallApp` impl blocks.

pub(crate) use std::collections::HashSet;
pub(crate) use std::collections::VecDeque;
pub(crate) use std::sync::mpsc::Receiver;
pub(crate) use std::time::{Duration, Instant};

pub(crate) use eframe::egui;
pub(crate) use egui::Color32;
pub(crate) use egui_extras::{Column, TableBuilder};
pub(crate) use hfsdr::{
    decimation_factor, compose_panadapter_row, panadapter_output_bins, stretch_row_to_width,
    strongest_offset_hz, Continent,
    ContinentResolver, AgcMode, ChannelFilterKind, ChannelOffsetHz, CwChannelSettings, RowFold, SlowWaterfall, SpectrumViewMapping, Spot,
    SpotKind, SpotSort, SkimmerConfig, SkimmerDecoderKind, channel_group_delay_ms, IirFilterKind,
    SidetoneEnvelopeShape, WindowKind,
    MAX_KAISER_BETA, MAX_NOTCHES, MIN_KAISER_BETA,
};
pub(crate) use hfsdr::kiwi::protocol::{man_gain_db_below_max, man_gain_from_db_below_max};

pub(crate) use crate::meters::{
    self, AfScopeParams, DualAgcParams, MeterSmoothed, classify_level, dbm_to_needle_t,
    rf_level_dbm, show_dual_agc_loop, show_status_rf_meter,
};
pub(crate) use crate::audio::AudioOutput;
pub(crate) use crate::colormap::db_to_colour;
pub(crate) use crate::controls::{
    preset_combo_f64, preset_combo_u32, filter_shift_control, rit_control, scroll_slider_f32,
    scroll_slider_f32_step,
    scroll_slider_log_f32, vfo_wheel_khz,
};
pub(crate) use crate::display_levels::{
    display_levels_initialized_after_settings_load, estimate_levels, estimate_levels_from_rows,
    lock_display_levels_for_rf_tuning, should_auto_adjust_display_levels,
};
pub(crate) use crate::engine::{
    ConnState, EngineCommand, EngineHandle, EngineParams, EnginePoll, EngineStats, FFT_SIZE,
    WATERFALL_ROWS,
};
pub(crate) use crate::ham_bands;
pub(crate) use crate::rf_view;
pub(crate) use crate::popup::{
    alert_banner, band_preset_grid, chip_row, configure_popup_window, ghost_button, inline_stats, list_row,
    popup_body_max_height, popup_header, popup_scroll_body, popup_section, primary_button,
    secondary_button,
    preset_segment_f32, segment_choice, segment_choice_sized, labeled_segment_choice, status_pill,
    truncate_middle, PopupHeader,
};
pub(crate) use crate::iq_panel::{IqPanel, IqPanelCmd, IqPanelView};
pub(crate) use crate::interaction::{
    PlotAction, PlotFreqMapping, PlotInteraction, PlotViewState, RIT_MAX_HZ, RIT_MIN_HZ, NOTCH_WIDTH_MAX_HZ,
    NOTCH_WIDTH_MIN_HZ, suggest_notch_offset_hz,
};
pub(crate) use crate::interaction::{CW_PASSBAND_MAX_HZ, CW_PASSBAND_MIN_HZ, CW_PASSBAND_NARROW_MAX_HZ};
pub(crate) use crate::kiwi_directory::{GeoLocation, KiwiReceiver};
pub(crate) use crate::log;
pub(crate) use crate::pipeline_flow::{PipelineFlow, PipelineSnapshot, PipelineStage};
pub(crate) use crate::settings::{AppSettings, NotchData};
pub(crate) use crate::source::{AirspySettings, ConnectRequest, KiwiSettings, QmxSettings, RtlSdrSettings, SourceKind};
pub(crate) use crate::spot_filter::{
    build_spot_labels, continent_index, filter_spots, SpotFilterConfig, SpotLabelConfig,
};
pub(crate) use crate::theme::{
    apply, attach_rich_tooltip, band_lock_toggle, clickable_badge, collapsible_section,
    panel_toggle, section_card, section_frame, section_heading, section_heading_with_tip, section_hint,
    side_panel_frame, stat_row, stage_toggle, status_panel_frame, toggle, ACCENT, MUTED, OK, WARN,
};
pub(crate) use crate::widgets::{
    update_trace, PanadapterPlot, SpotLabel, TraceViewKey, SCOPE_HEIGHT,
};

pub(crate) use crate::source::{
    is_local_source, sanitize_source_kind, source_kind_from_index, source_kind_index,
    source_kind_label, source_kind_labels,
};
pub(crate) use crate::app::codec::{
    agc_mode_from_u8, agc_mode_to_u8, channel_filter_from_u8, channel_filter_to_u8,
    iir_filter_from_u8, iir_filter_to_u8,
    normalize_waterfall_avg, plot_action_changes_view, skimmer_config_from_settings,
    skimmer_decoder_from_u8, skimmer_decoder_to_u8, spot_sort_from_u8, spot_sort_to_u8,
    st_envelope_shape_from_u8, st_envelope_shape_to_u8,
    window_from_u8, window_to_u8,
};
pub(crate) use crate::app::constants::*;
