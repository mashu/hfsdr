use eframe::egui::{Align2, FontId, Painter, Pos2, Rect, Stroke};

use crate::interaction::offset_hz_to_x;
use crate::theme::{OK, WARN};

/// A decoded-signal label floated above its spectral peak.
#[derive(Clone, Debug)]
pub struct SpotLabel {
    pub offset_hz: f32,
    pub text: String,
    pub cq: bool,
    pub snr_db: f32,
}

pub(crate) fn draw_spot_labels(
    painter: &Painter,
    rect: Rect,
    view_span_hz: f32,
    pan_offset_hz: f64,
    labels: &[SpotLabel],
) {
    const CHAR_W: f32 = 6.5;
    const ROW_H: f32 = 13.0;
    const MIN_GAP: f32 = 3.0;
    const MAX_ROWS: u8 = 3;

    let mut placed: Vec<(f32, f32, u8)> = Vec::new();
    let mut sorted: Vec<&SpotLabel> = labels.iter().collect();
    sorted.sort_by(|a, b| {
        b.snr_db
            .total_cmp(&a.snr_db)
            .then_with(|| a.offset_hz.total_cmp(&b.offset_hz))
    });

    for label in sorted {
        let x = offset_hz_to_x(label.offset_hz as f64, rect, view_span_hz, pan_offset_hz);
        if x < rect.left() || x > rect.right() {
            continue;
        }
        let half_w = label.text.len() as f32 * CHAR_W * 0.5;
        let left = x - half_w;
        let right = x + half_w;

        let mut row = 0u8;
        'rows: while row < MAX_ROWS {
            let overlaps = placed.iter().any(|(pl, pr, r)| {
                *r == row && left < *pr + MIN_GAP && right > *pl - MIN_GAP
            });
            if !overlaps {
                break 'rows;
            }
            row += 1;
        }
        if row >= MAX_ROWS {
            continue;
        }
        placed.push((left, right, row));

        let y = rect.top() + 11.0 + row as f32 * ROW_H;
        let color = if label.cq { WARN } else { OK };
        painter.line_segment(
            [Pos2::new(x, rect.top()), Pos2::new(x, rect.top() + 8.0 + row as f32 * ROW_H)],
            Stroke::new(1.5, color),
        );
        painter.text(
            Pos2::new(x, y),
            Align2::CENTER_TOP,
            &label.text,
            FontId::proportional(11.0),
            color,
        );
    }
}
