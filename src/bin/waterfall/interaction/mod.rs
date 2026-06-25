//! Mouse interaction for RF plots: tune, zoom, pan view, filter/notch editing.

mod format;
mod geometry;
mod state;

pub use format::{
    format_absolute_freq_hz, format_freq_hz, format_offset_label, nice_freq_step_hz,
};
pub use geometry::{
    center_grab_px, edge_grab_px, filter_edges, notch_width_from_edge, offset_hz_to_x,
    passband_from_edge, suggest_notch_offset_hz, x_to_offset_hz, PlotFreqMapping,
};
pub use state::{
    DragMode, NotchMarker, PlotAction, PlotInteraction, PlotViewState, CW_PASSBAND_MAX_HZ,
    CW_PASSBAND_MIN_HZ, CW_PASSBAND_NARROW_MAX_HZ, NOTCH_MIN_SEPARATION_HZ, NOTCH_STAGGER_HZ,
    NOTCH_WIDTH_MAX_HZ, NOTCH_WIDTH_MIN_HZ, RIT_MAX_HZ, RIT_MIN_HZ,
};
