//! Interactive receive-pipeline flow diagram (node graph with Bézier links).

use std::collections::HashMap;

use eframe::egui::{
    self, Align2, Color32, CornerRadius, FontId, Painter, Pos2, Rect, Sense, Shape, Stroke,
    StrokeKind, Ui, Vec2,
};
use hfsdr::CwChannelSettings;

use crate::engine::EngineStats;
use crate::theme::{chip_hovered, ACCENT, MUTED, OK, TRACE, WARN};

const NODE_W: f32 = 128.0;
const NODE_H: f32 = 44.0;
const CANVAS_W: f32 = 820.0;
const CANVAS_H: f32 = 560.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum NodeId {
    Source,
    IngressDecim,
    NoiseBlanker,
    Nco,
    Decimator,
    Notches,
    ChannelFir,
    Agc,
    Bfo,
    Apf,
    AutoNotch,
    NoiseReduction,
    AudioOut,
    SpectrumFront,
    Fft,
    Waterfall,
    Skimmer,
    Spots,
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

#[derive(Clone, Copy, Debug)]
struct Edge {
    from: Port,
    to: Port,
    accent: bool,
    active: bool,
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

    pub fn show(&mut self, ui: &mut Ui, snap: &PipelineSnapshot<'_>) {
        ui.horizontal(|ui| {
            ui.label(
                egui::RichText::new("Drag blocks to rearrange · links follow ports")
                    .small()
                    .color(MUTED),
            );
            if ui.small_button("Reset layout").clicked() {
                self.layout.reset();
            }
        });
        ui.add_space(4.0);

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
            draw_bezier_link(&painter, from, to, edge.active, edge.accent);
        }

        let mut drag_delta = Vec2::ZERO;
        let mut dragged: Option<NodeId> = None;
        for node in &graph.nodes {
            let Some(node_rect) = rects.get(&node.id).copied() else {
                continue;
            };
            let id = ui.id().with(("pipeline_node", node.id as u8));
            let node_resp = ui.interact(node_rect, id, Sense::click_and_drag());
            if node_resp.dragged() {
                drag_delta = node_resp.drag_delta();
                dragged = Some(node.id);
            }
            let node_hovered = chip_hovered(ui, node_rect, &node_resp);
            paint_node(&painter, node_rect, &node, node_hovered);
        }

        if let Some(id) = dragged {
            self.layout.add_offset(id, drag_delta);
        }

        if response.hovered() && dragged.is_none() {
            ui.ctx().set_cursor_icon(egui::CursorIcon::Default);
        }

        ui.add_space(6.0);
        legend_row(ui, snap);
    }
}

struct NodeSpec {
    id: NodeId,
    title: &'static str,
    subtitle: String,
    enabled: bool,
    kind: NodeKind,
    size: Vec2,
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
        node(
            NodeId::NoiseBlanker,
            "Noise blanker",
            "IQ impulse blanker".to_string(),
            snap.cw.noise_blanker.enabled,
            NodeKind::Process,
        ),
        node(
            NodeId::Nco,
            "NCO shift",
            format!("RIT {:.0} Hz", snap.cw.listen_offset_hz.hz()),
            true,
            NodeKind::Process,
        ),
        node(
            NodeId::Decimator,
            "Decimator",
            format!(
                "→ {:.1} kS/s",
                snap.device_rate_hz
                    / decim_factor(snap.cw.decimation, snap.device_rate_hz) as f32
                    / 1000.0
            ),
            true,
            NodeKind::Process,
        ),
        node(
            NodeId::Notches,
            "IQ notches",
            if any_notch {
                format!("{notch_count} active")
            } else {
                "manual · off".to_string()
            },
            any_notch,
            NodeKind::Process,
        ),
        node(
            NodeId::ChannelFir,
            "Channel FIR",
            format!("BW {:.0} Hz", snap.cw.passband_hz),
            true,
            NodeKind::Process,
        ),
        node(
            NodeId::Agc,
            "AGC",
            if snap.cw.agc.enabled {
                "auto gain".to_string()
            } else {
                format!("manual {:.1}×", snap.cw.agc.manual_gain)
            },
            snap.cw.agc.enabled,
            NodeKind::Process,
        ),
        node(
            NodeId::Bfo,
            "BFO detector",
            format!("tone {:.0} Hz", snap.cw.bfo_hz),
            true,
            NodeKind::Process,
        ),
        node(
            NodeId::Apf,
            "Audio peak filter",
            "resonant boost".to_string(),
            snap.cw.apf.enabled,
            NodeKind::Process,
        ),
        node(
            NodeId::AutoNotch,
            "Auto-notch",
            format!("guard ±{:.0} Hz", snap.cw.auto_notch.guard_hz),
            snap.cw.auto_notch.enabled,
            NodeKind::Process,
        ),
        node(
            NodeId::NoiseReduction,
            "Noise reduction",
            "post-demod LMS".to_string(),
            snap.cw.noise_reduction.enabled,
            NodeKind::Process,
        ),
        node(
            NodeId::AudioOut,
            "Audio sink",
            snap.stats
                .audio_device
                .clone()
                .unwrap_or_else(|| "speakers".to_string()),
            snap.audio_enabled,
            NodeKind::Sink,
        ),
        node(
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
        node(
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
        node(
            NodeId::Waterfall,
            "Waterfall sink",
            format!("{} rows/pump", snap.stats.spectrum_rows_per_pump),
            true,
            NodeKind::Sink,
        ),
        node(
            NodeId::Skimmer,
            "Skimmer",
            format!("{} decoders", snap.stats.skimmer_channels),
            snap.skimmer_enabled,
            NodeKind::Process,
        ),
        node(
            NodeId::Spots,
            "Spots sink",
            "callsign table".to_string(),
            snap.skimmer_enabled,
            NodeKind::Sink,
        ),
    ];

    if ingress_on {
        nodes.push(node(
            NodeId::IngressDecim,
            "Ingress FIR",
            format!("÷{} → {:.0} kS/s", snap.ingress_decim, snap.stats.sample_rate / 1000.0),
            true,
            NodeKind::Process,
        ));
    }

    let listen_chain = [
        NodeId::NoiseBlanker,
        NodeId::Nco,
        NodeId::Decimator,
        NodeId::Notches,
        NodeId::ChannelFir,
        NodeId::Agc,
        NodeId::Bfo,
        NodeId::Apf,
        NodeId::AutoNotch,
        NodeId::NoiseReduction,
        NodeId::AudioOut,
    ];
    let spectrum_chain = [NodeId::SpectrumFront, NodeId::Fft, NodeId::Waterfall];
    let skimmer_chain = [NodeId::Skimmer, NodeId::Spots];

    let mut edges = Vec::new();

    if ingress_on {
        edges.push(edge(
            port(NodeId::Source, PortSide::Right, 0.35),
            port(NodeId::IngressDecim, PortSide::Left, 0.5),
            true,
            snap.streaming,
        ));
        edges.push(edge(
            port(NodeId::Source, PortSide::Right, 0.65),
            port(listen_chain[0], PortSide::Left, 0.5),
            true,
            snap.streaming,
        ));
        let ingress_out = port(NodeId::IngressDecim, PortSide::Right, 0.5);
        edges.push(edge(
            port(NodeId::IngressDecim, PortSide::Right, 0.35),
            port(spectrum_chain[0], PortSide::Left, 0.5),
            false,
            snap.streaming,
        ));
        edges.push(edge(
            port(NodeId::IngressDecim, PortSide::Right, 0.65),
            port(skimmer_chain[0], PortSide::Left, 0.5),
            false,
            snap.streaming,
        ));
        let _ = ingress_out;
    } else {
        edges.push(edge(
            port(NodeId::Source, PortSide::Right, 0.25),
            port(listen_chain[0], PortSide::Left, 0.5),
            true,
            snap.streaming,
        ));
        edges.push(edge(
            port(NodeId::Source, PortSide::Right, 0.5),
            port(spectrum_chain[0], PortSide::Left, 0.5),
            false,
            snap.streaming,
        ));
        edges.push(edge(
            port(NodeId::Source, PortSide::Right, 0.75),
            port(skimmer_chain[0], PortSide::Left, 0.5),
            false,
            snap.streaming && snap.skimmer_enabled,
        ));
    }

    chain_edges(&mut edges, &listen_chain, true);
    chain_edges(&mut edges, &spectrum_chain, false);
    chain_edges(&mut edges, &skimmer_chain, false);

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
        ));
    }
}

fn node(id: NodeId, title: &'static str, subtitle: String, enabled: bool, kind: NodeKind) -> NodeSpec {
    NodeSpec {
        id,
        title,
        subtitle,
        enabled,
        kind,
        size: Vec2::new(NODE_W, NODE_H),
    }
}

fn edge(from: Port, to: Port, accent: bool, active: bool) -> Edge {
    Edge {
        from,
        to,
        accent,
        active,
    }
}

fn port(node: NodeId, side: PortSide, t: f32) -> Port {
    Port { node, side, t }
}

fn default_pos(id: NodeId) -> Pos2 {
    match id {
        NodeId::Source => Pos2::new(346.0, 24.0),
        NodeId::IngressDecim => Pos2::new(346.0, 96.0),
        NodeId::NoiseBlanker => Pos2::new(24.0, 168.0),
        NodeId::Nco => Pos2::new(24.0, 228.0),
        NodeId::Decimator => Pos2::new(24.0, 288.0),
        NodeId::Notches => Pos2::new(24.0, 348.0),
        NodeId::ChannelFir => Pos2::new(24.0, 408.0),
        NodeId::Agc => Pos2::new(24.0, 468.0),
        NodeId::Bfo => Pos2::new(168.0, 168.0),
        NodeId::Apf => Pos2::new(168.0, 228.0),
        NodeId::AutoNotch => Pos2::new(168.0, 288.0),
        NodeId::NoiseReduction => Pos2::new(168.0, 348.0),
        NodeId::AudioOut => Pos2::new(168.0, 420.0),
        NodeId::SpectrumFront => Pos2::new(346.0, 168.0),
        NodeId::Fft => Pos2::new(346.0, 248.0),
        NodeId::Waterfall => Pos2::new(346.0, 328.0),
        NodeId::Skimmer => Pos2::new(668.0, 168.0),
        NodeId::Spots => Pos2::new(668.0, 248.0),
    }
}

fn port_pos(rect: Rect, port: Port) -> Pos2 {
    let y = egui::lerp(rect.top()..=rect.bottom(), port.t.clamp(0.0, 1.0));
    match port.side {
        PortSide::Left => Pos2::new(rect.left(), y),
        PortSide::Right => Pos2::new(rect.right(), y),
    }
}

fn draw_bezier_link(painter: &Painter, from: Pos2, to: Pos2, active: bool, accent: bool) {
    let color = if !active {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 70)
    } else if accent {
        ACCENT
    } else {
        Color32::from_rgba_unmultiplied(TRACE.r(), TRACE.g(), TRACE.b(), 180)
    };
    let dist = (to - from).length();
    let bend = (dist * 0.42).clamp(36.0, 110.0);
    let c1 = from + Vec2::new(bend, 0.0);
    let c2 = to - Vec2::new(bend, 0.0);
    painter.add(Shape::CubicBezier(egui::epaint::CubicBezierShape {
        points: [from, c1, c2, to],
        closed: false,
        fill: Color32::TRANSPARENT,
        stroke: Stroke::new(if active { 2.0 } else { 1.25 }, color).into(),
    }));
    draw_arrowhead(painter, c2, to, color);
    painter.circle_filled(from, 3.5, color);
    painter.circle_filled(to, 3.5, color);
}

fn draw_arrowhead(painter: &Painter, from: Pos2, to: Pos2, color: Color32) {
    let dir = (to - from).normalized();
    let tip = to;
    let left = tip - dir * 9.0 + Vec2::new(-dir.y, dir.x) * 4.5;
    let right = tip - dir * 9.0 + Vec2::new(dir.y, -dir.x) * 4.5;
    painter.add(Shape::convex_polygon(vec![tip, left, right], color, Stroke::NONE));
}

fn paint_node(painter: &Painter, rect: Rect, node: &NodeSpec, hovered: bool) {
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
        NodeKind::Process if node.enabled => (
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

    let port_color = if node.enabled {
        ACCENT
    } else {
        Color32::from_rgba_unmultiplied(MUTED.r(), MUTED.g(), MUTED.b(), 120)
    };
    painter.circle_filled(port_pos(rect, port(node.id, PortSide::Left, 0.5)), 4.0, port_color);
    painter.circle_filled(port_pos(rect, port(node.id, PortSide::Right, 0.5)), 4.0, port_color);

    painter.text(
        rect.left_top() + Vec2::new(10.0, 8.0),
        Align2::LEFT_TOP,
        node.title,
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
        legend_chip(ui, "Skimmer", WARN);
        if snap.stats.iq_recording {
            legend_chip(ui, "IQ recording", WARN);
        }
        if snap.stats.iq_playback {
            legend_chip(ui, "IQ playback", OK);
        }
        if snap.stats.slow {
            legend_chip(ui, "CPU slow", WARN);
        }
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
