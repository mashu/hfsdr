#!/usr/bin/env python3
"""Split waterfall app.rs into app/ module tree."""
from __future__ import annotations

import re
from pathlib import Path

ROOT = Path(__file__).resolve().parents[1]
SRC = ROOT / "src/bin/waterfall/app.rs"
OUT = ROOT / "src/bin/waterfall/app"
UTF8 = "utf-8"


def read_text(path: Path) -> str:
    return path.read_text(encoding=UTF8)


def write_text(path: Path, text: str) -> None:
    path.write_text(text, encoding=UTF8, newline="\n")

METHOD_GROUPS: dict[str, list[str]] = {
    "engine": [
        "start_kiwi_directory_fetch",
        "poll_kiwi_directory",
        "connection_unstable",
        "is_wideband_device",
        "is_wideband",
        "skimmer_spectrum_ok",
        "skimmer_runtime_enabled",
        "effective_target_fps",
        "effective_skimmer",
        "pump_engine",
    ],
    "settings": [
        "apply_settings",
        "current_settings",
        "autosave",
        "invalidate_waterfall_history",
    ],
    "plot/actions": [
        "apply_plot_actions",
        "iq_passband_hz",
        "plot_full_span_hz",
        "plot_max_zoom_out",
        "spectrum_view",
        "waterfall_storage_view",
        "storage_row_width",
        "update_plot_hover",
    ],
    "plot/waterfall": [
        "waterfall_source_row",
        "write_row_pixels",
        "waterfall_row_db_for_storage",
        "waterfall_row_db_for_viewport",
        "upload_waterfall_viewport",
        "sync_waterfall_storage",
        "sync_waterfall_viewport",
    ],
    "plot/central": ["central_panel", "refresh_plot_composites"],
    "tuning": [
        "clamp_center_to_ham_bands",
        "clear_rit",
        "zero_beat",
        "apply_pitch_lock",
        "listen_offset_hz",
        "center_hz",
        "cw_band_for_center",
        "band_preset_buttons",
        "band_overview_span_hz",
        "default_cw_segment_hz",
        "apply_default_view_zoom",
        "select_cw_band",
        "tune_to_hz",
    ],
    "display/levels": [
        "lock_display_levels_for_rf_tuning",
        "update_display_levels",
        "estimate_display_levels",
        "passband_max_hz",
    ],
    "ui/display": ["display_section"],
    "radio/sync": [
        "apply_audio_device",
        "kiwi_rf_live",
        "sync_kiwi_rf_now",
        "rf_meter_dbm",
        "apply_radio_settings",
        "apply_kiwi_rf_attn_settings",
        "apply_qmx_live_settings",
        "apply_rtlsdr_live_settings",
        "apply_airspy_live_settings",
    ],
    "connection/actions": [
        "apply_connect_form",
        "can_connect_request",
        "can_quick_connect",
        "quick_connect_target_label",
        "quick_connect_last",
        "connect_now",
        "cancel_connection",
        "remember_host",
    ],
    "pipeline": [
        "toggle_pipeline_stage",
        "toggle_notch_bypass",
        "arm_manual_notch",
        "enabled_notches",
        "toggle_manual_notch",
        "pipeline_ingress_decim",
        "process_iq_cmds",
    ],
    "spots/logic": [
        "annotate_new_spots",
        "spot_filter_config",
        "visible_spots",
        "spot_labels",
        "clear_spots",
        "poll_scp_download",
    ],
    "ui/spots/scp": ["scp_section"],
    "ui/spots/display": [
        "spot_display_section",
        "spot_display_body",
        "spot_table",
    ],
    "ui/spots/skimmer": ["skimmer_settings_body"],
    "input": [
        "handle_arrow_pan",
        "on_af_scope_panel_changed",
        "toggle_af_scope",
        "handle_shortcuts",
    ],
    "ui/history": ["history_panel"],
    "ui/status": [
        "connection_status_pill",
        "connection_session_live",
        "connection_alias",
        "status_banner",
    ],
    "ui/popups": [
        "connection_popup",
        "iq_popup",
        "pipeline_popup",
        "shortcuts_popup",
    ],
    "ui/layout": ["side_panel_scroll", "left_panel", "right_panel"],
    "ui/connection/card": ["connection_card"],
    "ui/connection/form": ["connection_form_section"],
    "ui/connection/airspy": ["connection_airspy_section"],
    "ui/connection/rtlsdr": ["connection_rtlsdr_section"],
    "ui/connection/qmx": ["connection_qmx_section"],
    "ui/connection/kiwi_iq": ["connection_kiwi_iq_section"],
    "ui/connection/kiwi_browser": ["connection_kiwi_browser_section"],
    "ui/connection/recent": ["connection_recent_section", "connection_status_footer"],
    "ui/left/cards": [
        "smeter_card",
        "frequency_card",
        "rf_front_end_card",
        "receive_chain_card",
        "manual_notches_body",
    ],
    "ui/left/rf_controls": [
        "hardware_rf_controls",
        "kiwi_rf_controls",
        "qmx_rf_controls",
        "rtlsdr_rf_controls",
        "airspy_rf_controls",
    ],
    "ui/right/af_tuning": ["af_tuning_card"],
    "ui/right/cw_demod": [
        "cw_carrier_tools",
        "cw_demod_card",
        "agc_controls",
    ],
    "ui/right/performance": ["performance_section"],
    "ui/right/audio": ["audio_card_body"],
    "ui/console": ["console_panel"],
}

PRELUDE = """// Shared imports for `WaterfallApp` impl blocks.

use std::collections::HashSet;
use std::collections::VecDeque;
use std::sync::mpsc::Receiver;
use std::time::{Duration, Instant};

use eframe::egui;
use egui::Color32;
use egui_extras::{Column, TableBuilder};
use hfsdr::{
    decimation_factor, compose_panadapter_row, panadapter_output_bins, stretch_row_to_width,
    strongest_offset_hz, Continent,
    ContinentResolver, AgcMode, ChannelFilterKind, ChannelOffsetHz, CwChannelSettings, RowFold, SlowWaterfall, SpectrumViewMapping, Spot,
    SpotKind, SpotSort, SkimmerConfig, SkimmerDecoderKind, channel_group_delay_ms, WindowKind,
    MAX_NOTCHES,
};
use hfsdr::kiwi::protocol::{man_gain_db_below_max, man_gain_from_db_below_max};

use crate::meters::{
    self, AfScopeParams, DualAgcParams, classify_level, rf_level_dbm, show_dual_agc_loop,
    show_status_rf_meter,
};
use crate::audio::AudioOutput;
use crate::colormap::db_to_colour;
use crate::controls::{
    preset_combo_f64, preset_combo_u32, scroll_slider_f32, scroll_slider_f32_step,
    scroll_slider_log_f32, vfo_wheel_khz,
};
use crate::display_levels::{
    display_levels_initialized_after_settings_load, estimate_levels, estimate_levels_from_rows,
    lock_display_levels_for_rf_tuning, should_auto_adjust_display_levels,
};
use crate::engine::{
    ConnState, EngineCommand, EngineHandle, EngineParams, EngineStats, FFT_SIZE, WATERFALL_ROWS,
};
use crate::ham_bands;
use crate::rf_view;
use crate::popup::{
    alert_banner, chip_row, configure_popup_window, ghost_button, inline_stats, list_row,
    popup_header, popup_scroll_body, popup_section, primary_button, secondary_button,
    segment_choice, PopupHeader,
};
use crate::iq_panel::{IqPanel, IqPanelCmd, IqPanelView};
use crate::interaction::{
    PlotAction, PlotFreqMapping, PlotInteraction, PlotViewState, RIT_MAX_HZ, RIT_MIN_HZ, NOTCH_WIDTH_MAX_HZ,
    NOTCH_WIDTH_MIN_HZ, suggest_notch_offset_hz,
};
use crate::interaction::{CW_PASSBAND_MAX_HZ, CW_PASSBAND_MIN_HZ, CW_PASSBAND_NARROW_MAX_HZ};
use crate::kiwi_directory::{GeoLocation, KiwiReceiver};
use crate::log;
use crate::pipeline_flow::{PipelineFlow, PipelineSnapshot, PipelineStage};
use crate::settings::{AppSettings, NotchData};
use crate::source::{AirspySettings, ConnectRequest, KiwiSettings, QmxSettings, RtlSdrSettings, SourceKind};
use crate::spot_filter::{
    build_spot_labels, continent_index, filter_spots, SpotFilterConfig, SpotLabelConfig,
};
use crate::theme::{
    apply, attach_rich_tooltip, band_lock_toggle, clickable_badge, collapsible_section,
    panel_toggle, section_card, section_frame, section_heading, section_heading_with_tip, section_hint,
    side_panel_frame, stat_row, stage_toggle, status_panel_frame, toggle, ACCENT, MUTED, OK, WARN,
};
use crate::widgets::{
    update_trace, PanadapterPlot, SpotLabel, TraceViewKey, SCOPE_HEIGHT,
};

use crate::source::{
    is_local_source, source_kind_from_index, source_kind_index, source_kind_label,
    source_kind_labels,
};
use crate::app::codec::{
    agc_mode_from_u8, agc_mode_to_u8, channel_filter_from_u8, channel_filter_to_u8,
    normalize_waterfall_avg, plot_action_changes_view, skimmer_config_from_settings,
    skimmer_decoder_from_u8, skimmer_decoder_to_u8, spot_sort_from_u8, spot_sort_to_u8,
    window_choice, window_from_u8, window_to_u8,
};
use crate::app::constants::*;
"""

CODEC_USE = """use hfsdr::{
    AgcMode, ChannelFilterKind, SkimmerConfig, SkimmerDecoderKind, SpotSort, WindowKind,
    DecoderParams, EnvelopeSettings,
};

use crate::settings::AppSettings;
use crate::interaction::PlotAction;
"""

CONSTANTS_HEADER = """//! Layout presets, band plans, and waterfall texture cache keys.

use hfsdr::SpectrumViewMapping;

"""


def method_start_with_docs(lines: list[str], fn_line: int) -> int:
    """1-based line of `fn`, extended upward through doc comments."""
    start = fn_line
    i = fn_line - 2
    while i >= 0:
        stripped = lines[i].strip()
        if stripped.startswith("///"):
            start = i + 1
            i -= 1
            while i >= 0 and lines[i].strip().startswith("///"):
                start = i + 1
                i -= 1
            break
        if stripped == "":
            i -= 1
            continue
        break
    return start


def parse_methods(lines: list[str]) -> dict[str, tuple[int, int]]:
    """Return method_name -> (start_line, end_line) 1-based inclusive."""
    starts: list[tuple[str, int]] = []
    in_impl = False
    for i, line in enumerate(lines, 1):
        if line.startswith("impl WaterfallApp"):
            in_impl = True
            continue
        if not in_impl:
            continue
        if line == "}" and i > 540:
            break
        m = re.match(r"    fn (\w+)", line)
        if m:
            starts.append((m.group(1), i))

    result: dict[str, tuple[int, int]] = {}
    for idx, (name, fn_line) in enumerate(starts):
        start = method_start_with_docs(lines, fn_line)
        if idx + 1 < len(starts):
            next_start = method_start_with_docs(lines, starts[idx + 1][1])
            end = next_start - 1
        else:
            end = None
            for j in range(fn_line, len(lines) + 1):
                if lines[j - 1] == "}":
                    end = j - 1
                    break
        assert end is not None and end >= start
        result[name] = (start, end)
    return result


def main() -> None:
    import sys

    if "--regenerate-impl" in sys.argv:
        out_path = DEFAULT_IMPL_OUT
        if "--out" in sys.argv:
            idx = sys.argv.index("--out")
            out_path = Path(sys.argv[idx + 1])
        regenerate_impl(out_path)
        return

    if not SRC.exists():
        raise SystemExit(f"{SRC} not found (use --regenerate-impl to rebuild impl_methods.inc)")

    text = read_text(SRC)
    lines = text.splitlines(keepends=True)

    methods = parse_methods([l.rstrip("\n") for l in lines])

    assigned: set[str] = set()
    for names in METHOD_GROUPS.values():
        assigned.update(names)

    missing = set(methods) - assigned
    extra = assigned - set(methods)
    if missing:
        raise SystemExit(f"Methods not assigned to a module: {sorted(missing)}")
    if extra:
        raise SystemExit(f"Unknown methods in groups: {sorted(extra)}")

    OUT.mkdir(parents=True, exist_ok=True)

    # prelude.rs (included by impl submodules)
    write_text(OUT / "prelude.rs", PRELUDE.lstrip())

    # constants.rs (lines 72-230)
    constants_text = CONSTANTS_HEADER + "".join(lines[71:230])
    constants_text = re.sub(
        r"^const ",
        "pub(crate) const ",
        constants_text,
        flags=re.MULTILINE,
    )
    constants_text = re.sub(
        r"^struct ",
        "pub(crate) struct ",
        constants_text,
        flags=re.MULTILINE,
    )
    constants_text = re.sub(
        r"^(\s+)(label|center_hz|segment_hz):",
        r"\1pub(crate) \2:",
        constants_text,
        flags=re.MULTILINE,
    )
    constants_text = constants_text.replace(
        "    fn from(storage:",
        "    pub(crate) fn from_storage(storage:",
    )
    constants_text = re.sub(
        r"^    fn (from_view|from)\(",
        r"    pub(crate) fn \1(",
        constants_text,
        flags=re.MULTILINE,
    )
    constants_text = constants_text.replace(
        "    pub(crate) fn from(",
        "    pub(crate) fn from_storage(",
    )
    write_text(OUT / "constants.rs", constants_text)

    # codec.rs (free functions after main impl, excluding eframe::App)
    codec_body = "".join(lines[5043:5229]) + "".join(lines[5330:5337])
    codec_body = re.sub(r"^fn ", "pub(crate) fn ", codec_body, flags=re.MULTILINE)
    write_text(
        OUT / "codec.rs",
        "//! Settings serialization helpers and small shared utilities.\n\n"
        + "use eframe::egui;\n"
        + CODEC_USE
        + "\n"
        + codec_body,
    )

    # Extract method bodies per group (included into the single impl block in mod.rs)
    method_sections: list[str] = []
    for rel_path, names in METHOD_GROUPS.items():
        path = OUT / "methods" / f"{rel_path}.rs"
        path.parent.mkdir(parents=True, exist_ok=True)
        chunks: list[str] = []
        for name in names:
            start, end = methods[name]
            chunks.append("".join(lines[start - 1 : end]))
        body = "\n\n".join(chunks)
        write_text(
            path,
            f"// `{rel_path}` — `WaterfallApp` methods.\n\n" + body,
        )
        method_sections.append(
            f"    // --- {rel_path} ---\n" + body
        )

    write_impl_methods(DEFAULT_IMPL_OUT)

    # mod.rs: header, struct, new, eframe::App
    mod_header = """//! Waterfall application state and rendering.
//!
//! The UI thread owns no DSP: it pushes settings to the [`crate::engine`] worker,
//! drains spectrum rows / status / spots it publishes, renders, and repaints
//! lazily. Connection lifecycle (connect, slow/unstable warnings, auto-reconnect)
//! is driven by the engine and surfaced here.

#![allow(unused_imports)]

mod codec;
mod constants;

pub(crate) use constants::*;

include!("prelude.rs");

pub struct WaterfallApp {
"""

    struct_body = "".join(lines[232:371])  # from `pub struct` fields only - skip first line
    # lines[231] is `pub struct WaterfallApp {` - lines[232:371] is fields + closing brace

    new_body = "".join(lines[372:540])  # impl opening + new

    eframe_impl = """impl eframe::App for WaterfallApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        if !self.themed {
            apply(&ctx);
            self.themed = true;
        }

        if let Some(mut req) = self.pending_connect.take() {
            self.form_kind = req.kind;
            self.form_host = req.host.clone();
            self.form_port = req.port;
            self.form_kiwi = req.kiwi.clone();
            if req.sample_rate != 0 {
                self.form_sample_rate = req.sample_rate;
            }
            self.form_airspy = req.airspy.clone();
            self.form_rtlsdr = req.rtlsdr.clone();
            self.form_qmx = req.qmx.clone();
            self.center_khz = req.center_hz / 1000.0;
            self.clamp_center_to_ham_bands();
            req.center_hz = self.center_khz * 1000.0;
            self.last_center_khz = self.center_khz;
            log::info(format!("connecting to {}", req.label()));
            self.engine.send(EngineCommand::Connect(req));
        }

        self.poll_scp_download();
        self.poll_kiwi_directory();
        self.handle_shortcuts(&ctx);
        self.pump_engine();
        self.frame_visible_spots = self.visible_spots();

        self.update_plot_hover(&ctx);
        egui::Panel::top("status")
            .frame(status_panel_frame())
            .show_inside(ui, |ui| self.status_banner(ui));

        if self.show_left || self.show_smeter {
            egui::Panel::left("left")
                .resizable(true)
                .frame(side_panel_frame())
                .size_range(LEFT_PANEL_MIN_W..=LEFT_PANEL_MAX_W)
                .default_size(if self.show_smeter && !self.show_left {
                    LEFT_PANEL_MIN_W
                } else {
                    300.0
                })
                .show_inside(ui, |ui| self.left_panel(ui));
        }

        if self.show_right {
            egui::Panel::right("controls")
                .resizable(true)
                .frame(side_panel_frame())
                .size_range(RIGHT_PANEL_MIN_W..=RIGHT_PANEL_MAX_W)
                .default_size(330.0)
                .show_inside(ui, |ui| self.right_panel(ui));
        }

        if self.show_history {
            egui::Panel::bottom("history")
                .resizable(true)
                .default_size(150.0)
                .show_inside(ui, |ui| self.history_panel(ui));
        }

        if self.show_console {
            egui::Panel::bottom("console")
                .resizable(true)
                .default_size(160.0)
                .show_inside(ui, |ui| self.console_panel(ui));
        }

        egui::CentralPanel::default()
            .frame(egui::Frame::NONE)
            .show_inside(ui, |ui| {
                self.central_panel(ui);
            });

        self.latest_frame_tick = false;

        self.connection_popup(&ctx);
        self.iq_popup(&ctx);
        self.pipeline_popup(&ctx);
        self.shortcuts_popup(&ctx);

        self.apply_radio_settings();
        self.autosave();

        let frame_ms = (1000 / self.effective_target_fps().max(1)).max(8) as u64;
        ctx.request_repaint_after(Duration::from_millis(frame_ms));
    }

    fn on_exit(&mut self) {
        self.current_settings().save();
        self.engine.shutdown_now();
    }
}
"""

    # Fix struct: use lines 233-371 (fields only with opening brace from mod_header)
    fields = "".join(lines[232:371])
    if not fields.lstrip().startswith("engine:"):
        # lines[231] is struct line, [232] starts fields
        fields = "".join(lines[232:371])

    mod_rs = (
        mod_header
        + fields
        + "\n"
        + new_body.rstrip()
        + "\n}\n\n"
        + 'include!(concat!(env!("OUT_DIR"), "/waterfall_impl_methods.inc"));\n\n'
        + eframe_impl
    )
    write_text(OUT / "mod.rs", mod_rs)

    SRC.unlink()
    print(f"Split {SRC} into {OUT}/")


def trim_method_group(body: str) -> str:
    """Drop header comments; keep `#[cfg]` attributes before the first method."""
    lines = body.splitlines(keepends=True)
    fn_idx = None
    for i, line in enumerate(lines):
        if line.startswith("    fn "):
            fn_idx = i
            break
    if fn_idx is None:
        return body
    start = fn_idx
    while start > 0 and lines[start - 1].strip().startswith("#["):
        start -= 1
    return "".join(lines[start:])


DEFAULT_IMPL_OUT = OUT / "impl_methods.inc"


def write_impl_methods(out_path: Path) -> None:
    """Concatenate methods/*.rs into a single `impl WaterfallApp` block."""
    sections: list[str] = []
    seen: set[str] = set()

    for rel_path, _names in METHOD_GROUPS.items():
        path = OUT / "methods" / f"{rel_path}.rs"
        if not path.exists():
            raise SystemExit(f"Missing method group file: {path}")
        seen.add(str(path.resolve()))
        body = trim_method_group(read_text(path))
        sections.append(f"    // --- {rel_path} ---\n{body}")

    for path in sorted((OUT / "methods").rglob("*.rs")):
        if str(path.resolve()) not in seen:
            raise SystemExit(
                f"Orphan method file {path} — add it to METHOD_GROUPS in split_app_rs.py"
            )

    impl_body = "impl WaterfallApp {\n" + "\n".join(sections) + "\n}\n"
    impl_body = impl_body.replace("StorageKey::from(", "StorageKey::from_storage(")
    out_path.parent.mkdir(parents=True, exist_ok=True)
    write_text(out_path, impl_body)


def regenerate_impl(out_path: Path | None = None) -> None:
    """Rebuild impl_methods.inc from methods/*.rs (after editing method groups)."""
    out_path = out_path or DEFAULT_IMPL_OUT
    write_impl_methods(out_path)
    print(f"Regenerated {out_path} from {len(METHOD_GROUPS)} method groups.")


if __name__ == "__main__":
    main()
