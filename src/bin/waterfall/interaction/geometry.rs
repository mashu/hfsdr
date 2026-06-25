//! Plot geometry: frequency ↔ pixel mapping and hit testing.

use eframe::egui::{Pos2, Rect};
use hfsdr::ChannelOffsetHz;

use super::state::{DragMode, NotchMarker, NOTCH_MIN_SEPARATION_HZ, NOTCH_STAGGER_HZ, NOTCH_WIDTH_MAX_HZ, NOTCH_WIDTH_MIN_HZ};

const CENTER_GRAB_PX: f32 = 18.0;
const EDGE_GRAB_PX: f32 = 12.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum NotchHit {
    Left,
    Right,
    Body,
}

pub fn x_to_offset_hz(x: f32, rect: Rect, span_hz: f32, pan_offset_hz: f64) -> f64 {
    let t = ((x - rect.left()) / rect.width()).clamp(0.0, 1.0) as f64;
    hfsdr::view_t_to_offset_hz(t, span_hz, pan_offset_hz)
}

pub fn offset_hz_to_x(offset_hz: f64, rect: Rect, span_hz: f32, pan_offset_hz: f64) -> f32 {
    let t = hfsdr::offset_hz_to_view_t(offset_hz, span_hz, pan_offset_hz);
    rect.left() + rect.width() * t as f32
}

/// Shared frequency ↔ horizontal pixel mapping for scope, axis, and waterfall.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PlotFreqMapping {
    pub view_span_hz: f32,
    pub pan_offset_hz: f64,
    pub storage_span_hz: f32,
}

impl PlotFreqMapping {
    pub fn new(view_span_hz: f32, pan_offset_hz: f64, storage_span_hz: f32) -> Self {
        Self {
            view_span_hz,
            pan_offset_hz,
            storage_span_hz,
        }
    }

    pub fn x_to_offset(&self, x: f32, rect: Rect) -> f64 {
        x_to_offset_hz(x, rect, self.view_span_hz, self.pan_offset_hz)
    }

    pub fn offset_to_x(&self, offset_hz: f64, rect: Rect) -> f32 {
        offset_hz_to_x(offset_hz, rect, self.view_span_hz, self.pan_offset_hz)
    }
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

pub fn notch_width_from_edge(center: ChannelOffsetHz, edge_offset_hz: f64) -> f32 {
    (2.0 * (edge_offset_hz - center.hz() as f64).abs() as f32)
        .clamp(NOTCH_WIDTH_MIN_HZ, NOTCH_WIDTH_MAX_HZ)
}

/// Suggested channel offset when the user arms a manual notch (listen point + stagger).
pub fn suggest_notch_offset_hz(
    listen_offset_hz: ChannelOffsetHz,
    other_offsets: &[ChannelOffsetHz],
) -> ChannelOffsetHz {
    if other_offsets.is_empty() {
        return ChannelOffsetHz::new(listen_offset_hz.hz() + NOTCH_STAGGER_HZ);
    }

    for step in 1..=4 {
        for sign in [1.0_f32, -1.0] {
            let candidate =
                ChannelOffsetHz::new(listen_offset_hz.hz() + sign * step as f32 * NOTCH_STAGGER_HZ);
            if other_offsets
                .iter()
                .all(|&o| (candidate.hz() - o.hz()).abs() >= NOTCH_MIN_SEPARATION_HZ)
            {
                return candidate;
            }
        }
    }

    let nearest = other_offsets
        .iter()
        .min_by(|a, b| {
            (a.hz() - listen_offset_hz.hz())
                .abs()
                .total_cmp(&(b.hz() - listen_offset_hz.hz()).abs())
        })
        .copied()
        .unwrap_or(listen_offset_hz);
    let mirrored = ChannelOffsetHz::new(2.0 * listen_offset_hz.hz() - nearest.hz());
    if other_offsets
        .iter()
        .all(|&o| (mirrored.hz() - o.hz()).abs() >= NOTCH_MIN_SEPARATION_HZ)
    {
        return mirrored;
    }

    let extreme = other_offsets
        .iter()
        .fold(listen_offset_hz, |acc, &o| {
            if (o.hz() - listen_offset_hz.hz()).abs() > (acc.hz() - listen_offset_hz.hz()).abs() {
                o
            } else {
                acc
            }
        });
    if extreme.hz() >= listen_offset_hz.hz() {
        ChannelOffsetHz::new(extreme.hz() + NOTCH_STAGGER_HZ)
    } else {
        ChannelOffsetHz::new(extreme.hz() - NOTCH_STAGGER_HZ)
    }
}

pub fn center_grab_px() -> f32 {
    CENTER_GRAB_PX
}

pub fn edge_grab_px() -> f32 {
    EDGE_GRAB_PX
}

fn in_passband_body(x: f32, left: f32, right: f32) -> bool {
    x > left + EDGE_GRAB_PX && x < right - EDGE_GRAB_PX
}

/// Single source of truth for what a press at `pos` targets. Shared by click-to-tune
/// and drag-start so the two can never disagree about what was hit.
#[allow(clippy::too_many_arguments)]
pub(super) fn classify_press(
    pos: Pos2,
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
    passband_hz: f32,
    filter_editable: bool,
    listen_center_hz: f64,
    preview_x: f32,
    shift: bool,
    notches: &[NotchMarker],
) -> DragMode {
    if let Some((slot, hit)) = pick_notch_hit(pos.x, rect, view_span_hz, pan_offset_hz, notches) {
        return match hit {
            NotchHit::Left => DragMode::ResizeNotchLeft(slot),
            NotchHit::Right => DragMode::ResizeNotchRight(slot),
            NotchHit::Body => DragMode::DragNotch(slot),
        };
    }

    let near_center =
        pos.x >= preview_x - CENTER_GRAB_PX && pos.x <= preview_x + CENTER_GRAB_PX;

    if filter_editable {
        let (left, right) =
            filter_edges(rect, view_span_hz, pan_offset_hz, listen_center_hz, passband_hz);
        if pos.x >= left - EDGE_GRAB_PX && pos.x <= left + EDGE_GRAB_PX {
            return DragMode::ResizeLeft;
        }
        if pos.x >= right - EDGE_GRAB_PX && pos.x <= right + EDGE_GRAB_PX {
            return DragMode::ResizeRight;
        }
        if near_center {
            return DragMode::DragCenter;
        }
        if in_passband_body(pos.x, left, right) {
            return DragMode::ShiftPassband;
        }
    } else if near_center {
        return DragMode::DragCenter;
    }

    if shift {
        DragMode::PanView
    } else {
        DragMode::Tune
    }
}

fn pick_notch_hit(
    x: f32,
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
    notches: &[NotchMarker],
) -> Option<(usize, NotchHit)> {
    let mut best: Option<(usize, NotchHit, f32)> = None;
    for n in notches {
        let half = n.width_hz as f64 / 2.0;
        let center = n.offset_hz.hz() as f64;
        let left = offset_hz_to_x(center - half, rect, view_span_hz, pan_offset_hz);
        let right = offset_hz_to_x(center + half, rect, view_span_hz, pan_offset_hz);
        let cx = offset_hz_to_x(center, rect, view_span_hz, pan_offset_hz);

        let mut consider = |part: NotchHit, dist: f32| {
            if best.is_none_or(|(_, _, d)| dist < d) {
                best = Some((n.slot, part, dist));
            }
        };

        if x >= left - EDGE_GRAB_PX && x <= left + EDGE_GRAB_PX {
            consider(NotchHit::Left, (x - left).abs());
        }
        if x >= right - EDGE_GRAB_PX && x <= right + EDGE_GRAB_PX {
            consider(NotchHit::Right, (x - right).abs());
        }
        if x > left + EDGE_GRAB_PX && x < right - EDGE_GRAB_PX {
            consider(NotchHit::Body, (x - cx).abs());
        }
    }
    best.map(|(slot, part, _)| (slot, part))
}

#[cfg(test)]
mod tests {
    use super::*;
    use eframe::egui::Pos2;

    #[test]
    fn edge_resize_uses_listen_center() {
        let bw = passband_from_edge(200.0, 50.0, 50.0, 500.0);
        assert!((bw - 300.0).abs() < 1.0);
        let bw = passband_from_edge(-100.0, 150.0, 50.0, 500.0);
        assert!((bw - 500.0).abs() < 1.0);
    }

    #[test]
    fn x_to_offset_maps_plot_edges_to_span() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1000.0, 100.0));
        assert!((x_to_offset_hz(500.0, rect, 70_000.0, 0.0) - 0.0).abs() < 1.0);
        assert!((x_to_offset_hz(0.0, rect, 70_000.0, 0.0) - (-35_000.0)).abs() < 1.0);
        assert!((x_to_offset_hz(1000.0, rect, 70_000.0, 0.0) - 35_000.0).abs() < 1.0);
    }

    #[test]
    fn offset_x_roundtrip_at_band_overview() {
        let rect = Rect::from_min_max(Pos2::new(0.0, 0.0), Pos2::new(1000.0, 100.0));
        let view_span = 70_000.0;
        let pan = 0.0;
        for offset in [-3_000.0, -500.0, 0.0, 750.0, 2_800.0] {
            let x = offset_hz_to_x(offset, rect, view_span, pan);
            let back = x_to_offset_hz(x, rect, view_span, pan);
            assert!(
                (back - offset).abs() < 1.0,
                "offset {offset} -> x {x} -> {back}"
            );
        }
    }

    #[test]
    fn notch_width_from_edge_symmetric() {
        let w = notch_width_from_edge(ChannelOffsetHz::new(100.0), 150.0);
        assert!((w - 100.0).abs() < 0.1);
    }

    #[test]
    fn passband_body_between_edges() {
        assert!(in_passband_body(50.0, 10.0, 90.0));
        assert!(!in_passband_body(15.0, 10.0, 90.0));
    }

    #[test]
    fn suggest_notch_first_staggers_from_listen() {
        assert_eq!(
            suggest_notch_offset_hz(ChannelOffsetHz::new(120.0), &[]),
            ChannelOffsetHz::new(200.0)
        );
    }

    #[test]
    fn suggest_notch_staggers_from_listen() {
        let o = suggest_notch_offset_hz(ChannelOffsetHz::ZERO, &[ChannelOffsetHz::ZERO]);
        assert_eq!(o, ChannelOffsetHz::new(80.0));
        let o = suggest_notch_offset_hz(
            ChannelOffsetHz::ZERO,
            &[ChannelOffsetHz::ZERO, ChannelOffsetHz::new(80.0)],
        );
        assert_eq!(o, ChannelOffsetHz::new(-80.0));
    }

    #[test]
    fn suggest_notch_mirrors_across_listen() {
        let o = suggest_notch_offset_hz(
            ChannelOffsetHz::new(100.0),
            &[ChannelOffsetHz::new(180.0)],
        );
        assert_eq!(o, ChannelOffsetHz::new(20.0));
    }

    #[test]
    fn suggest_notch_extends_when_cluster_full() {
        let o = suggest_notch_offset_hz(
            ChannelOffsetHz::ZERO,
            &[
                ChannelOffsetHz::new(80.0),
                ChannelOffsetHz::new(-80.0),
                ChannelOffsetHz::new(160.0),
                ChannelOffsetHz::new(-160.0),
            ],
        );
        assert_eq!(o, ChannelOffsetHz::new(240.0));
    }
}
