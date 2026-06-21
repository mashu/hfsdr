//! Modern dark theme for the waterfall UI.

use std::fmt::Display;

use eframe::egui::{
    Align, Color32, CornerRadius, FontFamily, FontId, Layout, RichText, Stroke, TextStyle, Ui,
    Visuals,
};

pub const ACCENT: Color32 = Color32::from_rgb(56, 189, 248);
pub const ACCENT_DIM: Color32 = Color32::from_rgb(30, 100, 140);
pub const PANEL: Color32 = Color32::from_rgb(22, 27, 38);
pub const SURFACE: Color32 = Color32::from_rgb(14, 17, 24);
pub const GRID: Color32 = Color32::from_rgb(50, 60, 78);
pub const TRACE: Color32 = Color32::from_rgb(110, 231, 183);
pub const TRACE_GLOW: Color32 = Color32::from_rgb(40, 120, 160);
pub const FILTER_EDGE: Color32 = Color32::from_rgb(125, 211, 252);
pub const CENTER_LINE: Color32 = Color32::from_rgb(248, 113, 113);
pub const NOTCH_LINE: Color32 = Color32::from_rgb(192, 132, 252);
pub const MUTED: Color32 = Color32::from_rgb(140, 150, 170);
pub const OK: Color32 = Color32::from_rgb(110, 231, 183);
pub const WARN: Color32 = Color32::from_rgb(251, 191, 36);

pub fn apply(ctx: &eframe::egui::Context) {
    let mut style = (*ctx.global_style()).clone();
    let mut visuals = Visuals::dark();
    visuals.window_fill = SURFACE;
    visuals.panel_fill = PANEL;
    visuals.extreme_bg_color = SURFACE;
    visuals.faint_bg_color = Color32::from_rgb(30, 36, 48);
    visuals.widgets.noninteractive.bg_fill = Color32::from_rgb(28, 33, 44);
    visuals.widgets.inactive.bg_fill = Color32::from_rgb(38, 44, 58);
    visuals.widgets.hovered.bg_fill = Color32::from_rgb(48, 56, 72);
    visuals.widgets.active.bg_fill = Color32::from_rgb(52, 72, 96);
    visuals.widgets.inactive.weak_bg_fill = Color32::from_rgb(32, 38, 50);
    visuals.selection.bg_fill = ACCENT_DIM;
    style.visuals = visuals;
    style.spacing.item_spacing = eframe::egui::vec2(8.0, 6.0);
    style.spacing.button_padding = eframe::egui::vec2(10.0, 5.0);
    style.text_styles.insert(TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
    ctx.set_global_style(style);
}

pub fn section_frame() -> eframe::egui::Frame {
    eframe::egui::Frame::new()
        .fill(Color32::from_rgb(28, 33, 44))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(12.0)
        .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 68)))
}

/// Full-width section card within the current panel.
pub fn section_card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    section_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        add_contents(ui);
    });
}

pub fn section_heading(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).strong().color(ACCENT));
}

pub fn section_hint(ui: &mut Ui, text: &str) {
    ui.label(RichText::new(text).small().color(MUTED));
}

pub fn stat_row(ui: &mut Ui, label: &str, value: impl Display) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(label).small().color(MUTED));
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(value.to_string()).strong());
        });
    });
}

pub fn badge(ui: &mut Ui, text: &str, color: Color32) {
    let frame = eframe::egui::Frame::new()
        .fill(Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40))
        .corner_radius(CornerRadius::same(4))
        .inner_margin(eframe::egui::vec2(6.0, 2.0))
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 120),
        ));
    frame.show(ui, |ui| {
        ui.label(RichText::new(text).small().color(color));
    });
}

pub fn collapsible_section(
    ui: &mut Ui,
    id: &str,
    title: &str,
    default_open: bool,
    add_contents: impl FnOnce(&mut Ui),
) {
    section_frame().show(ui, |ui| {
        ui.set_min_width(ui.available_width());
        eframe::egui::CollapsingHeader::new(RichText::new(title).strong().color(ACCENT))
            .id_salt(id)
            .default_open(default_open)
            .show(ui, add_contents);
    });
}
