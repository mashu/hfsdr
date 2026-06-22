//! Modern dark theme for the waterfall UI.

use std::fmt::Display;

use eframe::egui::{
    Align, Align2, Color32, CornerRadius, FontFamily, FontId, Layout, Pos2, Rect, RichText, Sense,
    Stroke, TextStyle, Ui, Vec2, Visuals,
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

pub fn clickable_badge(ui: &mut Ui, text: &str, color: Color32) -> eframe::egui::Response {
    ui.add(
        eframe::egui::Button::new(RichText::new(text).small().color(color))
            .fill(Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 40))
            .stroke(Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 120),
            )),
    )
}

/// Compact padlock toggle for amateur band lock (icon-only).
pub fn band_lock_toggle(ui: &mut Ui, on: &mut bool) -> bool {
    let size = Vec2::splat(22.0);
    let (rect, resp) = ui.allocate_exact_size(size, Sense::click());
    let mut changed = false;
    if resp.clicked() {
        *on = !*on;
        changed = true;
    }

    let color = if *on { ACCENT } else { MUTED };
    let fill = if *on {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 36)
    } else {
        Color32::from_rgb(32, 38, 50)
    };
    let stroke = Stroke::new(1.5, color);
    let painter = ui.painter();
    painter.rect_filled(rect, 4.0, fill);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 90)),
        eframe::egui::StrokeKind::Inside,
    );

    let cx = rect.center().x;
    let cy = rect.center().y;
    let body = Rect::from_center_size(Pos2::new(cx, cy + 2.5), Vec2::new(8.0, 6.0));
    painter.rect_stroke(body, 1.0, stroke, eframe::egui::StrokeKind::Inside);

    let shackle_left = cx - 3.0;
    let shackle_right = cx + 3.0;
    let shackle_top = cy - 4.5;
    let shackle_bottom = body.top() + 1.0;
    if *on {
        painter.line_segment(
            [Pos2::new(shackle_left, shackle_bottom), Pos2::new(shackle_left, shackle_top)],
            stroke,
        );
        painter.line_segment(
            [Pos2::new(shackle_left, shackle_top), Pos2::new(shackle_right, shackle_top)],
            stroke,
        );
        painter.line_segment(
            [Pos2::new(shackle_right, shackle_top), Pos2::new(shackle_right, shackle_bottom)],
            stroke,
        );
    } else {
        painter.line_segment(
            [Pos2::new(shackle_left, shackle_bottom), Pos2::new(shackle_left, shackle_top)],
            stroke,
        );
        painter.line_segment(
            [Pos2::new(shackle_left, shackle_top), Pos2::new(shackle_right, shackle_top)],
            stroke,
        );
        painter.line_segment(
            [Pos2::new(shackle_right, shackle_top), Pos2::new(shackle_right, shackle_bottom - 2.0)],
            stroke,
        );
    }

    resp.on_hover_text("Lock RX to amateur bands (160m–10m, 6m)");
    changed
}

/// Full-width DSP stage row: label, optional shortcut chip, animated pill switch.
pub fn stage_toggle(
    ui: &mut Ui,
    on: &mut bool,
    title: &str,
    subtitle: Option<&str>,
    shortcut: Option<&str>,
) -> bool {
    let id = ui.id().with(title);
    let row_h = if subtitle.is_some() { 46.0 } else { 38.0 };
    let width = ui.available_width();
    let (rect, resp) =
        ui.allocate_exact_size(eframe::egui::vec2(width, row_h), eframe::egui::Sense::click());
    let mut changed = false;
    if resp.clicked() {
        *on = !*on;
        changed = true;
    }

    let anim = ui.ctx().animate_bool(id, *on);
    let bg = Color32::from_rgba_unmultiplied(
        ACCENT.r(),
        ACCENT.g(),
        ACCENT.b(),
        (eframe::egui::lerp(8.0..=36.0, anim)) as u8,
    );
    let border = Color32::from_rgba_unmultiplied(
        ACCENT.r(),
        ACCENT.g(),
        ACCENT.b(),
        (eframe::egui::lerp(40.0..=140.0, anim)) as u8,
    );
    ui.painter().rect(
        rect,
        CornerRadius::same(8),
        bg,
        Stroke::new(1.0, border),
        eframe::egui::StrokeKind::Inside,
    );

    let pill_w = 38.0;
    let pill_h = 20.0;
    let pill_rect = eframe::egui::Rect::from_min_size(
        eframe::egui::pos2(rect.right() - pill_w - 10.0, rect.center().y - pill_h / 2.0),
        eframe::egui::vec2(pill_w, pill_h),
    );
    let how_on = ui.ctx().animate_bool(id.with("pill"), *on);
    let track = if *on {
        ACCENT_DIM
    } else {
        Color32::from_rgb(48, 54, 70)
    };
    ui.painter().rect(
        pill_rect,
        CornerRadius::same(10),
        track,
        Stroke::NONE,
        eframe::egui::StrokeKind::Inside,
    );
    let cx = eframe::egui::lerp((pill_rect.left() + 10.0)..=(pill_rect.right() - 10.0), how_on);
    let knob = if *on { ACCENT } else { MUTED };
    ui.painter()
        .circle(eframe::egui::pos2(cx, pill_rect.center().y), 8.0, knob, Stroke::NONE);

    let text_left = rect.left() + 12.0;
    let title_y = if subtitle.is_some() {
        rect.top() + 8.0
    } else {
        rect.center().y
    };
    ui.painter().text(
        eframe::egui::pos2(text_left, title_y),
        if subtitle.is_some() {
            Align2::LEFT_TOP
        } else {
            Align2::LEFT_CENTER
        },
        title,
        FontId::proportional(13.0),
        if *on {
            ACCENT
        } else {
            Color32::from_rgb(220, 228, 240)
        },
    );
    if let Some(sub) = subtitle {
        ui.painter().text(
            eframe::egui::pos2(text_left, rect.top() + 26.0),
            Align2::LEFT_TOP,
            sub,
            FontId::proportional(11.0),
            MUTED,
        );
    }
    if let Some(key) = shortcut {
        let chip = format!("[{key}]");
        let chip_rect = eframe::egui::Rect::from_min_size(
            eframe::egui::pos2(pill_rect.left() - 36.0, rect.center().y - 9.0),
            eframe::egui::vec2(28.0, 18.0),
        );
        ui.painter().rect(
            chip_rect,
            CornerRadius::same(4),
            Color32::from_rgb(38, 44, 58),
            Stroke::new(1.0, Color32::from_rgb(70, 80, 100)),
            eframe::egui::StrokeKind::Inside,
        );
        ui.painter().text(
            chip_rect.center(),
            Align2::CENTER_CENTER,
            chip,
            FontId::monospace(10.0),
            MUTED,
        );
    }

    changed
}

/// A compact on/off pill toggle (a nicer checkbox). Returns true if toggled.
pub fn toggle(ui: &mut Ui, on: &mut bool, label: &str) -> bool {
    let desired = eframe::egui::vec2(34.0, 18.0);
    let mut changed = false;
    ui.horizontal(|ui| {
        let (rect, resp) = ui.allocate_exact_size(desired, eframe::egui::Sense::click());
        if resp.clicked() {
            *on = !*on;
            changed = true;
        }
        let how_on = ui.ctx().animate_bool(resp.id, *on);
        let track = if *on { ACCENT_DIM } else { Color32::from_rgb(48, 54, 70) };
        ui.painter().rect(
            rect,
            CornerRadius::same(9),
            track,
            Stroke::NONE,
            eframe::egui::StrokeKind::Inside,
        );
        let cx = eframe::egui::lerp((rect.left() + 9.0)..=(rect.right() - 9.0), how_on);
        let knob = if *on { ACCENT } else { MUTED };
        ui.painter()
            .circle(eframe::egui::pos2(cx, rect.center().y), 7.0, knob, Stroke::NONE);
        ui.label(label);
    });
    changed
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
