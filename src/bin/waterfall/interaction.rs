//! Mouse interaction for RF plots: tune, zoom, pan view.

use eframe::egui::{Pos2, Rect, Response, Ui};

const CENTER_GRAB_PX: f32 = 18.0;
const EDGE_GRAB_PX: f32 = 12.0;
const MIN_ZOOM: f32 = 0.04;
const DRAG_TUNE_THRESHOLD_PX: f32 = 4.0;

pub const CW_PASSBAND_MIN_HZ: f32 = 50.0;
/// CW contest filters top out around 500 Hz; wide mode allows RTTY-adjacent widths.
pub const CW_PASSBAND_MAX_HZ: f32 = 2_000.0;
pub const CW_PASSBAND_NARROW_MAX_HZ: f32 = 500.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragMode {
    None,
    DragCenter,
    Tune,
    PanView,
    ResizeLeft,
    ResizeRight,
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

    pub fn view_span_hz(&self, sample_rate: f32) -> f32 {
        (sample_rate * self.zoom).clamp(sample_rate * MIN_ZOOM, sample_rate)
    }

    pub fn clamp_pan(&mut self, sample_rate: f32) {
        let half_view = self.view_span_hz(sample_rate) as f64 / 2.0;
        let half_iq = sample_rate as f64 / 2.0;
        let max = (half_iq - half_view).max(0.0);
        self.pan_offset_hz = self.pan_offset_hz.clamp(-max, max);
    }

    pub fn zoom_by(&mut self, factor: f32, sample_rate: f32) {
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, 1.0);
        self.clamp_pan(sample_rate);
    }

    /// True when the visible span is narrower than the IQ passband (horizontal pan useful).
    pub fn can_pan(&self, sample_rate: f32) -> bool {
        self.view_span_hz(sample_rate) < sample_rate * 0.995
    }
}

pub struct PlotInteraction {
    pub drag_mode: DragMode,
    drag_origin: Option<Pos2>,
    tune_drag_active: bool,
}

impl PlotInteraction {
    pub fn new() -> Self {
        Self {
            drag_mode: DragMode::None,
            drag_origin: None,
            tune_drag_active: false,
        }
    }

    #[allow(clippy::too_many_arguments)]
    pub fn handle(
        &mut self,
        ui: &mut Ui,
        rect: Rect,
        response: &Response,
        view: &mut PlotViewState,
        sample_rate: f32,
        passband_hz: f32,
        passband_min_hz: f32,
        passband_max_hz: f32,
        filter_editable: bool,
        listen_center_hz: f64,
        _tune_preview_offset_hz: f64,
    ) -> Vec<PlotAction> {
        let mut actions = Vec::new();
        let view_span = view.view_span_hz(sample_rate);
        let pan = view.pan_offset_hz;
        let center_x = offset_hz_to_x(listen_center_hz, rect, view_span, pan);
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

        if response.double_clicked() {
            if let Some(pos) = response.interact_pointer_pos() {
                let offset = x_to_offset_hz(pos.x, rect, view_span, pan);
                actions.push(PlotAction::CenterOnOffsetHz(offset));
            }
        }

        if response.drag_started() {
            if let Some(pos) = response.interact_pointer_pos() {
                self.drag_origin = Some(pos);
                self.tune_drag_active = false;
                let can_pan = view.can_pan(sample_rate);
                if pos.x >= center_x - CENTER_GRAB_PX && pos.x <= center_x + CENTER_GRAB_PX {
                    self.drag_mode = DragMode::DragCenter;
                } else if filter_editable {
                    let (left, right) =
                        filter_edges(rect, view_span, pan, listen_center_hz, passband_hz);
                    if pos.x >= left - EDGE_GRAB_PX && pos.x <= left + EDGE_GRAB_PX {
                        self.drag_mode = DragMode::ResizeLeft;
                    } else if pos.x >= right - EDGE_GRAB_PX && pos.x <= right + EDGE_GRAB_PX {
                        self.drag_mode = DragMode::ResizeRight;
                    } else if shift || can_pan {
                        self.drag_mode = DragMode::PanView;
                    } else {
                        self.drag_mode = DragMode::Tune;
                    }
                } else if shift || can_pan {
                    self.drag_mode = DragMode::PanView;
                } else {
                    self.drag_mode = DragMode::Tune;
                }
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
                    let pos = response.interact_pointer_pos();
                    if let (Some(origin), Some(pos)) = (self.drag_origin, pos) {
                        if !self.tune_drag_active && pos.distance(origin) < DRAG_TUNE_THRESHOLD_PX {
                            // Wait for click vs drag threshold.
                        } else {
                            self.tune_drag_active = true;
                            let delta_hz =
                                -response.drag_delta().x as f64 / rect.width() as f64 * view_span as f64;
                            if delta_hz.abs() > f64::EPSILON {
                                actions.push(PlotAction::TuneDeltaHz(delta_hz));
                            }
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
                DragMode::None => {}
            }
        }

        if response.drag_stopped() {
            match self.drag_mode {
                DragMode::DragCenter => actions.push(PlotAction::CommitTunePreview),
                DragMode::Tune if !self.tune_drag_active => {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let offset = x_to_offset_hz(pos.x, rect, view_span, pan);
                        actions.push(PlotAction::CenterOnOffsetHz(offset));
                    }
                }
                _ => actions.push(PlotAction::ClearTunePreview),
            }
            self.drag_mode = DragMode::None;
            self.drag_origin = None;
            self.tune_drag_active = false;
        }

        actions
    }
}

pub fn x_to_offset_hz(x: f32, rect: Rect, span_hz: f32, pan_offset_hz: f64) -> f64 {
    let t = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0);
    pan_offset_hz + (t as f64 - 0.5) * span_hz as f64
}

pub fn offset_hz_to_x(offset_hz: f64, rect: Rect, span_hz: f32, pan_offset_hz: f64) -> f32 {
    let rel = offset_hz - pan_offset_hz;
    let t = rel / span_hz as f64 + 0.5;
    rect.left() + rect.width() * t as f32
}

pub fn filter_edges(
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
    listen_center_hz: f64,
    passband_hz: f32,
) -> (f32, f32) {
    let half = passband_hz / 2.0;
    (
        offset_hz_to_x(listen_center_hz - half as f64, rect, view_span_hz, pan_offset_hz),
        offset_hz_to_x(listen_center_hz + half as f64, rect, view_span_hz, pan_offset_hz),
    )
}

pub fn format_offset_label(offset_hz: f64) -> String {
    if offset_hz.abs() >= 1000.0 {
        format!("{:.2} kHz", offset_hz / 1000.0)
    } else {
        format!("{:.0} Hz", offset_hz)
    }
}

/// Absolute carrier frequency for axis labels.
pub fn format_freq_hz(freq_hz: f64) -> String {
    if freq_hz.abs() >= 1_000_000.0 {
        format!("{:.3}", freq_hz / 1_000_000.0)
    } else if freq_hz.abs() >= 10_000.0 {
        format!("{:.1}k", freq_hz / 1_000.0)
    } else {
        format!("{:.0}", freq_hz)
    }
}

/// Nice major tick spacing for a frequency axis of the given span (Hz).
pub fn nice_freq_step_hz(span_hz: f32) -> f32 {
    if span_hz <= 0.0 {
        return 1000.0;
    }
    let targets = [5.0_f32, 4.0, 6.0];
    let mut best = span_hz / 5.0;
    for &n in &targets {
        let raw = span_hz / n;
        let exp = raw.log10().floor();
        let mag = 10f32.powf(exp);
        let norm = raw / mag;
        let nice = if norm <= 1.0 {
            1.0
        } else if norm <= 2.0 {
            2.0
        } else if norm <= 5.0 {
            5.0
        } else {
            10.0
        };
        let step = nice * mag;
        if (span_hz / step).round() >= 3.0 {
            best = step;
            break;
        }
        best = step;
    }
    best.max(1.0)
}

/// Passband width from dragging a filter edge at `edge_offset_hz` relative to tuned center.
pub fn passband_from_edge(
    listen_center_hz: f64,
    edge_offset_hz: f64,
    passband_min_hz: f32,
    passband_max_hz: f32,
) -> f32 {
    let half = (edge_offset_hz - listen_center_hz).abs() as f32;
    (half * 2.0).clamp(passband_min_hz, passband_max_hz)
}

pub fn center_grab_px() -> f32 {
    CENTER_GRAB_PX
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn edge_resize_uses_listen_center() {
        let bw = passband_from_edge(200.0, 50.0, 50.0, 500.0);
        assert!((bw - 300.0).abs() < 1.0);
        let bw = passband_from_edge(-100.0, 150.0, 50.0, 500.0);
        assert!((bw - 500.0).abs() < 1.0);
    }

    #[test]
    fn pan_available_when_zoomed_in() {
        let mut view = PlotViewState::new();
        view.zoom = 0.25;
        assert!(view.can_pan(12_000.0));
        view.zoom = 1.0;
        assert!(!view.can_pan(12_000.0));
    }

    #[test]
    fn nice_freq_step_splits_span() {
        let step = nice_freq_step_hz(3_000.0);
        assert!(step >= 200.0 && step <= 1_500.0);
        assert!((3_000.0 / step).round() >= 2.0);
    }
}
