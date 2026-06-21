//! Scroll-friendly control widgets for the side panel.

use std::ops::RangeInclusive;

use eframe::egui::{DragValue, Response, Slider, Ui};

fn scroll_steps(scroll_y: f32) -> i32 {
    if scroll_y > 2.0 {
        1
    } else if scroll_y < -2.0 {
        -1
    } else {
        0
    }
}

pub fn scroll_drag_f64(
    ui: &mut Ui,
    value: &mut f64,
    range: RangeInclusive<f64>,
    step: f64,
    suffix: &str,
) -> Response {
    let response = ui.add(
        DragValue::new(value)
            .range(range.clone())
            .speed(step)
            .suffix(suffix),
    );
    if response.hovered() {
        let steps = scroll_steps(ui.input(|i| i.smooth_scroll_delta.y));
        if steps != 0 {
            *value = (*value + steps as f64 * step).clamp(*range.start(), *range.end());
        }
    }
    response
}

pub fn scroll_slider_f32(ui: &mut Ui, value: &mut f32, range: RangeInclusive<f32>, label: &str) -> Response {
    let response = ui.add(Slider::new(value, range.clone()).text(label));
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 2.0 {
            let span = *range.end() - *range.start();
            let step = span / 120.0;
            let delta = if scroll > 0.0 { step } else { -step };
            *value = (*value + delta).clamp(*range.start(), *range.end());
        }
    }
    response
}

pub fn scroll_slider_log_f32(
    ui: &mut Ui,
    value: &mut f32,
    range: RangeInclusive<f32>,
    label: &str,
) -> Response {
    let response = ui.add(Slider::new(value, range.clone()).logarithmic(true).text(label));
    if response.hovered() {
        let scroll = ui.input(|i| i.smooth_scroll_delta.y);
        if scroll.abs() > 2.0 {
            let factor = if scroll > 0.0 { 0.94 } else { 1.06 };
            let new = (*value * factor).round();
            *value = new.clamp(*range.start(), *range.end());
        }
    }
    response
}
