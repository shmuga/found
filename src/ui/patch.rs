//! Patch view: the edit graph rendered with egui-snarl, styled as a patcher.

use super::theme;
use crate::graph::{self, Graph, NodeData};
use crate::ops::{Category, Op};
use egui::{Color32, Pos2, Stroke, Ui};
use egui_snarl::ui::{PinInfo, SnarlPin, SnarlStyle, SnarlViewer, WireStyle};
use egui_snarl::{InPin, NodeId, OutPin, Snarl};

pub struct PatchViewer<'a> {
    pub selected: &'a mut Option<NodeId>,
    pub roll: &'a mut u32,
}

fn pin_color(op: &Op) -> Color32 {
    match op.category() {
        Category::Io => theme::ACCENT,
        Category::Basic => theme::GREEN,
        Category::Experimental => theme::EXPERIMENTAL,
    }
}

pub fn style() -> SnarlStyle {
    let mut s = SnarlStyle::new();
    s.wire_style = Some(WireStyle::AxisAligned {
        corner_radius: 12.0,
    });
    s.wire_width = Some(2.5);
    s.pin_size = Some(9.0);
    s.collapsible = Some(true);
    s.bg_frame = Some(
        egui::Frame::new()
            .fill(theme::CANVAS)
            .inner_margin(egui::Margin::ZERO),
    );
    s.node_frame = Some(
        egui::Frame::new()
            .fill(Color32::from_rgb(196, 199, 204))
            .stroke(Stroke::new(1.0, Color32::from_rgb(110, 112, 116)))
            .corner_radius(egui::CornerRadius::same(6))
            .inner_margin(egui::Margin::same(6)),
    );
    s.header_frame = Some(
        egui::Frame::new()
            .fill(theme::BOX)
            .corner_radius(egui::CornerRadius::same(4))
            .inner_margin(egui::Margin::symmetric(6, 3)),
    );
    s
}

pub fn add_menu(ui: &mut Ui, mut on_pick: impl FnMut(Op)) {
    ui.menu_button("Basic", |ui| {
        theme::popup(ui);
        for op in Op::catalog()
            .into_iter()
            .filter(|o| o.category() == Category::Basic)
        {
            if ui.button(op.name()).clicked() {
                on_pick(op);
                ui.close();
            }
        }
    });
    ui.menu_button("Experimental", |ui| {
        theme::popup(ui);
        for op in Op::catalog()
            .into_iter()
            .filter(|o| o.category() == Category::Experimental)
        {
            if ui.button(op.name()).clicked() {
                on_pick(op);
                ui.close();
            }
        }
    });
}

impl SnarlViewer<NodeData> for PatchViewer<'_> {
    fn title(&mut self, node: &NodeData) -> String {
        node.op.name().to_string()
    }

    fn show_header(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeData>,
    ) {
        let n = &snarl[node];
        let mut text = egui::RichText::new(n.op.name()).color(theme::BOX_TEXT);
        if n.bypass {
            text = text.strikethrough().color(Color32::from_gray(170));
        }
        if *self.selected == Some(node) {
            text = text.color(theme::ACCENT);
        }
        if ui
            .add(egui::Label::new(text).sense(egui::Sense::click()))
            .clicked()
        {
            *self.selected = Some(node);
        }
    }

    fn inputs(&mut self, node: &NodeData) -> usize {
        node.op.inputs()
    }

    fn outputs(&mut self, node: &NodeData) -> usize {
        node.op.outputs()
    }

    fn show_input(
        &mut self,
        pin: &InPin,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeData>,
    ) -> impl SnarlPin + 'static {
        let n = &snarl[pin.id.node];
        let label = n.op.input_label(pin.id.input);
        ui.label(egui::RichText::new(label).small().color(theme::TEXT_DIM));
        PinInfo::circle().with_fill(pin_color(&n.op))
    }

    fn show_output(
        &mut self,
        pin: &OutPin,
        ui: &mut Ui,
        snarl: &mut Snarl<NodeData>,
    ) -> impl SnarlPin + 'static {
        let n = &snarl[pin.id.node];
        let label = n.op.output_label(pin.id.output);
        if !label.is_empty() {
            ui.label(egui::RichText::new(label).small().color(theme::TEXT_DIM));
        }
        PinInfo::circle().with_fill(pin_color(&n.op))
    }

    fn has_body(&mut self, node: &NodeData) -> bool {
        !matches!(node.op, Op::Scan | Op::Output)
    }

    fn show_body(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeData>,
    ) {
        ui.vertical(|ui| {
            ui.set_max_width(230.0);
            let n = &mut snarl[node];
            if let Op::Crop(c) = &n.op {
                ui.label(format!(
                    "{} · rot {}° {:+.1}°",
                    c.aspect.label(),
                    c.rot90 as u32 * 90,
                    c.angle
                ));
                ui.small("edit framing in presentation (C)");
            }
            ui.spacing_mut().slider_width = 110.0;
            super::params::op_params(ui, &mut n.op, ("patch", node.0), None, false);
        });
    }

    fn has_graph_menu(&mut self, _pos: Pos2, _snarl: &mut Snarl<NodeData>) -> bool {
        true
    }

    fn show_graph_menu(&mut self, pos: Pos2, ui: &mut Ui, snarl: &mut Snarl<NodeData>) {
        theme::popup(ui);
        ui.label("Add node");
        let mut picked = None;
        add_menu(ui, |op| picked = Some(op));
        if let Some(op) = picked {
            let id = snarl.insert_node(pos, NodeData::new(op));
            *self.selected = Some(id);
        }
    }

    fn has_node_menu(&mut self, _node: &NodeData) -> bool {
        true
    }

    fn show_node_menu(
        &mut self,
        node: NodeId,
        _inputs: &[InPin],
        _outputs: &[OutPin],
        ui: &mut Ui,
        snarl: &mut Snarl<NodeData>,
    ) {
        theme::popup(ui);
        let io = matches!(snarl[node].op, Op::Scan | Op::Output);
        if !io {
            let label = if snarl[node].bypass {
                "Enable"
            } else {
                "Bypass"
            };
            if ui.button(label).clicked() {
                snarl[node].bypass = !snarl[node].bypass;
                ui.close();
            }
            if snarl[node].op.category() == Category::Experimental
                && ui.button("Randomize").clicked()
            {
                *self.roll = self.roll.wrapping_add(1);
                snarl[node].op.randomize(*self.roll);
                ui.close();
            }
            if ui.button("Duplicate").clicked() {
                let n = snarl.get_node_info(node).cloned();
                if let Some(n) = n {
                    snarl.insert_node(n.pos + egui::vec2(30.0, 60.0), n.value);
                }
                ui.close();
            }
            if ui.button("Remove (bridge)").clicked() {
                graph::remove_bridging(snarl, node);
                *self.selected = None;
                ui.close();
            }
            if ui.button("Delete").clicked() {
                snarl.remove_node(node);
                *self.selected = None;
                ui.close();
            }
        } else {
            ui.label("Scan / Output are fixed");
        }
    }

    fn connect(&mut self, from: &OutPin, to: &InPin, snarl: &mut Snarl<NodeData>) {
        if from.id.node == to.id.node || graph::would_cycle(snarl, from.id.node, to.id.node) {
            return;
        }
        // One wire per input (an image inlet takes a single source).
        for r in &to.remotes {
            snarl.disconnect(*r, to.id);
        }
        snarl.connect(from.id, to.id);
    }
}

pub fn show(ui: &mut Ui, g: &mut Graph, selected: &mut Option<NodeId>, roll: &mut u32) {
    let mut viewer = PatchViewer { selected, roll };
    egui_snarl::ui::SnarlWidget::new()
        .id_salt("patch")
        .style(style())
        .show(g, &mut viewer, ui);
}
