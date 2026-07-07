//! Modern dark theme for the waterfall UI.

use std::fmt::Display;

use eframe::egui::{
    Align2, Color32, CornerRadius, FontFamily, FontId, Frame, Label, Pos2, Rect,
    Response, RichText, Sense, Stroke, TextStyle, Ui, Vec2, Visuals,
};

/// Pointer is over the painted `rect` and nothing opaque covers this widget.
pub fn chip_hovered(ui: &Ui, rect: Rect, response: &Response) -> bool {
    ui.rect_contains_pointer(rect) && response.contains_pointer()
}

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
    visuals.hyperlink_color = ACCENT;
    visuals.warn_fg_color = WARN;
    visuals.error_fg_color = Color32::from_rgb(248, 113, 113);
    visuals.override_text_color = Some(Color32::from_rgb(226, 232, 244));
    style.visuals = visuals;
    // Avoid tooltip-under-cursor hover flicker on compact status chips.
    style.interaction.tooltip_delay = 0.55;
    style.spacing.item_spacing = eframe::egui::vec2(8.0, 6.0);
    style.spacing.button_padding = eframe::egui::vec2(10.0, 5.0);
    style.spacing.indent = 16.0;
    style.spacing.scroll = eframe::egui::style::ScrollStyle {
        bar_width: 8.0,
        ..Default::default()
    };
    style.text_styles.insert(TextStyle::Heading, FontId::new(18.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Body, FontId::new(13.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Small, FontId::new(11.0, FontFamily::Proportional));
    style.text_styles.insert(TextStyle::Monospace, FontId::new(12.0, FontFamily::Monospace));
    style.visuals.widgets.noninteractive.bg_fill = tooltip_fill();
    style.visuals.window_stroke = Stroke::new(
        1.0,
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 120),
    );
    ctx.set_global_style(style);
}

/// Side panel chrome — fills the full panel width while resizing.
pub fn side_panel_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .inner_margin(eframe::egui::Margin::symmetric(8, 6))
        .stroke(Stroke::new(1.0, Color32::from_rgb(38, 46, 62)))
}

/// Bottom panel chrome (log, spots history) — fills height while dragging the resize handle.
pub fn bottom_panel_frame() -> Frame {
    side_panel_frame()
}

/// Top status bar chrome.
pub fn status_panel_frame() -> Frame {
    Frame::new()
        .fill(PANEL)
        .inner_margin(eframe::egui::Margin::symmetric(8, 4))
        .stroke(Stroke::new(1.0, Color32::from_rgb(38, 46, 62)))
        .corner_radius(CornerRadius {
            nw: 0,
            ne: 0,
            sw: 8,
            se: 8,
        })
}

pub fn section_frame() -> eframe::egui::Frame {
    eframe::egui::Frame::new()
        .fill(Color32::from_rgb(28, 33, 44))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(12.0)
        .stroke(Stroke::new(1.0, Color32::from_rgb(45, 52, 68)))
        .shadow(eframe::egui::epaint::Shadow {
            offset: [0, 2],
            blur: 6,
            spread: 0,
            color: Color32::from_black_alpha(40),
        })
}

/// Full-width section card within the current panel.
pub fn section_card(ui: &mut Ui, add_contents: impl FnOnce(&mut Ui)) {
    section_frame().show(ui, |ui| {
        let w = ui.available_width();
        ui.set_min_width(w);
        ui.set_max_width(w);
        add_contents(ui);
    });
}

pub fn section_heading(ui: &mut Ui, title: &str) {
    ui.label(RichText::new(title).strong().color(ACCENT));
}

pub fn section_hint(ui: &mut Ui, text: &str) {
    ui.add(Label::new(RichText::new(text).small().color(MUTED)).wrap());
}

fn tooltip_fill() -> Color32 {
    Color32::from_rgba_unmultiplied(22, 27, 38, 242)
}

pub fn rich_tooltip_frame() -> Frame {
    Frame::new()
        .fill(tooltip_fill())
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 140),
        ))
        .corner_radius(CornerRadius::same(8))
        .inner_margin(eframe::egui::Margin::symmetric(10, 8))
        .shadow(eframe::egui::epaint::Shadow {
            offset: [0, 4],
            blur: 14,
            spread: 0,
            color: Color32::from_black_alpha(70),
        })
}

pub fn rich_tooltip_body(ui: &mut Ui, title: Option<&str>, lines: &[(&str, Color32)]) {
    rich_tooltip_frame().show(ui, |ui| {
        ui.set_max_width(300.0);
        if let Some(t) = title {
            ui.label(RichText::new(t).strong().color(ACCENT));
            ui.add_space(3.0);
        }
        for (text, color) in lines {
            ui.label(RichText::new(*text).small().color(*color));
            ui.add_space(1.0);
        }
    });
}

pub fn attach_rich_tooltip(resp: &Response, title: Option<&str>, lines: &[(&str, Color32)]) {
    resp.clone().on_hover_ui(|ui| rich_tooltip_body(ui, title, lines));
}

pub fn section_heading_with_tip(ui: &mut Ui, title: &str, tip: &[(&str, Color32)]) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let title_resp = ui.label(RichText::new(title).strong().color(ACCENT));
        let hint_resp = ui.label(RichText::new("(?)").small().color(MUTED));
        attach_rich_tooltip(&title_resp, Some(title), tip);
        attach_rich_tooltip(&hint_resp, Some(title), tip);
    });
}

pub fn stat_row(ui: &mut Ui, label: &str, value: impl Display) {
    ui.vertical(|ui| {
        let w = ui.available_width();
        ui.set_max_width(w);
        ui.label(RichText::new(label).small().color(MUTED));
        ui.add(
            eframe::egui::Label::new(RichText::new(value.to_string()).strong())
                .wrap_mode(eframe::egui::TextWrapMode::Wrap),
        );
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

/// Compact chain-link toggle for amateur band lock (linked = constrained to ham bands).
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
    let painter = ui.painter_at(rect);
    painter.rect_filled(rect, 4.0, fill);
    painter.rect_stroke(
        rect,
        4.0,
        Stroke::new(1.0, Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 90)),
        eframe::egui::StrokeKind::Inside,
    );

    let cx = rect.center().x;
    let cy = rect.center().y;
    draw_chain_links(&painter, cx, cy, *on, stroke);

    let tip = if *on {
        "Band lock: linked — RX stays inside amateur allocations (160m–6m)"
    } else {
        "Band lock: open — tune anywhere on the dial"
    };
    resp.on_hover_text(tip);
    changed
}

fn draw_chain_links(painter: &eframe::egui::Painter, cx: f32, cy: f32, linked: bool, stroke: Stroke) {
    let r = 4.2;
    let gap = if linked { 2.8 } else { 5.6 };
    let left = Pos2::new(cx - gap / 2.0 - r, cy);
    let right = Pos2::new(cx + gap / 2.0 + r, cy);

    painter.circle_stroke(left, r, stroke);
    painter.circle_stroke(right, r, stroke);

    if linked {
        painter.line_segment(
            [Pos2::new(left.x + r * 0.35, cy - r * 0.55), Pos2::new(right.x - r * 0.35, cy - r * 0.55)],
            stroke,
        );
        painter.line_segment(
            [Pos2::new(left.x + r * 0.35, cy + r * 0.55), Pos2::new(right.x - r * 0.35, cy + r * 0.55)],
            stroke,
        );
    } else {
        let break_stroke = Stroke::new(stroke.width + 0.5, Color32::from_rgb(248, 113, 113));
        let mid = Pos2::new(cx, cy);
        painter.line_segment(
            [Pos2::new(mid.x - 2.5, cy - 3.0), Pos2::new(mid.x + 1.0, cy)],
            break_stroke,
        );
        painter.line_segment(
            [Pos2::new(mid.x + 1.0, cy), Pos2::new(mid.x - 2.5, cy + 3.0)],
            break_stroke,
        );
    }
}

/// Full-width DSP stage row: label, optional shortcut chip, animated pill switch.
pub fn stage_toggle(
    ui: &mut Ui,
    on: &mut bool,
    title: &str,
    subtitle: Option<&str>,
    shortcut: Option<&str>,
    tip: Option<&[(&str, Color32)]>,
) -> bool {
    let id = ui.id().with(title);
    let row_h = if subtitle.is_some() { 46.0 } else { 38.0 };
    let width = ui.available_width();
    let show_shortcut = shortcut.is_some() && width >= 280.0;
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
    let mut border = Color32::from_rgba_unmultiplied(
        ACCENT.r(),
        ACCENT.g(),
        ACCENT.b(),
        (eframe::egui::lerp(40.0..=140.0, anim)) as u8,
    );
    let hovered = chip_hovered(ui, rect, &resp);
    if hovered && !*on {
        border = Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 90);
    }
    ui.painter_at(rect).rect(
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
    let painter = ui.painter_at(rect);
    painter.rect(
        pill_rect,
        CornerRadius::same(10),
        track,
        Stroke::NONE,
        eframe::egui::StrokeKind::Inside,
    );
    let cx = eframe::egui::lerp((pill_rect.left() + 10.0)..=(pill_rect.right() - 10.0), how_on);
    let knob = if *on { ACCENT } else { MUTED };
    painter.circle(eframe::egui::pos2(cx, pill_rect.center().y), 8.0, knob, Stroke::NONE);

    let text_left = rect.left() + 12.0;
    let title_y = if subtitle.is_some() {
        rect.top() + 8.0
    } else {
        rect.center().y
    };
    painter.text(
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
        painter.text(
            eframe::egui::pos2(text_left, rect.top() + 26.0),
            Align2::LEFT_TOP,
            sub,
            FontId::proportional(11.0),
            MUTED,
        );
    }
    if let Some(key) = shortcut {
        if show_shortcut {
            let chip = format!("[{key}]");
            let chip_rect = eframe::egui::Rect::from_min_size(
                eframe::egui::pos2(pill_rect.left() - 36.0, rect.center().y - 9.0),
                eframe::egui::vec2(28.0, 18.0),
            );
            painter.rect(
                chip_rect,
                CornerRadius::same(4),
                Color32::from_rgb(38, 44, 58),
                Stroke::new(1.0, Color32::from_rgb(70, 80, 100)),
                eframe::egui::StrokeKind::Inside,
            );
            painter.text(
                chip_rect.center(),
                Align2::CENTER_CENTER,
                chip,
                FontId::monospace(10.0),
                MUTED,
            );
        } else {
            let tip_key = format!("Shortcut: {key}");
            resp.clone().on_hover_text(tip_key);
        }
    }

    if let Some(lines) = tip {
        attach_rich_tooltip(&resp, Some(title), lines);
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
        ui.painter_at(rect).rect(
            rect,
            CornerRadius::same(9),
            track,
            Stroke::NONE,
            eframe::egui::StrokeKind::Inside,
        );
        let cx = eframe::egui::lerp((rect.left() + 9.0)..=(rect.right() - 9.0), how_on);
        let knob = if *on { ACCENT } else { MUTED };
        ui.painter_at(rect)
            .circle(eframe::egui::pos2(cx, rect.center().y), 7.0, knob, Stroke::NONE);
        ui.label(label);
    });
    changed
}

pub fn collapsible_section(
    ui: &mut Ui,
    id: &str,
    title: &str,
    tip: Option<&[(&str, Color32)]>,
    default_open: bool,
    add_contents: impl FnOnce(&mut Ui),
) {
    section_frame().show(ui, |ui| {
        let w = ui.available_width();
        ui.set_min_width(w);
        ui.set_max_width(w);
        let cr = eframe::egui::CollapsingHeader::new(RichText::new(title).strong().color(ACCENT))
            .id_salt(id)
            .default_open(default_open)
            .show(ui, add_contents);
        if let Some(lines) = tip {
            attach_rich_tooltip(&cr.header_response, Some(title), lines);
        }
    });
}
