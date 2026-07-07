//! Interactive receive-pipeline flow diagram (node graph with Bézier links).

use std::collections::HashMap;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};
use hfsdr::{AgcMode, ChannelFilterKind, CwChannelSettings, CwDetectorMode};

use crate::engine::EngineStats;
use crate::theme::{chip_hovered, ACCENT, MUTED, OK, TRACE, WARN};

/// Bypassable stage in the receive pipeline (click node in the flow diagram).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PipelineStage {
    NoiseBlanker,
    ManualNotches,
    ListenNco,
    DecimatorFir,
    ChannelFir,
    IqApf,
    IqWiener,
    Agc,
    Bfo,
    Sidetone,
    Apf,
    AutoNotch,
    NoiseReduction,
    Squelch,
    Skimmer,
    AudioOutput,
}

impl PipelineStage {
    pub const fn label(self) -> &'static str {
        match self {
            Self::NoiseBlanker => "Noise blanker",
            Self::ManualNotches => "IQ notches",
            Self::ListenNco => "NCO / listen shift",
            Self::DecimatorFir => "Decimator anti-alias FIR",
            Self::ChannelFir => "Channel filter",
            Self::IqApf => "IQ peak filter",
            Self::IqWiener => "IQ Wiener NR",
            Self::Agc => "AGC",
            Self::Bfo => "BFO detector",
            Self::Sidetone => "Sidetone envelope",
            Self::Apf => "Audio peak filter",
            Self::AutoNotch => "Auto-notch",
            Self::NoiseReduction => "Noise reduction",
            Self::Squelch => "Squelch",
            Self::Skimmer => "Skimmer",
            Self::AudioOutput => "Audio output",
        }
    }

    pub const fn is_diagnostic(self) -> bool {
        matches!(
            self,
            Self::ListenNco | Self::DecimatorFir | Self::ChannelFir | Self::Bfo
        )
    }
}

const NODE_W: f32 = 136.0;
const NODE_H: f32 = 48.0;
const CANVAS_W: f32 = 820.0;
const CANVAS_H: f32 = 700.0;
/// Matches `WidebandCwIngress` / `IqAudioDemod` wideband path selection.
const LISTEN_WIDEBAND_RATE_HZ: f32 = 96_000.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum NodeId {
    Source,
    RingBuffer,
    RfGain,
    IngressDecim,
    /// Fused NCO + decimation (`WidebandCwIngress`) before the baseband CW chain.
    WidebandIngress,
    NoiseBlanker,
    Nco,
    Decimator,
    Notches,
    ChannelFir,
    IqApf,
    IqWiener,
    Agc,
    Bfo,
    Sidetone,
    Apf,
    AutoNotch,
    NoiseReduction,
    Squelch,
    AudioOut,
    SpectrumFront,
    Fft,
    Waterfall,
    Skimmer,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PortSide {
    Left,
    Right,
}

#[derive(Clone, Copy, Debug)]
struct Port {
    node: NodeId,
    side: PortSide,
    /// Position along the edge, 0 = top, 1 = bottom.
    t: f32,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EdgeStyle {
    Solid,
    /// Spectrum peak-hold metadata (not IQ).
    PeakHold,
}

#[derive(Clone, Copy, Debug)]
struct Edge {
    from: Port,
    to: Port,
    accent: bool,
    active: bool,
    style: EdgeStyle,
}

/// Live pipeline state for labels and on/off styling.
pub struct PipelineSnapshot<'a> {
    pub source_label: &'a str,
    pub streaming: bool,
    pub device_rate_hz: f32,
    pub ingress_decim: usize,
    pub cw: &'a CwChannelSettings,
    pub skimmer_enabled: bool,
    pub audio_enabled: bool,
    /// User software RF gain (dB); applied once before all IQ consumers.
    pub rf_gain_db: f32,
    pub stats: &'a EngineStats,
}

/// Draggable node offsets (session-local layout).
#[derive(Clone, Debug, Default)]
pub struct PipelineLayout {
    offsets: HashMap<NodeId, Vec2>,
}

impl PipelineLayout {
    fn offset(&self, id: NodeId) -> Vec2 {
        self.offsets.get(&id).copied().unwrap_or(Vec2::ZERO)
    }

    fn add_offset(&mut self, id: NodeId, delta: Vec2) {
        let entry = self.offsets.entry(id).or_insert(Vec2::ZERO);
        *entry += delta;
    }

    fn reset(&mut self) {
        self.offsets.clear();
    }
}

pub struct PipelineFlow {
    layout: PipelineLayout,
}

impl PipelineFlow {
    pub fn new() -> Self {
        Self {
            layout: PipelineLayout::default(),
        }
    }

    /// Draw the diagram; returns stages the user clicked to bypass / restore.
    pub fn show(&mut self, ui: &mut Ui, snap: &PipelineSnapshot<'_>) -> Vec<PipelineStage> {
        let mut toggled = Vec::new();
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Drag blocks · use toggles to bypass stages (A/B)")
                .small()
                .color(MUTED),
            );
            if ui.small_button("Reset layout").clicked() {
                self.layout.reset();
            }
        });
        ui.add_space(4.0);
        legend_row(ui, snap);
        ui.add_space(6.0);

        let (rect, response) =
            ui.allocate_exact_size(Vec2::new(CANVAS_W, CANVAS_H), Sense::click_and_drag());
        let painter = ui.painter_at(rect);
        painter.rect_filled(rect, 8.0, Color32::from_rgb(16, 20, 28));
        painter.rect(
            rect,
            8.0,
            Color32::from_rgb(34, 42, 56),
            Stroke::new(1.0, Color32::from_rgb(48, 58, 76)),
            StrokeKind::Inside,
        );

        let graph = build_graph(snap);
        let mut rects = HashMap::new();
        for node in &graph.nodes {
            let base = default_pos(node.id);
            let offset = self.layout.offset(node.id);
            let node_rect = Rect::from_min_size(rect.min + base.to_vec2() + offset, node.size);
            rects.insert(node.id, node_rect);
        }

        for edge in &graph.edges {
            let Some(from_rect) = rects.get(&edge.from.node) else {
                continue;
            };
            let Some(to_rect) = rects.get(&edge.to.node) else {
                continue;
            };
            let from = port_pos(*from_rect, edge.from);
            let to = port_pos(*to_rect, edge.to);
            draw_bezier_link(&painter, from, to, edge.active, edge.accent, edge.style);
        }

        let mut drag_delta = Vec2::ZERO;
        let mut dragged: Option<NodeId> = None;
        for node in &graph.nodes {
            let Some(node_rect) = rects.get(&node.id).copied() else {
                continue;
            };
            let body_rect = if node.toggle.is_some() || node.extra_toggle.is_some() {
                node_rect.with_max_x(node_rect.right() - 26.0)
            } else {
                node_rect
            };
            let id = ui.id().with(("pipeline_node", node.id as u8));
            let node_resp = ui.interact(body_rect, id, Sense::drag());
            if node_resp.dragged() {
                drag_delta = node_resp.drag_delta();
                dragged = Some(node.id);
            }
            let node_hovered = chip_hovered(ui, body_rect, &node_resp);
            paint_node(&painter, node_rect, &node, node_hovered);
        }

        for node in &graph.nodes {
            let Some(node_rect) = rects.get(&node.id).copied() else {
                continue;
            };
            if let Some(stage) = node.toggle {
                paint_stage_toggle(ui, node_rect, stage, node.enabled, 8.0, &mut toggled);
            }
            if let Some(stage) = node.extra_toggle {
                paint_stage_toggle(
                    ui,
                    node_rect,
                    stage,
                    node.extra_toggle_enabled,
                    28.0,
                    &mut toggled,
                );
            }
        }

        if let Some(id) = dragged {
            self.layout.add_offset(id, drag_delta);
        }

        if response.hovered() && dragged.is_none() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }

        toggled
    }
}

struct NodeSpec {
    id: NodeId,
    title: String,
    subtitle: String,
    enabled: bool,
    kind: NodeKind,
    size: Vec2,
    /// Click toggles bypass when `Some` (same flags as the settings panel).
    toggle: Option<PipelineStage>,
    /// Second diagnostic toggle (wideband listen ingress: NCO + decim).
    extra_toggle: Option<PipelineStage>,
    extra_toggle_enabled: bool,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum NodeKind {
    Source,
    Sink,
    Process,
}

struct Graph {
    nodes: Vec<NodeSpec>,
    edges: Vec<Edge>,
}

fn build_graph(snap: &PipelineSnapshot<'_>) -> Graph {
    let ingress_on = snap.ingress_decim > 1;
    let wideband_listen = snap.device_rate_hz > LISTEN_WIDEBAND_RATE_HZ;
    let listen_audio_ksps = snap.device_rate_hz
        / decim_factor(snap.cw.decimation, snap.device_rate_hz) as f32
        / 1000.0;
    let decim_filter_label = match snap.cw.decim_filter {
        ChannelFilterKind::LinearFir => "FIR",
        ChannelFilterKind::Iir2Pole => "IIR",
    };
    let notch_count = snap
        .cw
        .notches
        .iter()
        .filter(|n| n.enabled)
        .count();
    let any_notch = notch_count > 0;

    let mut nodes = vec![
        node(
            NodeId::Source,
            "IQ source",
            snap.source_label.to_string(),
            snap.streaming,
            NodeKind::Source,
        ),
        node_fixed(
            NodeId::RingBuffer,
            "IQ ring",
            ring_buffer_subtitle(snap),
            ring_buffer_healthy(snap),
            NodeKind::Process,
        ),
        node_fixed(
            NodeId::RfGain,
            "Software RF gain",
            rf_gain_subtitle(snap.rf_gain_db),
            snap.rf_gain_db.abs() > 0.05,
            NodeKind::Process,
        ),
        node_toggle(
            NodeId::NoiseBlanker,
            "Noise blanker",
            if wideband_listen {
                "impulse blanker @ baseband".to_string()
            } else {
                "IQ impulse blanker".to_string()
            },
            snap.cw.noise_blanker.enabled,
            NodeKind::Process,
            PipelineStage::NoiseBlanker,
        ),
        node_toggle(
            NodeId::Notches,
            "IQ notches",
            if any_notch {
                format!("{notch_count} active")
            } else {
                "manual · off".to_string()
            },
            any_notch,
            NodeKind::Process,
            PipelineStage::ManualNotches,
        ),
        node_toggle(
            NodeId::ChannelFir,
            channel_filter_title(snap.cw.channel_filter),
            format!(
                "BW {:.0} Hz · {}",
                snap.cw.passband_hz,
                match snap.cw.channel_filter {
                    ChannelFilterKind::LinearFir => "FIR",
                    ChannelFilterKind::Iir2Pole => "IIR",
                }
            ),
            !snap.cw.diagnostic.channel_fir,
            NodeKind::Process,
            PipelineStage::ChannelFir,
        ),
        node_toggle(
            NodeId::IqApf,
            "IQ peak filter",
            format!("±{:.0} Hz · {:.1}×", snap.cw.iq_apf.width_hz, snap.cw.iq_apf.gain),
            snap.cw.iq_apf.enabled,
            NodeKind::Process,
            PipelineStage::IqApf,
        ),
        node_toggle(
            NodeId::IqWiener,
            "IQ Wiener NR",
            format!("level {:.0}%", snap.cw.iq_wiener.level * 100.0),
            snap.cw.iq_wiener.enabled,
            NodeKind::Process,
            PipelineStage::IqWiener,
        ),
        node_toggle(
            NodeId::Agc,
            "AGC",
            if snap.cw.agc.enabled {
                format!(
                    "{} gain",
                    match snap.cw.agc_mode {
                        AgcMode::Envelope => "envelope",
                        AgcMode::Hang => "hang",
                        AgcMode::DualLoop => "dual-loop",
                        AgcMode::Lookahead => "lookahead",
                    }
                )
            } else {
                format!("manual {:.1}×", snap.cw.agc.manual_gain)
            },
            snap.cw.agc.enabled,
            NodeKind::Process,
            PipelineStage::Agc,
        ),
        node_toggle(
            NodeId::Bfo,
            "BFO detector",
            {
                let det = match snap.cw.detector_mode {
                    CwDetectorMode::Product => "product",
                    CwDetectorMode::Coherent => "coherent",
                    CwDetectorMode::MatchedDit => "matched dit",
                };
                format!("{det} · {:.0} Hz BFO", snap.cw.bfo_hz)
            },
            !snap.cw.diagnostic.bfo,
            NodeKind::Process,
            PipelineStage::Bfo,
        ),
        node_toggle(
            NodeId::Sidetone,
            "Sidetone envelope",
            format!(
                "{:.0}/{:.0} ms",
                snap.cw.sidetone_envelope.rise_ms, snap.cw.sidetone_envelope.fall_ms
            ),
            snap.cw.sidetone_envelope.enabled,
            NodeKind::Process,
            PipelineStage::Sidetone,
        ),
        node_toggle(
            NodeId::Apf,
            "Audio peak filter",
            "resonant boost".to_string(),
            snap.cw.apf.enabled,
            NodeKind::Process,
            PipelineStage::Apf,
        ),
        node_toggle(
            NodeId::AutoNotch,
            "Auto-notch",
            format!("guard ±{:.0} Hz", snap.cw.auto_notch.guard_hz),
            snap.cw.auto_notch.enabled,
            NodeKind::Process,
            PipelineStage::AutoNotch,
        ),
        node_toggle(
            NodeId::NoiseReduction,
            "Noise reduction",
            "post-demod LMS".to_string(),
            snap.cw.noise_reduction.enabled,
            NodeKind::Process,
            PipelineStage::NoiseReduction,
        ),
        node_toggle(
            NodeId::Squelch,
            "Squelch",
            format!("hang {:.0} ms", snap.cw.squelch.hang_ms),
            snap.cw.squelch.enabled,
            NodeKind::Process,
            PipelineStage::Squelch,
        ),
        node_toggle(
            NodeId::AudioOut,
            "Audio sink",
            snap.stats
                .audio_device
                .clone()
                .unwrap_or_else(|| "speakers".to_string()),
            snap.audio_enabled,
            NodeKind::Sink,
            PipelineStage::AudioOutput,
        ),
        node_fixed(
            NodeId::SpectrumFront,
            "Spectrum front",
            if snap.stats.spectrum_zoomed {
                format!("zoom ×{}", snap.stats.spectrum_decim)
            } else {
                "mix + decim".to_string()
            },
            true,
            NodeKind::Process,
        ),
        node_fixed(
            NodeId::Fft,
            "FFT",
            format!(
                "{} @ {:.1} kS/s",
                snap.stats.spectrum_fft,
                snap.stats.spectrum_rate / 1000.0
            ),
            true,
            NodeKind::Process,
        ),
        node_fixed(
            NodeId::Waterfall,
            "Waterfall sink",
            format!("{} rows/pump", snap.stats.spectrum_rows_per_pump),
            true,
            NodeKind::Sink,
        ),
        node_toggle(
            NodeId::Skimmer,
            "Skimmer",
            format!(
                "IQ + peak hold · {} ch",
                snap.stats.skimmer_channels
            ),
            snap.skimmer_enabled,
            NodeKind::Process,
            PipelineStage::Skimmer,
        ),
    ];

    if wideband_listen {
        nodes.push(node_dual_toggle(
            NodeId::WidebandIngress,
            "Listen ingress",
            format!(
                "Listen {:.0} Hz · {decim_filter_label} → {listen_audio_ksps:.1} kS/s",
                snap.cw.listen_offset_hz.hz(),
            ),
            !snap.cw.diagnostic.listen_nco,
            !snap.cw.diagnostic.decim_fir,
            NodeKind::Process,
            PipelineStage::ListenNco,
            PipelineStage::DecimatorFir,
        ));
    } else {
        nodes.push(node_toggle(
            NodeId::Nco,
            "NCO shift",
            format!("Listen {:.0} Hz", snap.cw.listen_offset_hz.hz()),
            !snap.cw.diagnostic.listen_nco,
            NodeKind::Process,
            PipelineStage::ListenNco,
        ));
        nodes.push(node_toggle(
            NodeId::Decimator,
            format!("Decimator {decim_filter_label}"),
            format!("→ {listen_audio_ksps:.1} kS/s · {decim_filter_label}"),
            !snap.cw.diagnostic.decim_fir,
            NodeKind::Process,
            PipelineStage::DecimatorFir,
        ));
    }

    if ingress_on {
        let ingress_sub = if snap.stats.pipeline.dual_ring {
            format!(
                "÷{} → {:.0} kS/s · dual ring",
                snap.ingress_decim,
                snap.stats.sample_rate / 1000.0
            )
        } else {
            format!(
                "÷{} → {:.0} kS/s · spectrum + skimmer",
                snap.ingress_decim,
                snap.stats.sample_rate / 1000.0
            )
        };
        nodes.push(node_fixed(
            NodeId::IngressDecim,
            format!("Ingress {decim_filter_label}"),
            ingress_sub,
            true,
            NodeKind::Process,
        ));
    }

    let listen_tail = [
        NodeId::Notches,
        NodeId::ChannelFir,
        NodeId::IqApf,
        NodeId::IqWiener,
        NodeId::Agc,
        NodeId::Bfo,
        NodeId::Sidetone,
        NodeId::Apf,
        NodeId::AutoNotch,
        NodeId::NoiseReduction,
        NodeId::Squelch,
        NodeId::AudioOut,
    ];
    let listen_head: Vec<NodeId> = if wideband_listen {
        vec![NodeId::WidebandIngress, NodeId::NoiseBlanker]
    } else {
        vec![NodeId::NoiseBlanker, NodeId::Nco, NodeId::Decimator]
    };
    let listen_chain: Vec<NodeId> = listen_head
        .into_iter()
        .chain(listen_tail.iter().copied())
        .collect();
    let spectrum_chain = [NodeId::SpectrumFront, NodeId::Fft, NodeId::Waterfall];

    let mut edges = Vec::new();

    edges.push(edge(
        port(NodeId::Source, PortSide::Right, 0.5),
        port(NodeId::RingBuffer, PortSide::Left, 0.5),
        true,
        snap.streaming,
        EdgeStyle::Solid,
    ));
    edges.push(edge(
        port(NodeId::RingBuffer, PortSide::Right, 0.5),
        port(NodeId::RfGain, PortSide::Left, 0.5),
        true,
        snap.streaming,
        EdgeStyle::Solid,
    ));

    if ingress_on {
        edges.push(edge(
            port(NodeId::RfGain, PortSide::Right, 0.35),
            port(NodeId::IngressDecim, PortSide::Left, 0.5),
            true,
            snap.streaming,
            EdgeStyle::Solid,
        ));
        edges.push(edge(
            port(NodeId::RfGain, PortSide::Right, 0.65),
            port(listen_chain[0], PortSide::Left, 0.5),
            true,
            snap.streaming,
            EdgeStyle::Solid,
        ));
        edges.push(edge(
            port(NodeId::IngressDecim, PortSide::Right, 0.35),
            port(spectrum_chain[0], PortSide::Left, 0.5),
            false,
            snap.streaming,
            EdgeStyle::Solid,
        ));
        edges.push(edge(
            port(NodeId::IngressDecim, PortSide::Right, 0.65),
            port(NodeId::Skimmer, PortSide::Left, 0.35),
            false,
            snap.streaming && snap.skimmer_enabled,
            EdgeStyle::Solid,
        ));
    } else {
        edges.push(edge(
            port(NodeId::RfGain, PortSide::Right, 0.25),
            port(listen_chain[0], PortSide::Left, 0.5),
            true,
            snap.streaming,
            EdgeStyle::Solid,
        ));
        edges.push(edge(
            port(NodeId::RfGain, PortSide::Right, 0.5),
            port(spectrum_chain[0], PortSide::Left, 0.5),
            false,
            snap.streaming,
            EdgeStyle::Solid,
        ));
        edges.push(edge(
            port(NodeId::RfGain, PortSide::Right, 0.75),
            port(NodeId::Skimmer, PortSide::Left, 0.35),
            false,
            snap.streaming && snap.skimmer_enabled,
            EdgeStyle::Solid,
        ));
    }

    chain_edges(&mut edges, &listen_chain, true);
    chain_edges(&mut edges, &spectrum_chain, false);

    edges.push(edge(
        port(NodeId::Fft, PortSide::Right, 0.65),
        port(NodeId::Skimmer, PortSide::Left, 0.75),
        false,
        snap.streaming && snap.skimmer_enabled,
        EdgeStyle::PeakHold,
    ));

    Graph { nodes, edges }
}

fn chain_edges(edges: &mut Vec<Edge>, chain: &[NodeId], accent: bool) {
    for w in chain.windows(2) {
        let [a, b] = w else { continue };
        edges.push(edge(
            port(*a, PortSide::Right, 0.5),
            port(*b, PortSide::Left, 0.5),
            accent,
            true,
            EdgeStyle::Solid,
        ));
    }
}

fn paint_stage_toggle(
    ui: &mut Ui,
    node_rect: Rect,
    stage: PipelineStage,
    is_on: bool,
    top: f32,
    toggled: &mut Vec<PipelineStage>,
) {
    let toggle_rect = Rect::from_min_size(
        Pos2::new(node_rect.right() - 24.0, node_rect.top() + top),
        Vec2::new(20.0, 20.0),
    );
    let label = stage.label();
    let mut switch = is_on;
    let toggle_resp = ui.put(toggle_rect, |ui: &mut Ui| {
        ui.add(egui::Checkbox::without_text(&mut switch))
    });
    if toggle_resp.changed() && switch != is_on {
        toggled.push(stage);
    }
    toggle_resp.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Checkbox,
            true,
            format!("{label} {}", if switch { "on" } else { "bypassed" }),
        )
    });
}

fn node_stage_active(node: &NodeSpec) -> bool {
    if node.extra_toggle.is_some() {
        node.enabled && node.extra_toggle_enabled
    } else {
        node.enabled
    }
}

fn channel_filter_title(kind: ChannelFilterKind) -> String {
    match kind {
        ChannelFilterKind::LinearFir => "Channel FIR".to_string(),
        ChannelFilterKind::Iir2Pole => "Channel IIR".to_string(),
    }
}

fn node_fixed(
    id: NodeId,
    title: impl Into<String>,
    subtitle: String,
    enabled: bool,
    kind: NodeKind,
) -> NodeSpec {
    NodeSpec {
        id,
        title: title.into(),
        subtitle,
        enabled,
        kind,
        size: Vec2::new(NODE_W, NODE_H),
        toggle: None,
        extra_toggle: None,
        extra_toggle_enabled: false,
    }
}

fn node_toggle(
    id: NodeId,
    title: impl Into<String>,
    subtitle: String,
    enabled: bool,
    kind: NodeKind,
    stage: PipelineStage,
) -> NodeSpec {
    NodeSpec {
        id,
        title: title.into(),
        subtitle,
        enabled,
        kind,
        size: Vec2::new(NODE_W, NODE_H),
        toggle: Some(stage),
        extra_toggle: None,
        extra_toggle_enabled: false,
    }
}

fn node_dual_toggle(
    id: NodeId,
    title: impl Into<String>,
    subtitle: String,
    nco_on: bool,
    decim_on: bool,
    kind: NodeKind,
    primary: PipelineStage,
    secondary: PipelineStage,
) -> NodeSpec {
    NodeSpec {
        id,
        title: title.into(),
        subtitle,
        enabled: nco_on,
        kind,
        size: Vec2::new(NODE_W, NODE_H),
        toggle: Some(primary),
        extra_toggle: Some(secondary),
        extra_toggle_enabled: decim_on,
    }
}

fn node(id: NodeId, title: impl Into<String>, subtitle: String, enabled: bool, kind: NodeKind) -> NodeSpec {
    node_fixed(id, title, subtitle, enabled, kind)
}

fn edge(from: Port, to: Port, accent: bool, active: bool, style: EdgeStyle) -> Edge {
    Edge {
        from,
        to,
        accent,
        active,
        style,
    }
}

fn port(node: NodeId, side: PortSide, t: f32) -> Port {
    Port { node, side, t }
}

fn default_pos(id: NodeId) -> Pos2 {
    match id {
        NodeId::Source => Pos2::new(346.0, 8.0),
        NodeId::RingBuffer => Pos2::new(346.0, 56.0),
        NodeId::RfGain => Pos2::new(346.0, 104.0),
        NodeId::IngressDecim => Pos2::new(346.0, 152.0),
        NodeId::WidebandIngress => Pos2::new(24.0, 148.0),
        NodeId::NoiseBlanker => Pos2::new(24.0, 208.0),
        NodeId::Nco => Pos2::new(24.0, 268.0),
        NodeId::Decimator => Pos2::new(24.0, 328.0),
        NodeId::Notches => Pos2::new(24.0, 388.0),
        NodeId::ChannelFir => Pos2::new(24.0, 448.0),
        NodeId::IqApf => Pos2::new(24.0, 508.0),
        NodeId::IqWiener => Pos2::new(24.0, 568.0),
        NodeId::Agc => Pos2::new(24.0, 628.0),
        NodeId::Bfo => Pos2::new(168.0, 208.0),
        NodeId::Sidetone => Pos2::new(168.0, 268.0),
        NodeId::Apf => Pos2::new(168.0, 328.0),
        NodeId::AutoNotch => Pos2::new(168.0, 388.0),
        NodeId::NoiseReduction => Pos2::new(168.0, 448.0),
        NodeId::Squelch => Pos2::new(168.0, 508.0),
        NodeId::AudioOut => Pos2::new(168.0, 568.0),
        NodeId::SpectrumFront => Pos2::new(346.0, 224.0),
        NodeId::Fft => Pos2::new(346.0, 304.0),
        NodeId::Waterfall => Pos2::new(346.0, 384.0),
        NodeId::Skimmer => Pos2::new(668.0, 240.0),
    }
}

fn ring_buffer_subtitle(snap: &PipelineSnapshot<'_>) -> String {
    let fill_pct = snap.stats.iq_buffer_fill * 100.0;
    let secs = snap.stats.iq_buffer_secs;
    let mut parts = vec![format!("{fill_pct:.0}% · {secs:.2}s queued")];
    if snap.stats.iq_playback {
        parts.push("playback ring".into());
    } else if snap.stats.pipeline.dual_ring {
        parts.push("dual ring".into());
    }
    let pump_drops = snap.stats.pipeline.iq_dropped_catchup;
    let bridge_drops = snap
        .stats
        .pipeline
        .raw_ring_dropped
        .saturating_add(snap.stats.pipeline.decim_ring_dropped);
    if pump_drops > 0 {
        parts.push(format!("catch-up −{pump_drops}"));
    }
    if bridge_drops > 0 {
        parts.push(format!("bridge −{bridge_drops}"));
    }
    if snap.stats.dropped > 0 {
        parts.push(format!("dev −{}", snap.stats.dropped));
    }
    if snap.stats.last_drain > 0 {
        parts.push(format!("drain {}", snap.stats.last_drain));
    }
    parts.join(" · ")
}

fn ring_buffer_healthy(snap: &PipelineSnapshot<'_>) -> bool {
    if !snap.streaming {
        return true;
    }
    let drops = snap.stats.pipeline.iq_dropped_catchup
        + snap.stats.pipeline.raw_ring_dropped
        + snap.stats.pipeline.decim_ring_dropped;
    if drops > 0 || snap.stats.slow {
        return false;
    }
    snap.stats.last_drain > 0 || snap.stats.iq_buffer_fill > 0.05
}

fn rf_gain_subtitle(gain_db: f32) -> String {
    if gain_db.abs() < 0.05 {
        "unity · listen / spectrum / S-meter".to_string()
    } else {
        format!("{gain_db:+.1} dB · all IQ consumers")
    }
}

fn port_pos(rect: Rect, port: Port) -> Pos2 {
    let y = egui::lerp(rect.top()..=rect.bottom(), port.t.clamp(0.0, 1.0));
    match port.side {
        PortSide::Left => Pos2::new(rect.left(), y),
        PortSide::Right => Pos2::new(rect.right(), y),
    }
}

fn draw_bezier_link(
    painter: &Painter,
    from: Pos2,
    to: Pos2,
    active: bool,
    accent: bool,
    style: EdgeStyle,
) {
    let color = if !active {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 70)
    } else if style == EdgeStyle::PeakHold {
        Color32::from_rgba_unmultiplied(WARN.r(), WARN.g(), WARN.b(), 200)
    } else if accent {
        ACCENT
    } else {
        Color32::from_rgba_unmultiplied(TRACE.r(), TRACE.g(), TRACE.b(), 180)
    };
    let dist = (to - from).length();
    let bend = (dist * 0.42).clamp(36.0, 110.0);
    let c1 = from + Vec2::new(bend, 0.0);
    let c2 = to - Vec2::new(bend, 0.0);
    let stroke_width = if active { 1.75 } else { 1.25 };
    if style == EdgeStyle::PeakHold {
        draw_dashed_bezier_link(painter, from, c1, c2, to, stroke_width, color);
    } else {
        painter.add(Shape::CubicBezier(egui::epaint::CubicBezierShape {
            points: [from, c1, c2, to],
            closed: false,
            fill: Color32::TRANSPARENT,
            stroke: Stroke::new(if active { 2.0 } else { 1.25 }, color).into(),
        }));
        draw_arrowhead(painter, c2, to, color);
    }
    painter.circle_filled(from, 3.5, color);
    painter.circle_filled(to, 3.5, color);
}

fn cubic_bezier(p0: Pos2, p1: Pos2, p2: Pos2, p3: Pos2, t: f32) -> Pos2 {
    let u = 1.0 - t;
    let uu = u * u;
    let tt = t * t;
    let uuu = uu * u;
    let ttt = tt * t;
    Pos2::new(
        uuu * p0.x + 3.0 * uu * t * p1.x + 3.0 * u * tt * p2.x + ttt * p3.x,
        uuu * p0.y + 3.0 * uu * t * p1.y + 3.0 * u * tt * p2.y + ttt * p3.y,
    )
}

fn draw_dashed_bezier_link(
    painter: &Painter,
    from: Pos2,
    c1: Pos2,
    c2: Pos2,
    to: Pos2,
    width: f32,
    color: Color32,
) {
    const STEPS: usize = 28;
    let stroke = Stroke::new(width, color);
    let mut prev = from;
    for i in 1..=STEPS {
        let t = i as f32 / STEPS as f32;
        let point = cubic_bezier(from, c1, c2, to, t);
        if i % 2 == 0 {
            painter.line_segment([prev, point], stroke);
        }
        prev = point;
    }
}

fn draw_arrowhead(painter: &Painter, from: Pos2, to: Pos2, color: Color32) {
    let dir = (to - from).normalized();
    let tip = to;
    let left = tip - dir * 9.0 + Vec2::new(-dir.y, dir.x) * 4.5;
    let right = tip - dir * 9.0 + Vec2::new(dir.y, -dir.x) * 4.5;
    painter.add(Shape::convex_polygon(vec![tip, left, right], color, Stroke::NONE));
}

fn paint_node(painter: &Painter, rect: Rect, node: &NodeSpec, hovered: bool) {
    let active = node_stage_active(node);
    let (fill, border, title_color) = match node.kind {
        NodeKind::Source => (
            Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 32),
            Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 150),
            ACCENT,
        ),
        NodeKind::Sink => (
            Color32::from_rgba_unmultiplied(OK.r(), OK.g(), OK.b(), 28),
            Color32::from_rgba_unmultiplied(OK.r(), OK.g(), OK.b(), 140),
            OK,
        ),
        NodeKind::Process if active => (
            Color32::from_rgb(30, 38, 52),
            Color32::from_rgb(70, 88, 118),
            Color32::from_rgb(220, 228, 240),
        ),
        NodeKind::Process => (
            Color32::from_rgb(24, 28, 38),
            Color32::from_rgb(50, 58, 72),
            MUTED,
        ),
    };
    let border = if hovered {
        Color32::from_rgba_unmultiplied(ACCENT.r(), ACCENT.g(), ACCENT.b(), 220)
    } else {
        border
    };
    painter.rect(
        rect,
        CornerRadius::same(8),
        fill,
        Stroke::new(1.25, border),
        StrokeKind::Inside,
    );

    let port_color = if active {
        ACCENT
    } else {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 120)
    };
    painter.circle_filled(port_pos(rect, port(node.id, PortSide::Left, 0.5)), 4.0, port_color);
    painter.circle_filled(port_pos(rect, port(node.id, PortSide::Right, 0.5)), 4.0, port_color);

    painter.text(
        rect.left_top() + Vec2::new(10.0, 8.0),
        Align2::LEFT_TOP,
        &node.title,
        FontId::proportional(12.0),
        title_color,
    );
    painter.text(
        rect.left_top() + Vec2::new(10.0, 26.0),
        Align2::LEFT_TOP,
        &node.subtitle,
        FontId::monospace(10.0),
        MUTED,
    );
}

fn legend_row(ui: &mut Ui, snap: &PipelineSnapshot<'_>) {
    ui.horizontal_wrapped(|ui| {
        ui.spacing_mut().item_spacing.x = 12.0;
        legend_chip(ui, "Listen path", ACCENT);
        legend_chip(ui, "Spectrum", TRACE);
        legend_chip(ui, "Skimmer IQ", WARN);
        legend_dashed_chip(ui, "Peak hold → skimmer", WARN);
        if snap.cw.diagnostic.any_active() {
            legend_chip(ui, "Diagnostic bypass active", WARN);
        }
        if snap.stats.iq_recording {
            legend_chip(ui, "IQ recording", WARN);
        }
        if snap.stats.iq_playback {
            legend_chip(ui, "IQ playback", OK);
        }
        if snap.stats.slow {
            legend_chip(ui, "CPU slow", WARN);
        }
        if !ring_buffer_healthy(snap) && snap.streaming {
            legend_chip(ui, "Ring stress", WARN);
        }
    });
}

fn legend_dashed_chip(ui: &mut Ui, label: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(14.0, 12.0), Sense::hover());
        let painter = ui.painter_at(rect);
        let y = rect.center().y;
        let mut x = rect.left();
        let stroke = Stroke::new(2.0, color);
        while x < rect.right() {
            let x_end = (x + 4.0).min(rect.right());
            painter.line_segment([egui::pos2(x, y), egui::pos2(x_end, y)], stroke);
            x += 7.0;
        }
        ui.label(egui::RichText::new(label).small().color(MUTED));
    });
}

fn legend_chip(ui: &mut Ui, label: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 4.0;
        let (rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 12.0), Sense::hover());
        ui.painter_at(rect).circle_filled(rect.center(), 4.0, color);
        ui.label(egui::RichText::new(label).small().color(MUTED));
    });
}

fn decim_factor(manual: u32, sample_rate: f32) -> usize {
    if manual == 0 {
        hfsdr::decimation_factor(sample_rate).max(1)
    } else {
        manual as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::engine::EngineStats;
    use eframe::egui;
    use hfsdr::CwChannelSettings;

    fn sample_snapshot<'a>(
        ingress_decim: usize,
        cw: &'a CwChannelSettings,
        stats: &'a EngineStats,
    ) -> PipelineSnapshot<'a> {
        PipelineSnapshot {
            source_label: "test",
            streaming: true,
            device_rate_hz: 384_000.0,
            ingress_decim,
            cw,
            skimmer_enabled: true,
            audio_enabled: true,
            rf_gain_db: 0.0,
            stats,
        }
    }

    #[test]
    fn pipeline_stage_labels_non_empty() {
        let stages = [
            PipelineStage::NoiseBlanker,
            PipelineStage::Skimmer,
            PipelineStage::AudioOutput,
        ];
        for stage in stages {
            assert!(!stage.label().is_empty());
        }
        assert!(PipelineStage::ListenNco.is_diagnostic());
        assert!(!PipelineStage::Agc.is_diagnostic());
    }

    #[test]
    fn pipeline_layout_tracks_offsets() {
        let mut layout = PipelineLayout::default();
        assert_eq!(layout.offset(NodeId::Source), egui::Vec2::ZERO);
        layout.add_offset(NodeId::Source, egui::vec2(10.0, 5.0));
        assert_eq!(layout.offset(NodeId::Source).x, 10.0);
        layout.reset();
        assert_eq!(layout.offset(NodeId::Source), egui::Vec2::ZERO);
    }

    #[test]
    fn build_graph_includes_ingress_when_decimating() {
        let cw = CwChannelSettings::default();
        let stats = EngineStats::default();
        let snap = sample_snapshot(4, &cw, &stats);
        let graph = build_graph(&snap);
        assert!(graph.nodes.iter().any(|n| n.id == NodeId::IngressDecim));
        assert!(graph.edges.len() > 10);
    }

    #[test]
    fn build_graph_omits_ingress_at_unity() {
        let cw = CwChannelSettings::default();
        let stats = EngineStats::default();
        let snap = sample_snapshot(1, &cw, &stats);
        let graph = build_graph(&snap);
        assert!(!graph.nodes.iter().any(|n| n.id == NodeId::IngressDecim));
    }

    #[test]
    fn build_graph_ingress_has_no_bypass_toggle() {
        let cw = CwChannelSettings::default();
        let stats = EngineStats::default();
        let snap = sample_snapshot(4, &cw, &stats);
        let graph = build_graph(&snap);
        let ingress = graph
            .nodes
            .iter()
            .find(|n| n.id == NodeId::IngressDecim)
            .expect("ingress node");
        assert!(ingress.toggle.is_none());
        assert!(ingress.extra_toggle.is_none());
    }

    #[test]
    fn build_graph_wideband_uses_fused_listen_ingress() {
        let cw = CwChannelSettings::default();
        let stats = EngineStats::default();
        let snap = sample_snapshot(1, &cw, &stats);
        let graph = build_graph(&snap);
        assert!(graph.nodes.iter().any(|n| n.id == NodeId::WidebandIngress));
        assert!(!graph.nodes.iter().any(|n| n.id == NodeId::Nco));
        assert!(!graph.nodes.iter().any(|n| n.id == NodeId::Decimator));
    }

    #[test]
    fn build_graph_narrowband_keeps_nco_and_decimator() {
        let cw = CwChannelSettings::default();
        let stats = EngineStats::default();
        let snap = PipelineSnapshot {
            source_label: "test",
            streaming: true,
            device_rate_hz: 12_000.0,
            ingress_decim: 1,
            cw: &cw,
            skimmer_enabled: true,
            audio_enabled: true,
            rf_gain_db: 0.0,
            stats: &stats,
        };
        let graph = build_graph(&snap);
        assert!(!graph.nodes.iter().any(|n| n.id == NodeId::WidebandIngress));
        assert!(graph.nodes.iter().any(|n| n.id == NodeId::Nco));
        assert!(graph.nodes.iter().any(|n| n.id == NodeId::Decimator));
    }

    #[test]
    fn build_graph_links_fft_peak_hold_to_skimmer() {
        let cw = CwChannelSettings::default();
        let stats = EngineStats::default();
        let snap = sample_snapshot(1, &cw, &stats);
        let graph = build_graph(&snap);
        assert!(graph.edges.iter().any(|e| {
            e.from.node == NodeId::Fft
                && e.to.node == NodeId::Skimmer
                && e.style == EdgeStyle::PeakHold
        }));
    }

    #[test]
    fn build_graph_links_weak_signal_stages_before_agc() {
        let cw = CwChannelSettings::default();
        let stats = EngineStats::default();
        let snap = PipelineSnapshot {
            source_label: "test",
            streaming: true,
            device_rate_hz: 12_000.0,
            ingress_decim: 1,
            cw: &cw,
            skimmer_enabled: false,
            audio_enabled: true,
            rf_gain_db: 0.0,
            stats: &stats,
        };
        let graph = build_graph(&snap);
        assert!(graph.nodes.iter().any(|n| n.id == NodeId::IqApf));
        assert!(graph.nodes.iter().any(|n| n.id == NodeId::IqWiener));
        assert!(graph.nodes.iter().any(|n| n.id == NodeId::Squelch));
        assert!(graph.edges.iter().any(|e| {
            e.from.node == NodeId::ChannelFir && e.to.node == NodeId::IqApf
        }));
        assert!(graph.edges.iter().any(|e| {
            e.from.node == NodeId::IqWiener && e.to.node == NodeId::Agc
        }));
    }

    #[test]
    fn build_graph_links_source_through_ring_and_gain() {
        let cw = CwChannelSettings::default();
        let stats = EngineStats::default();
        let snap = PipelineSnapshot {
            source_label: "test",
            streaming: true,
            device_rate_hz: 12_000.0,
            ingress_decim: 1,
            cw: &cw,
            skimmer_enabled: true,
            audio_enabled: true,
            rf_gain_db: 6.0,
            stats: &stats,
        };
        let graph = build_graph(&snap);
        assert!(graph.nodes.iter().any(|n| n.id == NodeId::RingBuffer));
        assert!(graph.nodes.iter().any(|n| n.id == NodeId::RfGain));
        assert!(graph.edges.iter().any(|e| {
            e.from.node == NodeId::Source && e.to.node == NodeId::RingBuffer
        }));
        assert!(graph.edges.iter().any(|e| {
            e.from.node == NodeId::RingBuffer && e.to.node == NodeId::RfGain
        }));
        assert!(graph.edges.iter().any(|e| {
            e.from.node == NodeId::RfGain && e.to.node == NodeId::NoiseBlanker
        }));
    }

    #[test]
    fn ring_buffer_subtitle_reports_dual_ring() {
        let cw = CwChannelSettings::default();
        let mut stats = EngineStats::default();
        stats.pipeline.dual_ring = true;
        stats.iq_buffer_fill = 0.42;
        stats.iq_buffer_secs = 0.8;
        let snap = sample_snapshot(4, &cw, &stats);
        let sub = ring_buffer_subtitle(&snap);
        assert!(sub.contains("dual ring"));
        assert!(sub.contains("42%"));
    }

    #[test]
    fn decim_factor_auto_uses_library_heuristic() {
        assert!(decim_factor(0, 384_000.0) > 1);
        assert_eq!(decim_factor(8, 384_000.0), 8);
        assert_eq!(decim_factor(0, 12_000.0), 1);
    }

    #[test]
    fn all_pipeline_stages_have_labels() {
        use PipelineStage::*;
        for stage in [
            NoiseBlanker,
            ManualNotches,
            ListenNco,
            DecimatorFir,
            ChannelFir,
            IqApf,
            IqWiener,
            Agc,
            Bfo,
            Sidetone,
            Apf,
            AutoNotch,
            NoiseReduction,
            Squelch,
            Skimmer,
            AudioOutput,
        ] {
            assert!(!stage.label().is_empty());
        }
    }

    #[test]
    fn diagnostic_stages_flagged() {
        assert!(PipelineStage::Bfo.is_diagnostic());
        assert!(!PipelineStage::Skimmer.is_diagnostic());
        assert!(!PipelineStage::Agc.is_diagnostic());
    }
}
