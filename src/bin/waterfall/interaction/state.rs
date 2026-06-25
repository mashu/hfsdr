//! Plot view state and mouse interaction dispatch.

use eframe::egui::{Pos2, Rect, Response, Ui};
use hfsdr::ChannelOffsetHz;

use super::geometry::{
    classify_press, notch_width_from_edge, offset_hz_to_x, passband_from_edge, x_to_offset_hz,
};

const MIN_ZOOM: f32 = 0.04;

pub use hfsdr::{
    CHANNEL_PASSBAND_MAX_HZ as CW_PASSBAND_MAX_HZ,
    CHANNEL_PASSBAND_MIN_HZ as CW_PASSBAND_MIN_HZ,
    CHANNEL_PASSBAND_NARROW_MAX_HZ as CW_PASSBAND_NARROW_MAX_HZ,
};

pub const NOTCH_WIDTH_MIN_HZ: f32 = 10.0;
pub const NOTCH_WIDTH_MAX_HZ: f32 = 500.0;
/// Default spacing when arming another manual notch around the listen point.
pub const NOTCH_STAGGER_HZ: f32 = 80.0;
pub const NOTCH_MIN_SEPARATION_HZ: f32 = 40.0;
pub const RIT_MIN_HZ: f32 = -800.0;
pub const RIT_MAX_HZ: f32 = 800.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragMode {
    None,
    DragCenter,
    Tune,
    PanView,
    ResizeLeft,
    ResizeRight,
    ShiftPassband,
    DragNotch(usize),
    ResizeNotchLeft(usize),
    ResizeNotchRight(usize),
}

#[derive(Clone, Copy, Debug)]
pub struct NotchMarker {
    pub slot: usize,
    pub offset_hz: ChannelOffsetHz,
    pub width_hz: f32,
}

#[derive(Clone, Copy, Debug)]
pub enum PlotAction {
    TuneDeltaHz(f64),
    CenterOnOffsetHz(f64),
    SetTunePreviewOffsetHz(f64),
    CommitTunePreview,
    ClearTunePreview,
    PanViewDeltaHz(f64),
    ZoomView(f32),
    SetPassbandHz(f32),
    /// Move listen offset (RIT) without retuning the carrier.
    SetRitHz(f32),
    SetNotchOffset {
        slot: usize,
        offset_hz: ChannelOffsetHz,
    },
    SetNotchWidth {
        slot: usize,
        width_hz: f32,
    },
    /// Center the panadapter view at this offset (Hz relative to RX).
    SetViewPanHz(f64),
}

#[derive(Clone, Debug)]
pub struct PlotViewState {
    pub zoom: f32,
    pub pan_offset_hz: f64,
}

impl PlotViewState {
    pub fn new() -> Self {
        Self {
            zoom: 1.0,
            pan_offset_hz: 0.0,
        }
    }

    /// Visible span in Hz. `zoom` 1.0 = full IQ passband; `max_zoom` = CW band overview on Kiwi.
    pub fn view_span_hz(&self, full_span_hz: f32, max_zoom: f32) -> f32 {
        let cap = full_span_hz * max_zoom.max(1.0);
        (full_span_hz * self.zoom).clamp(full_span_hz * MIN_ZOOM, cap)
    }

    pub fn clamp_pan(&mut self, full_span_hz: f32, max_zoom: f32) {
        let view_span = self.view_span_hz(full_span_hz, max_zoom) as f64;
        let half_view = view_span / 2.0;
        let half_data = full_span_hz as f64 / 2.0;
        let max = if half_view > half_data {
            half_view - half_data
        } else {
            (half_data - half_view).max(0.0)
        };
        self.pan_offset_hz = self.pan_offset_hz.clamp(-max, max);
    }

    pub fn zoom_by(&mut self, factor: f32, full_span_hz: f32, max_zoom: f32) {
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, max_zoom.max(1.0));
        self.clamp_pan(full_span_hz, max_zoom);
    }

    /// Zoom to the CW band segment for the current center (default on startup / band pick).
    pub fn zoom_to_cw_segment(&mut self, segment_hz: f32, full_span_hz: f32, max_zoom: f32) {
        self.zoom = (segment_hz / full_span_hz).clamp(MIN_ZOOM, max_zoom.max(1.0));
        self.pan_offset_hz = 0.0;
        self.clamp_pan(full_span_hz, max_zoom);
    }

    /// Full IQ passband (widest data view).
    pub fn zoom_to_full_span(&mut self) {
        self.zoom = 1.0;
        self.pan_offset_hz = 0.0;
    }

    /// True when horizontal pan is useful (view narrower or wider than the IQ passband).
    pub fn can_pan(&self, full_span_hz: f32, max_zoom: f32) -> bool {
        let view_span = self.view_span_hz(full_span_hz, max_zoom) as f64;
        let half_view = view_span / 2.0;
        let half_data = full_span_hz as f64 / 2.0;
        (half_view - half_data).abs() > full_span_hz as f64 * 0.005
    }
}

pub struct PlotInteraction {
    pub drag_mode: DragMode,
    drag_origin: Option<Pos2>,
}

impl PlotInteraction {
    pub fn new() -> Self {
        Self {
            drag_mode: DragMode::None,
            drag_origin: None,
        }
    }

    /// True while the user is dragging on a plot (pan, tune, notch, etc.).
    pub fn is_dragging(&self) -> bool {
        !matches!(self.drag_mode, DragMode::None)
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        response: &Response,
        view: &mut PlotViewState,
        full_span_hz: f32,
        max_zoom: f32,
        display_view_span_hz: f32,
        display_pan_offset_hz: f64,
        passband_hz: f32,
        passband_min_hz: f32,
        passband_max_hz: f32,
        filter_editable: bool,
        listen_center_hz: f64,
        tune_preview_offset_hz: f64,
        notches: &[NotchMarker],
    ) -> Vec<PlotAction> {
        let mut actions = Vec::new();
        let view_span = display_view_span_hz;
        let pan = display_pan_offset_hz;
        let can_pan = view.can_pan(full_span_hz, max_zoom);
        let preview_x = offset_hz_to_x(tune_preview_offset_hz, rect, view_span, pan);
        let shift = ui.input(|i| i.modifiers.shift);
        let ctrl = ui.input(|i| i.modifiers.ctrl);

        if response.hovered() {
            let scroll = ui.input(|i| i.smooth_scroll_delta.y);
            if scroll != 0.0 {
                if ctrl && filter_editable {
                    let factor = if scroll > 0.0 { 0.94 } else { 1.06 };
                    let new_bw = (passband_hz * factor).round();
                    actions.push(PlotAction::SetPassbandHz(
                        new_bw.clamp(passband_min_hz, passband_max_hz),
                    ));
                } else if !ctrl {
                    let factor = if scroll > 0.0 { 1.12 } else { 0.88 };
                    actions.push(PlotAction::ZoomView(factor));
                }
            }
        }

        // Click-to-tune: let egui decide click vs drag (distance + time aware) so a
        // steady-enough tap always jumps, instead of a brittle pixel threshold that
        // turned small hand movement into a pan when zoomed in.
        if (response.clicked() || response.double_clicked()) && !self.is_dragging() {
            if let Some(pos) = response.interact_pointer_pos() {
                let mode = classify_press(
                    pos,
                    rect,
                    view_span,
                    pan,
                    passband_hz,
                    filter_editable,
                    listen_center_hz,
                    preview_x,
                    shift,
                    notches,
                );
                // Tune on empty-spectrum clicks only; handles (edges, center, notches) need drags.
                if matches!(
                    mode,
                    DragMode::Tune | DragMode::PanView | DragMode::ShiftPassband
                ) {
                    let offset = x_to_offset_hz(pos.x, rect, view_span, pan);
                    actions.push(PlotAction::CenterOnOffsetHz(offset));
                }
            }
        }

        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.drag_origin = Some(pos);
                self.drag_mode = classify_press(
                    pos,
                    rect,
                    view_span,
                    pan,
                    passband_hz,
                    filter_editable,
                    listen_center_hz,
                    preview_x,
                    shift,
                    notches,
                );
            }
        }

        if response.dragged() {
            match self.drag_mode {
                DragMode::DragCenter => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let offset = x_to_offset_hz(pos.x, rect, view_span, pan);
                        actions.push(PlotAction::SetTunePreviewOffsetHz(offset));
                    }
                }
                DragMode::Tune => {
                    // Drag on empty spectrum: pan the view when zoomed, otherwise
                    // walk the carrier. Pure taps are handled by `clicked()` above.
                    let delta_hz =
                        -response.drag_delta().x as f64 / rect.width() as f64 * view_span as f64;
                    if delta_hz.abs() > f64::EPSILON {
                        if can_pan {
                            actions.push(PlotAction::PanViewDeltaHz(delta_hz));
                        } else {
                            actions.push(PlotAction::TuneDeltaHz(delta_hz));
                        }
                    }
                }
                DragMode::PanView => {
                    let delta_hz =
                        -response.drag_delta().x as f64 / rect.width() as f64 * view_span as f64;
                    if delta_hz.abs() > f64::EPSILON {
                        actions.push(PlotAction::PanViewDeltaHz(delta_hz));
                    }
                }
                DragMode::ResizeLeft | DragMode::ResizeRight => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let offset = x_to_offset_hz(pos.x, rect, view_span, pan);
                        let bw = passband_from_edge(
                            listen_center_hz,
                            offset,
                            passband_min_hz,
                            passband_max_hz,
                        );
                        actions.push(PlotAction::SetPassbandHz(bw));
                    }
                }
                DragMode::ShiftPassband => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let offset = x_to_offset_hz(pos.x, rect, view_span, pan);
                        let rit = (offset - tune_preview_offset_hz) as f32;
                        actions.push(PlotAction::SetRitHz(rit.clamp(RIT_MIN_HZ, RIT_MAX_HZ)));
                    }
                }
                DragMode::DragNotch(slot) => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let offset = ChannelOffsetHz::from_plot_hz(
                            x_to_offset_hz(pos.x, rect, view_span, pan) as f32,
                        );
                        actions.push(PlotAction::SetNotchOffset { slot, offset_hz: offset });
                    }
                }
                DragMode::ResizeNotchLeft(slot) | DragMode::ResizeNotchRight(slot) => {
                    if let (Some(pos), Some(n)) = (
                        response.interact_pointer_pos(),
                        notches.iter().find(|n| n.slot == slot),
                    ) {
                        let edge = x_to_offset_hz(pos.x, rect, view_span, pan);
                        let width = notch_width_from_edge(n.offset_hz, edge);
                        actions.push(PlotAction::SetNotchWidth { slot, width_hz: width });
                    }
                }
                DragMode::None => {}
            }
        }

        if response.drag_stopped() {
            match self.drag_mode {
                DragMode::DragCenter => actions.push(PlotAction::CommitTunePreview),
                DragMode::ResizeLeft | DragMode::ResizeRight => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let offset = x_to_offset_hz(pos.x, rect, view_span, pan);
                        let bw = passband_from_edge(
                            listen_center_hz,
                            offset,
                            passband_min_hz,
                            passband_max_hz,
                        );
                        actions.push(PlotAction::SetPassbandHz(bw));
                    }
                    actions.push(PlotAction::ClearTunePreview);
                }
                DragMode::ResizeNotchLeft(slot) | DragMode::ResizeNotchRight(slot) => {
                    if let (Some(pos), Some(n)) = (
                        response.interact_pointer_pos(),
                        notches.iter().find(|n| n.slot == slot),
                    ) {
                        let edge = x_to_offset_hz(pos.x, rect, view_span, pan);
                        let width = notch_width_from_edge(n.offset_hz, edge);
                        actions.push(PlotAction::SetNotchWidth { slot, width_hz: width });
                    }
                    actions.push(PlotAction::ClearTunePreview);
                }
                _ => actions.push(PlotAction::ClearTunePreview),
            }
            self.drag_mode = DragMode::None;
            self.drag_origin = None;
        }

        actions
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pan_available_when_zoomed_in() {
        let mut view = PlotViewState::new();
        view.zoom = 0.25;
        assert!(view.can_pan(12_000.0, 1.0));
        view.zoom = 1.0;
        assert!(!view.can_pan(12_000.0, 1.0));
        view.zoom = 5.0;
        assert!(view.can_pan(12_000.0, 6.0));
    }

    #[test]
    fn view_span_clamps_to_passband() {
        let view = PlotViewState {
            zoom: 0.01,
            pan_offset_hz: 0.0,
        };
        let span = view.view_span_hz(12_000.0, 1.0);
        assert!((span - 12_000.0 * 0.04).abs() < 1.0);

        let wide = PlotViewState {
            zoom: 10.0,
            pan_offset_hz: 0.0,
        };
        assert!((wide.view_span_hz(12_000.0, 2.0) - 24_000.0).abs() < 1.0);
    }

    #[test]
    fn clamp_pan_limits_offset() {
        let mut view = PlotViewState {
            zoom: 0.25,
            pan_offset_hz: 50_000.0,
        };
        view.clamp_pan(12_000.0, 1.0);
        assert!(view.pan_offset_hz.abs() < 5_000.0);
    }

    #[test]
    fn zoom_by_and_reset_full_span() {
        let mut view = PlotViewState::new();
        view.zoom_by(0.5, 12_000.0, 4.0);
        assert!((view.zoom - 0.5).abs() < 1e-5);
        view.zoom_to_cw_segment(3_000.0, 12_000.0, 4.0);
        assert!((view.zoom - 0.25).abs() < 1e-5);
        assert_eq!(view.pan_offset_hz, 0.0);
        view.zoom_to_full_span();
        assert_eq!(view.zoom, 1.0);
    }

    #[test]
    fn plot_interaction_dragging_flag() {
        let mut interaction = PlotInteraction::new();
        assert!(!interaction.is_dragging());
        interaction.drag_mode = DragMode::PanView;
        assert!(interaction.is_dragging());
    }
}
