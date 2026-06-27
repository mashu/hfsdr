//! Modern floating panel chrome (connection, IQ I/O).

use std::fmt::Display;

use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, FontId, Frame, Layout, Margin, RichText, Stroke,
    StrokeKind, Ui, Vec2,
};

use crate::theme::{chip_hovered, ACCENT, MUTED, PANEL, SURFACE, WARN};

pub fn popup_window_frame() -> Frame {
    Frame::new()
        .fill(SURFACE)
        .corner_radius(CornerRadius::same(12))
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 55),
        ))
        .inner_margin(0.0)
        .shadow(egui::epaint::Shadow {
            offset: [0, 8],
            blur: 20,
            spread: 0,
            color: Color32::from_black_alpha(100),
        })
}

pub struct PopupHeader<'a> {
    pub title: &'a str,
    pub subtitle: Option<&'a str>,
    pub status: Option<(String, Color32)>,
}

pub fn popup_header(ui: &mut Ui, header: PopupHeader<'_>, open: &mut bool) {
    let width = ui.available_width();
    ui.allocate_ui(Vec2::new(width, 2.0), |ui| {
        ui.painter().rect_filled(ui.max_rect(), 0.0, ACCENT);
    });

    Frame::new()
        .fill(PANEL)
        .inner_margin(Margin::symmetric(12, 8))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new(header.title)
                        .size(15.0)
                        .strong()
                        .color(Color32::from_rgb(232, 238, 248)),
                );
                if let Some(sub) = header.subtitle {
                    ui.label(RichText::new(format!("· {sub}")).small().color(MUTED));
                }
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if icon_button(ui, "✕", "Close").clicked() {
                        *open = false;
                    }
                    if let Some((label, color)) = &header.status {
                        status_pill(ui, label, *color);
                        ui.add_space(6.0);
                    }
                });
            });
        });

    let sep_y = ui.cursor().top();
    let sep_rect = ui.available_rect_before_wrap();
    ui.painter().hline(
        sep_rect.left()..=sep_rect.right(),
        sep_y,
        Stroke::new(1.0, Color32::from_rgb(38, 46, 60)),
    );
}

/// Max scrollable body height inside a popup (header chrome is ~48px).
pub fn popup_body_max_height(window_max_h: f32) -> f32 {
    (window_max_h - 48.0).max(120.0)
}

pub fn popup_scroll_body<R>(ui: &mut Ui, max_body_height: f32, add_contents: impl FnOnce(&mut Ui) -> R) -> R {
    Frame::new()
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            ui.spacing_mut().item_spacing = Vec2::new(6.0, 4.0);
            egui::ScrollArea::vertical()
                .auto_shrink([true, true])
                .max_height(max_body_height)
                .show(ui, add_contents)
                .inner
        })
        .inner
}

pub fn popup_section(ui: &mut Ui, title: &str, hint: Option<&str>, add_contents: impl FnOnce(&mut Ui)) {
    Frame::new()
        .fill(Color32::from_rgb(24, 29, 40))
        .corner_radius(CornerRadius::same(8))
        .stroke(Stroke::new(1.0, Color32::from_rgb(42, 50, 66)))
        .inner_margin(Margin::symmetric(10, 8))
        .show(ui, |ui| {
            let w = ui.available_width();
            ui.set_min_width(w);
            ui.set_max_width(w);
            ui.horizontal(|ui| {
                let bar = egui::Rect::from_min_size(ui.cursor().min, Vec2::new(2.0, 12.0));
                ui.painter().rect_filled(bar, CornerRadius::same(1), ACCENT);
                ui.add_space(6.0);
                ui.label(RichText::new(title).small().strong().color(ACCENT));
            });
            if let Some(text) = hint {
                ui.label(RichText::new(text).small().color(MUTED));
            }
            ui.add_space(4.0);
            add_contents(ui);
        });
    ui.add_space(6.0);
}

pub fn alert_banner(ui: &mut Ui, text: &str, detail: Option<&str>) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), 22))
        .corner_radius(CornerRadius::same(6))
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), 90),
        ))
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.label(RichText::new(text).small().color(WARN));
            if let Some(d) = detail {
                ui.label(RichText::new(d).small().color(MUTED));
            }
        });
    ui.add_space(6.0);
}

/// Label, truncated path, and a clear browse action — stacked for readability.
pub fn path_row(ui: &mut Ui, label: &str, path: &str, browse_label: &str) -> bool {
    let mut picked = false;
    ui.vertical(|ui| {
        ui.label(RichText::new(label).small().color(MUTED));
        ui.horizontal(|ui| {
            ui.label(
                RichText::new(truncate_middle(path, 42))
                    .small()
                    .monospace()
                    .color(Color32::from_rgb(170, 180, 198)),
            );
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if secondary_button(ui, browse_label).clicked() {
                    picked = true;
                }
            });
        });
    });
    picked
}

pub fn truncate_middle(s: &str, max_chars: usize) -> String {
    if s.chars().count() <= max_chars {
        return s.to_string();
    }
    let keep = max_chars.saturating_sub(1) / 2;
    let head: String = s.chars().take(keep).collect();
    let tail: String = s.chars().rev().take(keep).collect::<Vec<_>>().into_iter().rev().collect();
    format!("{head}…{tail}")
}

pub fn inline_stats(ui: &mut Ui, parts: &[(&str, String)]) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 10.0;
        for (i, (label, value)) in parts.iter().enumerate() {
            if i > 0 {
                ui.label(RichText::new("·").small().color(MUTED));
            }
            ui.label(
                RichText::new(format!("{label} {value}"))
                    .small()
                    .color(MUTED),
            );
        }
    });
}

pub fn primary_button(ui: &mut Ui, label: &str, enabled: bool) -> egui::Response {
    ui.add_enabled(
        enabled,
        Button::new(RichText::new(label).small().strong().color(SURFACE))
            .fill(ACCENT)
            .stroke(Stroke::NONE)
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::new(76.0, 26.0)),
    )
}

pub fn secondary_button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        Button::new(RichText::new(label).small().color(Color32::from_rgb(210, 218, 232)))
            .fill(Color32::from_rgb(36, 42, 56))
            .stroke(Stroke::new(1.0, Color32::from_rgb(58, 68, 88)))
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::new(68.0, 26.0)),
    )
}

pub fn ghost_button(ui: &mut Ui, label: &str) -> egui::Response {
    ui.add(
        Button::new(RichText::new(label).small().color(MUTED))
            .fill(Color32::TRANSPARENT)
            .stroke(Stroke::NONE),
    )
}

pub fn icon_button(ui: &mut Ui, icon: &str, tooltip: &str) -> egui::Response {
    ui.add(
        Button::new(RichText::new(icon).size(12.0).color(MUTED))
            .fill(Color32::from_rgb(32, 38, 50))
            .stroke(Stroke::new(
                1.0,
                Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 50),
            ))
            .corner_radius(CornerRadius::same(6))
            .min_size(Vec2::splat(24.0)),
    )
    .on_hover_text(tooltip)
}

pub fn segment_choice(ui: &mut Ui, id: &str, selected: usize, labels: &[&str]) -> Option<usize> {
    segment_choice_sized(ui, id, selected, labels, 64.0)
}

/// Label on its own row; segmented control below (fits narrow side panels).
pub fn labeled_segment_choice(
    ui: &mut Ui,
    id: &str,
    label: &str,
    selected: usize,
    options: &[&str],
    min_button_width: f32,
) -> Option<usize> {
    let picked = ui.vertical(|ui| {
        ui.label(RichText::new(label).small().color(MUTED));
        segment_choice_sized(ui, id, selected, options, min_button_width)
    });
    picked.inner
}

pub fn segment_choice_sized(
    ui: &mut Ui,
    id: &str,
    selected: usize,
    labels: &[&str],
    min_button_width: f32,
) -> Option<usize> {
    let mut picked = None;
    let compact = min_button_width < 48.0;
    Frame::new()
        .fill(Color32::from_rgb(18, 22, 30))
        .corner_radius(CornerRadius::same(6))
        .stroke(Stroke::new(1.0, Color32::from_rgb(42, 50, 66)))
        .inner_margin(2.0)
        .show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            let add_buttons = |ui: &mut Ui| {
                ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                for (i, label) in labels.iter().enumerate() {
                    let on = i == selected;
                    let resp = ui.add(
                        Button::new(
                            RichText::new(*label)
                                .small()
                                .color(if on { ACCENT } else { MUTED }),
                        )
                        .fill(if on {
                            Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 40)
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if on {
                            Stroke::new(
                                1.0,
                                Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 120),
                            )
                        } else {
                            Stroke::NONE
                        })
                        .corner_radius(CornerRadius::same(5))
                        .min_size(Vec2::new(min_button_width, 24.0)),
                    );
                    if resp.clicked() {
                        picked = Some(i);
                    }
                }
            };
            if compact {
                ui.horizontal_wrapped(add_buttons);
            } else {
                ui.horizontal(add_buttons);
            }
        });
    let _ = id;
    picked
}

/// Compact wrapped band chips (segment-control style). Returns clicked preset index.
pub fn band_preset_grid(
    ui: &mut Ui,
    id: &str,
    rx_center_hz: f64,
    bands: &[(&str, f64)],
) -> Option<usize> {
    let mut picked = None;
    ui.push_id(id, |ui| {
        Frame::new()
            .fill(Color32::from_rgb(18, 22, 30))
            .corner_radius(CornerRadius::same(6))
            .stroke(Stroke::new(1.0, Color32::from_rgb(42, 50, 66)))
            .inner_margin(2.0)
            .show(ui, |ui| {
                ui.set_max_width(ui.available_width());
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = Vec2::new(2.0, 2.0);
                    for (i, (label, center_hz)) in bands.iter().enumerate() {
                        let selected = (rx_center_hz - center_hz).abs() < 0.5;
                        let resp = ui.add(
                            Button::new(
                                RichText::new(*label)
                                    .small()
                                    .color(if selected { ACCENT } else { MUTED }),
                            )
                            .fill(if selected {
                                Color32::from_rgba_unmultiplied(
                                    ACCENT.r(),
                                    ACCENT.g(),
                                    ACCENT.b(),
                                    40,
                                )
                            } else {
                                Color32::TRANSPARENT
                            })
                            // Transparent 1px stroke keeps cell size stable when selection changes.
                            .stroke(if selected {
                                Stroke::new(
                                    1.0,
                                    Color32::from_rgba_unmultiplied(
                                        ACCENT.r(),
                                        ACCENT.g(),
                                        ACCENT.b(),
                                        120,
                                    ),
                                )
                            } else {
                                Stroke::new(1.0, Color32::TRANSPARENT)
                            })
                            .corner_radius(CornerRadius::same(5))
                            .min_size(Vec2::new(0.0, 24.0)),
                        );
                        let mhz = center_hz / 1_000_000.0;
                        let clicked = resp.clicked();
                        resp.on_hover_text(format!("{mhz:.3} MHz · CW segment"));
                        if clicked {
                            picked = Some(i);
                        }
                    }
                });
            });
    });
    picked
}

/// Segmented preset row for a numeric value; highlights the nearest preset within `match_eps`.
pub fn preset_segment_f32(
    ui: &mut Ui,
    id: &str,
    value: &mut f32,
    presets: &[(&str, f32)],
    match_eps: f32,
) -> bool {
    let selected = presets
        .iter()
        .position(|(_, hz)| (*value - hz).abs() <= match_eps)
        .unwrap_or(presets.len());
    let labels: Vec<&str> = presets.iter().map(|(label, _)| *label).collect();
    if let Some(i) = segment_choice_sized(ui, id, selected, &labels, 36.0) {
        if i < presets.len() {
            *value = presets[i].1;
            return true;
        }
    }
    false
}

pub fn list_row(ui: &mut Ui, text: &str, enabled: bool) -> egui::Response {
    let width = ui.available_width();
    let height = 26.0;
    let (rect, response) = ui.allocate_exact_size(Vec2::new(width, height), egui::Sense::click());
    let hovered = chip_hovered(ui, rect, &response) && enabled;
    let painter = ui.painter_at(rect);
    let bg = if !enabled {
        Color32::from_rgb(22, 26, 34)
    } else if hovered {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 24)
    } else {
        Color32::from_rgb(18, 22, 30)
    };
    let border = if hovered && enabled {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 120)
    } else {
        Color32::from_rgb(40, 48, 62)
    };
    painter.rect(
        rect,
        CornerRadius::same(6),
        bg,
        Stroke::new(1.0, border),
        StrokeKind::Inside,
    );
    painter.text(
        rect.left_center() + Vec2::new(8.0, 0.0),
        egui::Align2::LEFT_CENTER,
        text,
        FontId::proportional(11.0),
        if enabled {
            Color32::from_rgb(220, 228, 240)
        } else {
            MUTED
        },
    );
    response
}

pub fn chip_row(ui: &mut Ui, labels: &[String]) -> Option<usize> {
    let mut picked = None;
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing = Vec2::new(4.0, 4.0);
        for (i, label) in labels.iter().enumerate() {
            let resp = ui.add(
                Button::new(RichText::new(label).small())
                    .fill(Color32::from_rgb(32, 38, 50))
                    .stroke(Stroke::new(1.0, Color32::from_rgb(58, 68, 88)))
                    .corner_radius(CornerRadius::same(12)),
            );
            if resp.clicked() {
                picked = Some(i);
            }
        }
    });
    picked
}

pub fn status_pill(ui: &mut Ui, label: impl Display, color: Color32) {
    Frame::new()
        .fill(Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 36))
        .corner_radius(CornerRadius::same(10))
        .stroke(Stroke::new(
            1.0,
            Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 120),
        ))
        .inner_margin(Margin::symmetric(8, 2))
        .show(ui, |ui| {
            ui.label(RichText::new(label.to_string()).small().strong().color(color));
        });
}

pub fn configure_popup_window(
    id: &str,
    default_pos: [f32; 2],
    width: f32,
    max_height: f32,
) -> egui::Window<'static> {
    egui::Window::new(egui::RichText::new("").size(0.0))
        .id(egui::Id::new(id))
        .title_bar(false)
        .frame(popup_window_frame())
        .collapsible(false)
        .resizable(false)
        .auto_sized()
        .min_width(width)
        .max_width(width)
        .max_height(max_height)
        .default_pos(default_pos)
}

