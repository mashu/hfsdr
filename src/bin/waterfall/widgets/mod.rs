//! Interactive spectrum + waterfall rendering.

mod filter_band;
mod freq_axis;
mod overview;
mod smooth;
mod spot_labels;
mod trace;
mod waterfall;
mod waterfall_mesh;

use eframe::egui::{Painter, Rect, Sense, Ui, Vec2};

use crate::interaction::{NotchMarker, PlotAction, PlotFreqMapping, PlotInteraction, PlotViewState};

use self::filter_band::{draw_filter_band, draw_notch_marker};
use self::freq_axis::{draw_center_line, draw_freq_vertical_grid, show_freq_axis_bar};
use self::overview::draw_band_overview;
use self::spot_labels::draw_spot_labels;
use self::trace::{draw_db_scale, draw_plot_background, draw_trace};
use self::waterfall::draw_waterfall_layer;

pub use self::spot_labels::SpotLabel;
pub use self::trace::{update_trace, TraceViewKey};

/// Shared rendering/interaction parameters for the RF plots.
///
/// Bundling these keeps the widget API small and is the natural seam for the
/// future node-graph compositor (one struct describes what a plot shows).
pub const SCOPE_HEIGHT: f32 = 200.0;

pub struct PlotParams<'a> {
    /// Visible panadapter width at zoom 1.0 (Kiwi IQ passband; equals IQ rate on wideband SDRs).
    pub view_bandwidth_hz: f32,
    /// Maximum zoom-out factor (CW band overview / full_span); 1.0 on wideband SDRs.
    pub max_zoom: f32,
    /// Visible span and pan aligned with composed spectrum/waterfall rows (see [`PlotFreqMapping`]).
    /// Tuned carrier (Hz) — used for absolute MHz/kHz axis labels.
    pub center_freq_hz: f64,
    pub passband_hz: f32,
    /// -3 dB channel half-width for plot overlay (from [`hfsdr::build_filter_overlay`]).
    pub channel_half_hz: f32,
    pub overlay_audio_rate: f32,
    pub filter_settings: &'a hfsdr::CwChannelSettings,
    pub passband_min_hz: f32,
    pub passband_max_hz: f32,
    pub filter_editable: bool,
    pub filter_center_hz: f64,
    pub vfo_offset_hz: f64,
    pub notches: &'a [NotchMarker],
    pub labels: &'a [SpotLabel],
    pub trace: &'a [f32],
    /// Full-span trace for the optional band overview minimap (IQ passband).
    pub overview_trace: &'a [f32],
    pub overview_span_hz: f32,
    pub show_overview: bool,
    pub ref_db: f32,
    pub range_db: f32,
    /// Scope trace height (frequency axis is a separate row below).
    pub height: f32,
    /// Shared plot column width — scope, axis, and waterfall must match.
    pub plot_width: f32,
    /// Viewport waterfall texture (same frequency mapping as the scope trace).
    pub waterfall_display: Option<&'a eframe::egui::TextureHandle>,
    /// Ring-buffer write head (`next` row index); draw uses fixed screen UV mapping.
    pub waterfall_row_head: usize,
}

pub struct PanadapterPlot;

impl PanadapterPlot {
    pub fn new() -> Self {
        Self
    }

    /// Scope + frequency axis + waterfall with one shared frequency map and one interaction target.
    pub fn show(
        &mut self,
        ui: &mut Ui,
        interaction: &mut PlotInteraction,
        view: &mut PlotViewState,
        freq_map: PlotFreqMapping,
        p: &PlotParams,
        hover_out: &mut Option<f64>,
        plot_rect_out: &mut Option<Rect>,
    ) -> Vec<PlotAction> {
        let full_span = p.view_bandwidth_hz.max(1.0);
        let max_zoom = p.max_zoom.max(1.0);
        let view_span = freq_map.view_span_hz;
        let pan = freq_map.pan_offset_hz;
        let plot_w = p.plot_width.max(1.0);

        let (scope_response, scope_painter) = ui.allocate_painter(
            Vec2::new(plot_w, p.height),
            Sense::empty(),
        );
        let scope_rect = scope_response.rect;
        draw_scope_layer(
            &scope_painter,
            scope_rect,
            freq_map,
            p,
        );

        let axis_rect = show_freq_axis_bar(
            ui,
            plot_w,
            view_span,
            pan,
            p.center_freq_hz,
            hover_out,
        );

        let wf_height = ui.available_height().max(120.0);
        let (wf_response, wf_painter) =
            ui.allocate_painter(Vec2::new(plot_w, wf_height), Sense::empty());
        let wf_rect = wf_response.rect;
        draw_waterfall_layer(&wf_painter, wf_rect, freq_map, p);

        let interaction_rect = scope_rect.union(axis_rect).union(wf_rect);
        *plot_rect_out = Some(interaction_rect);
        let interact_id = ui.id().with("panadapter_interact");
        let response = ui.interact(interaction_rect, interact_id, Sense::click_and_drag());

        let mut actions = interaction.handle(
            ui,
            interaction_rect,
            &response,
            view,
            full_span,
            max_zoom,
            view_span,
            pan,
            p.passband_hz,
            p.passband_min_hz,
            p.passband_max_hz,
            crate::interaction::FilterOverlayContext {
                channel_half_hz: p.channel_half_hz,
                audio_rate: p.overlay_audio_rate,
            },
            p.filter_settings,
            p.filter_editable,
            p.filter_center_hz,
            p.notches,
        );

        if p.show_overview && !p.overview_trace.is_empty() {
            actions.extend(draw_band_overview(
                ui,
                &scope_painter,
                scope_rect,
                full_span,
                p.overview_span_hz,
                view_span,
                pan,
                p.overview_trace,
                p.ref_db,
                p.range_db,
            ));
        }

        if let Some(pos) = ui.input(|i| i.pointer.hover_pos()) {
            if interaction_rect.contains(pos) {
                *hover_out = Some(freq_map.x_to_offset(pos.x, interaction_rect));
            } else if hover_out.is_some() {
                *hover_out = None;
            }
        }

        actions
    }
}

fn draw_scope_layer(
    painter: &Painter,
    rect: Rect,
    freq_map: PlotFreqMapping,
    p: &PlotParams,
) {
    let view_span = freq_map.view_span_hz;
    let pan = freq_map.pan_offset_hz;

    draw_plot_background(painter, rect);

    if p.filter_editable {
                draw_filter_band(
            painter,
            rect,
            view_span,
            pan,
            p.filter_center_hz,
            p.channel_half_hz,
            true,
        );
    }

    for notch in p.notches {
        draw_notch_marker(
            painter,
            rect,
            view_span,
            pan,
            notch.slot,
            notch.offset_hz.hz(),
            notch.display_half_hz,
            true,
        );
    }

    draw_db_scale(painter, rect, p.ref_db, p.range_db);
    draw_freq_vertical_grid(painter, rect, view_span, pan);
    draw_trace(painter, rect, p.trace, p.ref_db, p.range_db);
    draw_center_line(
        painter,
        rect,
        view_span,
        pan,
        p.vfo_offset_hz,
        false,
    );

    draw_spot_labels(painter, rect, view_span, pan, p.labels);
}
