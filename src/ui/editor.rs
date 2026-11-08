//! Edit view: presentation mode (preview + node chain with sliders) and patch mode (graph).

use super::{params, patch, theme};
use crate::app::{CropDrag, Editor, FoundApp, Msg, Tool, gen_next};
use crate::graph;
use crate::library::ItemRef;
use crate::ops::{Aspect, Category, Op, basic};
use crate::presets::{self, Preset, PresetKind};
use egui::{Color32, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use egui_snarl::NodeId;
use std::sync::Arc;
use std::sync::atomic::Ordering;

/// Height of the bottom tool strips (editor, collage).
pub const BAR_H: f32 = 34.0;

pub enum EdAction {
    AutoWb(NodeId, bool),
    PickWb(NodeId, f32, f32),
    ApplyPreset(usize),
    SavePreset(String),
    DeletePreset(String),
    FileInto(String),
    SaveCopy(String),
    Save,
    Reveal,
}

impl FoundApp {
    /// Editor tools in the bottom bar. Left group: view, crop, before, history, apply last.
    /// Right group (`right`): unsaved, save, trash, zoom.
    pub fn edit_tools(&mut self, ui: &mut Ui, ctx: &egui::Context, right: bool) {
        use theme::{Icon, tool};
        let last = self.settings.last_recipe.as_ref().map(|l| l.name.clone());
        let dirty = self.editor_dirty();
        let Some(ed) = &mut self.editor else { return };
        let zoom = ed.zoom.percent();
        let (mut undo, mut redo, mut apply_last, mut save, mut trash, mut fit) =
            (false, false, false, false, false, false);
        if !right {
            if tool(
                ui,
                Icon::Sliders,
                "Presentation",
                "Presentation: sliders (Tab toggles)",
                Some(!ed.patch),
                true,
            )
            .clicked()
            {
                ed.patch = false;
            }
            if tool(
                ui,
                Icon::Patch,
                "Patch",
                "Patch: node graph (Tab toggles)",
                Some(ed.patch),
                true,
            )
            .clicked()
            {
                ed.patch = true;
            }
            ui.add_space(8.0);
            if tool(
                ui,
                Icon::Crop,
                "Crop",
                "Crop (c)",
                Some(ed.tool == Tool::Crop),
                true,
            )
            .clicked()
            {
                ed.tool = if ed.tool == Tool::Crop {
                    Tool::None
                } else {
                    Tool::Crop
                };
                ed.patch = false;
            }
            if tool(
                ui,
                Icon::Compare,
                "Before",
                "Before / after (\\)",
                Some(ed.compare),
                true,
            )
            .clicked()
            {
                ed.compare = !ed.compare;
            }
            ui.add_space(8.0);
            undo = tool(
                ui,
                Icon::Undo,
                "Undo",
                "Undo (u · ⌘Z)",
                None,
                ed.history.can_undo(),
            )
            .clicked();
            redo = tool(
                ui,
                Icon::Redo,
                "Redo",
                "Redo (Ctrl-r · ⇧⌘Z)",
                None,
                ed.history.can_redo(),
            )
            .clicked();
            ui.add_space(8.0);
            let tip = match &last {
                Some(n) => {
                    format!("Apply last: {n} (A · ⌘L) — keeps this photo's crop and white balance")
                }
                None => "Apply last — apply a preset or save a photo first (A · ⌘L)".into(),
            };
            apply_last = tool(ui, Icon::Bolt, "Apply last", &tip, None, last.is_some()).clicked();
        } else {
            // right_to_left: first added is rightmost.
            fit = tool(
                ui,
                Icon::Fit,
                "Fit",
                "Fit (= · ⌘0); double-click the photo for 100%",
                None,
                true,
            )
            .clicked();
            ui.label(
                RichText::new(format!("{zoom:.0}%"))
                    .monospace()
                    .color(theme::TEXT_DIM),
            );
            ui.add_space(6.0);
            trash = tool(
                ui,
                Icon::Trash,
                "Move to trash",
                "Move this photo to Trash and open the next (⌘Backspace)",
                None,
                true,
            )
            .clicked();
            save = tool(
                ui,
                Icon::Save,
                "Save",
                "Save (⌘S · :w) — filed photos also re-render",
                None,
                dirty,
            )
            .clicked();
            if dirty {
                theme::badge(ui, "unsaved", theme::ORANGE);
            }
        }
        if undo {
            self.undo();
        }
        if redo {
            self.redo();
        }
        if apply_last {
            self.apply_last();
        }
        if save {
            self.save_current(ctx);
        }
        if fit {
            self.act(ctx, crate::keys::Act::ZoomFit);
        }
        if trash {
            self.run_command(ctx, "trash");
        }
    }

    pub fn request_preview(&mut self, ctx: &egui::Context) {
        let Some(ed) = &mut self.editor else { return };
        let Some(proxy) = ed.proxy.clone() else {
            return;
        };
        let crop_mode = ed.tool == Tool::Crop;
        let crop_id = graph::crop_node(&ed.doc.graph).filter(|_| crop_mode);
        // In crop mode the rect itself isn't rendered (it's an overlay), so ignore it in the key.
        let mut keyed = ed.doc.graph.clone();
        if let Some(c) = crop_id
            && let Op::Crop(cp) = &mut keyed[c].op
        {
            cp.rect = [0.0, 0.0, 1.0, 1.0];
            cp.aspect = Aspect::Free;
        }
        let want = format!(
            "{}|{}|{}",
            graph::graph_json(&keyed),
            crop_id.is_some(),
            ed.compare
        );
        if want == ed.requested {
            return;
        }
        ed.requested = want;
        let generation = gen_next(&ed.latest_gen);
        let g = ed.doc.graph.clone();
        let ev = ed.evaluator.clone();
        let latest = ed.latest_gen.clone();
        let key = ed.proxy_key;
        let compare = ed.compare;
        let max_side = ctx.input(|i| i.max_texture_side).max(512);
        self.worker.spawn(ctx, move || {
            let none = Msg::Preview {
                generation,
                img: None,
                ms: 0.0,
                crop: false,
            };
            if latest.load(Ordering::SeqCst) != generation {
                return none;
            }
            let mut ev = ev.lock().unwrap();
            if latest.load(Ordering::SeqCst) != generation {
                return none;
            }
            let t = std::time::Instant::now();
            ev.prune(&g);
            let out: Option<Arc<crate::img::Img>> = if compare {
                Some(proxy.clone())
            } else if let Some(c) = crop_id {
                // Show the whole pipeline with the crop rect relaxed, so you frame the final look.
                let mut relaxed = g.clone();
                if let Op::Crop(cp) = &mut relaxed[c].op {
                    cp.rect = [0.0, 0.0, 1.0, 1.0];
                }
                ev.eval_output(&relaxed, &proxy, key)
            } else {
                ev.eval_output(&g, &proxy, key)
            };
            let ms = t.elapsed().as_secs_f32() * 1000.0;
            // Size reported is the processed size (crop math needs it); the texture may be
            // downscaled to fit the GPU's limit.
            let img = out.map(|o| {
                let ci = if o.long_edge() > max_side {
                    o.fit(max_side).to_color_image()
                } else {
                    o.to_color_image()
                };
                (ci, [o.w, o.h])
            });
            Msg::Preview {
                generation,
                img,
                ms,
                crop: crop_id.is_some(),
            }
        });
    }

    pub fn wb_auto(&mut self, ctx: &egui::Context, node: NodeId, white_patch: bool) {
        self.wb_job(ctx, node, None, white_patch);
    }

    fn wb_job(
        &mut self,
        ctx: &egui::Context,
        node: NodeId,
        pick: Option<(f32, f32)>,
        white_patch: bool,
    ) {
        let Some(ed) = &self.editor else { return };
        let Some(proxy) = ed.proxy.clone() else {
            return;
        };
        let g = ed.doc.graph.clone();
        let ev = ed.evaluator.clone();
        let key = ed.proxy_key;
        self.worker.spawn(ctx, move || {
            let inp = ev
                .lock()
                .unwrap()
                .eval_input(&g, &proxy, key, node)
                .unwrap_or(proxy);
            let c = match pick {
                Some((u, v)) => inp.mean_around(u, v, (inp.long_edge() / 300).max(2)),
                None => basic::wb_estimate(&inp, white_patch),
            };
            let (temp, tint) = basic::wb_solve(c);
            Msg::Wb { node, temp, tint }
        });
    }

    pub fn edit_view(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.editor.is_none() {
            self.view = crate::app::View::Library;
            return;
        }
        self.request_preview(&ctx);
        let presets = self.presets.clone();
        let collections = self.collections.clone();
        let mut actions: Vec<EdAction> = vec![];

        let patch_mode = self.editor.as_ref().is_some_and(|e| e.patch);
        egui::Panel::right("edit_panel")
            .resizable(true)
            .default_size(340.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ui, |ui| {
                let ed = self.editor.as_mut().unwrap();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    side_panel(ui, ed, &presets, &collections, &mut actions);
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::CANVAS))
            .show(ui, |ui| {
                let ed = self.editor.as_mut().unwrap();
                if patch_mode {
                    patch_center(ui, ed);
                } else {
                    preview_area(ui, ed, &mut actions);
                }
            });

        for a in actions {
            match a {
                EdAction::AutoWb(node, white) => self.wb_job(&ctx, node, None, white),
                EdAction::PickWb(node, u, v) => self.wb_job(&ctx, node, Some((u, v)), false),
                EdAction::ApplyPreset(i) => {
                    if let (Some(ed), Some(p)) = (&mut self.editor, self.presets.get(i).cloned()) {
                        ed.doc.graph = presets::apply(&ed.doc.graph, &p.graph);
                        ed.selected = None;
                        self.status = format!("Applied preset {}", p.name);
                        self.remember_recipe(p.name.clone(), p.graph.clone());
                    }
                }
                EdAction::SavePreset(name) => {
                    if let (Some(lib), Some(ed)) = (&self.lib, &self.editor) {
                        match presets::save_user(&lib.presets_dir(), &name, &ed.doc.graph) {
                            Ok(_) => self.status = format!("Saved preset {name}"),
                            Err(e) => self.status = format!("Couldn't save preset: {e:#}"),
                        }
                    }
                    self.refresh();
                }
                EdAction::DeletePreset(name) => {
                    if let Some(lib) = &self.lib {
                        let _ = presets::delete_user(&lib.presets_dir(), &name);
                    }
                    self.refresh();
                }
                EdAction::FileInto(c) => {
                    let advance = true;
                    self.file_current(&ctx, &c);
                    if advance && let Some(next) = self.next_inbox_item() {
                        self.open_item(&ctx, next);
                    }
                }
                EdAction::Save => self.save_current(&ctx),
                EdAction::SaveCopy(c) => self.save_copy(&ctx, &c),
                EdAction::Reveal => {
                    if let Some(ed) = &self.editor {
                        let _ = std::process::Command::new("open")
                            .arg("-R")
                            .arg(&ed.item.orig)
                            .spawn();
                    }
                }
            }
        }
    }
}

// ------------------------------------------------------------------ side panel

fn side_panel(
    ui: &mut Ui,
    ed: &mut Editor,
    presets: &[Preset],
    collections: &[String],
    actions: &mut Vec<EdAction>,
) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(ed.item.title()).heading());
        if ed.exporting {
            ui.spinner();
        }
    });
    match &ed.item.r {
        ItemRef::Inbox(_) | ItemRef::Trashed { .. } => ui.label(
            RichText::new("Inbox · not filed yet")
                .small()
                .color(theme::TEXT_DIM),
        ),
        ItemRef::Filed { .. } if ed.stale => {
            theme::badge(ui, "edit.jpg out of date · ⌘S renders", theme::ORANGE)
        }
        ItemRef::Filed { .. } => ui.label(
            RichText::new("edit.jpg up to date")
                .small()
                .color(theme::TEXT_DIM),
        ),
    };
    ui.add_space(6.0);

    if ed.patch {
        // Live mini preview next to the patch.
        if let Some(t) = &ed.preview {
            let w = ui.available_width();
            let s = t.size_vec2();
            let h = (w * s.y / s.x).min(260.0);
            ui.add(egui::Image::new((t.id(), vec2(h * s.x / s.y, h))));
        }
        ui.add_space(6.0);
        match ed
            .selected
            .filter(|id| ed.doc.graph.get_node(*id).is_some())
        {
            Some(id) => node_card(ui, ed, id, false, actions),
            None => {
                ui.label(RichText::new("Click a node title to edit it here. Right-click the canvas to add nodes, right-click a node for bypass / randomize / remove.").small().color(theme::TEXT_DIM));
            }
        }
    } else {
        let chain = graph::chain(&ed.doc.graph);
        if chain.len() < 2 {
            ui.colored_label(
                theme::ORANGE,
                "Output isn't connected to Scan — open Patch to rewire.",
            );
        }
        let cards: Vec<NodeId> = chain
            .iter()
            .copied()
            .filter(|id| !matches!(ed.doc.graph[*id].op, Op::Scan | Op::Output))
            .collect();
        for id in cards {
            node_card(ui, ed, id, true, actions);
        }
        ui.horizontal(|ui| {
            ui.menu_button("+ Add node", |ui| {
                theme::popup(ui);
                let mut picked = None;
                patch::add_menu(ui, |op| picked = Some(op));
                if let Some(op) = picked {
                    let chain = graph::chain(&ed.doc.graph);
                    let after = if chain.len() >= 2 {
                        Some(chain[chain.len() - 2])
                    } else {
                        chain.first().copied()
                    };
                    if let Some(after) = after {
                        let id = graph::insert_after(&mut ed.doc.graph, after, op);
                        ed.selected = Some(id);
                    }
                }
            });
            if graph::has_experimental(&ed.doc.graph)
                && ui
                    .button("🎲 Mutate")
                    .on_hover_text("Randomize every experimental node")
                    .clicked()
            {
                let ids: Vec<NodeId> = ed.doc.graph.node_ids().map(|(i, _)| i).collect();
                for id in ids {
                    if ed.doc.graph[id].op.category() == Category::Experimental {
                        ed.roll = ed.roll.wrapping_add(1);
                        let r = ed.roll;
                        ed.doc.graph[id].op.randomize(r);
                    }
                }
            }
        });
        let off = graph::off_chain(&ed.doc.graph);
        if !off.is_empty() {
            ui.add_space(6.0);
            ui.label(
                RichText::new("BRANCHES (wired in Patch)")
                    .small()
                    .color(theme::TEXT_DIM),
            );
            for id in off {
                if !matches!(ed.doc.graph[id].op, Op::Scan | Op::Output) {
                    node_card(ui, ed, id, false, actions);
                }
            }
        }
    }

    ui.add_space(10.0);
    presets_section(ui, ed, presets, actions);
    ui.add_space(10.0);
    tags_section(ui, ed);
    ui.add_space(10.0);
    file_section(ui, ed, collections, actions);
}

fn node_card(
    ui: &mut Ui,
    ed: &mut Editor,
    id: NodeId,
    in_chain: bool,
    actions: &mut Vec<EdAction>,
) {
    let cat = ed.doc.graph[id].op.category();
    let marker = match cat {
        Category::Experimental => theme::EXPERIMENTAL,
        Category::Basic => theme::BASIC,
        Category::Io => theme::IO,
    };
    let cursor = crate::ui::vim::rows(&ed.doc.graph)
        .get(ed.param_cursor)
        .copied()
        .filter(|r| r.0 == id);
    let selected = ed.selected == Some(id) || cursor.is_some();
    let tint = match cat {
        Category::Experimental => theme::TINT_EXP,
        Category::Basic => theme::TINT_BASIC,
        Category::Io => theme::TINT_IO,
    };
    let frame = egui::Frame::new()
        .fill(tint)
        .corner_radius(egui::CornerRadius::same(4))
        .stroke(if selected {
            Stroke::new(1.5, theme::ACCENT)
        } else {
            Stroke::new(1.0, tint.linear_multiply(0.8))
        });
    let scroll = std::mem::take(&mut ed.cursor_moved);
    let shown = frame.show(ui, |ui| {
        ui.set_width(ui.available_width());
        // Header = a patch object box: dark, light text, coloured inlet bar.
        let header = egui::Frame::new()
            .fill(if selected {
                theme::BOX_ACTIVE
            } else {
                theme::BOX
            })
            .corner_radius(egui::CornerRadius {
                nw: 4,
                ne: 4,
                sw: 0,
                se: 0,
            })
            .inner_margin(egui::Margin {
                left: 8,
                right: 4,
                top: 6,
                bottom: 3,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                {
                    let w = &mut ui.visuals_mut().widgets;
                    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
                    w.inactive.bg_fill = Color32::from_rgb(56, 58, 62);
                }
                ui.horizontal(|ui| {
                    let n = &mut ed.doc.graph[id];
                    let mut title = RichText::new(n.op.name()).strong().color(theme::BOX_TEXT);
                    if n.bypass {
                        title = title
                            .strikethrough()
                            .color(Color32::from_rgb(160, 162, 166));
                    }
                    let r = ui.add(egui::Label::new(title).sense(Sense::click()));
                    if r.clicked() {
                        ed.selected = Some(id);
                        if let Some(i) = crate::ui::vim::rows(&ed.doc.graph)
                            .iter()
                            .position(|x| x.0 == id)
                        {
                            ed.param_cursor = i;
                        }
                    }
                    if cat == Category::Experimental {
                        ui.label(
                            RichText::new("experimental")
                                .small()
                                .color(theme::EXPERIMENTAL),
                        );
                    }
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0;
                        if ui.small_button("🗙").on_hover_text("Remove").clicked() {
                            graph::remove_bridging(&mut ed.doc.graph, id);
                            if ed.selected == Some(id) {
                                ed.selected = None;
                            }
                            return;
                        }
                        if in_chain {
                            if ui.small_button("⏷").on_hover_text("Move later").clicked() {
                                graph::move_in_chain(&mut ed.doc.graph, id, 1);
                            }
                            if ui.small_button("⏶").on_hover_text("Move earlier").clicked() {
                                graph::move_in_chain(&mut ed.doc.graph, id, -1);
                            }
                        }
                        let n = &mut ed.doc.graph[id];
                        if cat == Category::Experimental
                            && ui.small_button("🎲").on_hover_text("Randomize").clicked()
                        {
                            ed.roll = ed.roll.wrapping_add(1);
                            n.op.randomize(ed.roll);
                        }
                        let mut on = !n.bypass;
                        if ui
                            .checkbox(&mut on, "")
                            .on_hover_text("Enable / bypass")
                            .changed()
                        {
                            n.bypass = !on;
                        }
                    });
                });
            });
        theme::object_bars(ui.painter(), header.response.rect, marker);
        if ed.doc.graph.get_node(id).is_none() {
            return;
        }
        egui::Frame::new()
            .inner_margin(egui::Margin {
                left: 8,
                right: 8,
                top: 6,
                bottom: 8,
            })
            .show(ui, |ui| {
                ui.set_width(ui.available_width());
                match ed.doc.graph[id].op.clone() {
                    Op::Crop(_) => crop_controls(ui, ed, id),
                    Op::WhiteBalance { .. } => {
                        ui.horizontal(|ui| {
                            if ui
                                .button("Auto WB (white)")
                                .on_hover_text(
                                    "Neutralise the brightest area — e.g. the scanner lid",
                                )
                                .clicked()
                            {
                                actions.push(EdAction::AutoWb(id, true));
                            }
                            if ui
                                .button("Auto WB (grey)")
                                .on_hover_text("Grey-world estimate")
                                .clicked()
                            {
                                actions.push(EdAction::AutoWb(id, false));
                            }
                            let picking = ed.tool == Tool::Eyedropper(id);
                            if theme::sel(ui, picking, "Eyedropper")
                                .on_hover_text("Click something neutral in the preview")
                                .clicked()
                            {
                                ed.tool = if picking {
                                    Tool::None
                                } else {
                                    Tool::Eyedropper(id)
                                };
                                ed.patch = false;
                            }
                        });
                    }
                    _ => {}
                }
                let hl = cursor.and_then(|c| c.1);
                params::op_params(ui, &mut ed.doc.graph[id].op, ("card", id.0), hl, scroll);
            });
    });
    let r = shown.response.rect;
    if scroll && cursor.is_some_and(|c| c.1.is_none()) {
        ui.scroll_to_rect(r, None);
    }
    ui.add_space(6.0);
}

fn crop_controls(ui: &mut Ui, ed: &mut Editor, id: NodeId) {
    let src = ed.crop_src_size.or_else(|| {
        ed.proxy.as_ref().map(|p| match &ed.doc.graph[id].op {
            Op::Crop(c) => c.rotated_size(p.w, p.h),
            _ => (p.w, p.h),
        })
    });
    let Op::Crop(c) = &mut ed.doc.graph[id].op else {
        return;
    };
    ui.horizontal_wrapped(|ui| {
        for a in Aspect::ALL {
            if theme::sel(ui, c.aspect == a, a.label()).clicked() {
                c.aspect = a;
                if let Some((w, h)) = src {
                    c.apply_aspect(w, h);
                }
                ed.tool = Tool::Crop;
            }
        }
    });
    ui.horizontal(|ui| {
        if ui.button("⟲").on_hover_text("Rotate left").clicked() {
            c.rot90 = (c.rot90 + 3) % 4;
            c.rect = [0.0, 0.0, 1.0, 1.0];
            if let Some((w, h)) = src {
                c.apply_aspect(h, w);
            }
            ed.crop_src_size = src.map(|(w, h)| (h, w));
        }
        if ui.button("⟳").on_hover_text("Rotate right").clicked() {
            c.rot90 = (c.rot90 + 1) % 4;
            c.rect = [0.0, 0.0, 1.0, 1.0];
            if let Some((w, h)) = src {
                c.apply_aspect(h, w);
            }
            ed.crop_src_size = src.map(|(w, h)| (h, w));
        }
        theme::toggle(ui, &mut c.flip_h, "Flip");
        let editing = ed.tool == Tool::Crop;
        if theme::sel(ui, editing, if editing { "Done" } else { "Edit crop" }).clicked() {
            ed.tool = if editing { Tool::None } else { Tool::Crop };
        }
        if ui.button("Reset").clicked() {
            *c = Default::default();
        }
    });
    ui.add(
        egui::Slider::new(&mut c.angle, -45.0..=45.0)
            .text("Straighten")
            .step_by(0.1),
    );
}

fn presets_section(ui: &mut Ui, ed: &mut Editor, presets: &[Preset], actions: &mut Vec<EdAction>) {
    theme::section(theme::TINT_IO).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("PRESETS").small().strong().color(theme::TEXT));
        let pick = ed.preset_pick.min(presets.len().saturating_sub(1));
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("preset_pick")
                .width(170.0)
                .selected_text(presets.get(pick).map(|p| p.name.as_str()).unwrap_or("—"))
                .show_ui(ui, |ui| {
                    theme::popup(ui);
                    for (kind, label) in [
                        (PresetKind::Look, "Looks"),
                        (PresetKind::Glitch, "Glitch"),
                        (PresetKind::User, "Mine"),
                    ] {
                        if !presets.iter().any(|p| p.kind == kind) {
                            continue;
                        }
                        ui.label(RichText::new(label).small().color(theme::TEXT_DIM));
                        for (i, p) in presets.iter().enumerate().filter(|(_, p)| p.kind == kind) {
                            theme::sel_value(ui, &mut ed.preset_pick, i, &p.name);
                        }
                    }
                });
            if ui.button("Apply preset").clicked() {
                actions.push(EdAction::ApplyPreset(ed.preset_pick));
            }
        });
        if let Some(p) = presets.get(pick)
            && p.kind == PresetKind::User
            && ui.small_button(format!("Delete \"{}\"", p.name)).clicked()
        {
            actions.push(EdAction::DeletePreset(p.name.clone()));
            ed.preset_pick = 0;
        }
        ui.horizontal(|ui| {
            ui.add(
                egui::TextEdit::singleline(&mut ed.preset_name)
                    .hint_text("Preset name")
                    .desired_width(150.0),
            );
            if ui
                .add_enabled(
                    !ed.preset_name.trim().is_empty(),
                    egui::Button::new("Save preset"),
                )
                .clicked()
            {
                actions.push(EdAction::SavePreset(ed.preset_name.trim().to_string()));
                ed.preset_name.clear();
            }
        });
    });
}

fn tags_section(ui: &mut Ui, ed: &mut Editor) {
    theme::section(theme::TINT_IO).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("TAGS").small().strong().color(theme::TEXT));
        let mut remove = None;
        ui.horizontal_wrapped(|ui| {
            for (i, t) in ed.doc.tags.iter().enumerate() {
                if ui
                    .button(format!("#{t} 🗙"))
                    .on_hover_text("Remove tag")
                    .clicked()
                {
                    remove = Some(i);
                }
            }
        });
        if let Some(i) = remove {
            ed.doc.tags.remove(i);
        }
        ui.horizontal(|ui| {
            let r = ui.add(
                egui::TextEdit::singleline(&mut ed.tag_input)
                    .hint_text("Tag")
                    .desired_width(150.0),
            );
            let enter = r.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
            if ui.button("Add tag").clicked() || enter {
                for t in ed
                    .tag_input
                    .split(',')
                    .map(|t| t.trim().trim_start_matches('#').to_lowercase())
                {
                    if !t.is_empty() && !ed.doc.tags.contains(&t) {
                        ed.doc.tags.push(t);
                    }
                }
                ed.tag_input.clear();
            }
        });
    });
}

fn file_section(ui: &mut Ui, ed: &mut Editor, collections: &[String], actions: &mut Vec<EdAction>) {
    if matches!(ed.item.r, ItemRef::Trashed { .. }) {
        return;
    }
    theme::section(theme::TINT_BASIC).show(ui, |ui| {
        ui.set_width(ui.available_width());
        let inbox = matches!(ed.item.r, ItemRef::Inbox(_));
        ui.label(
            RichText::new(if inbox { "FILE INTO COLLECTION" } else { "SAVE" })
                .small()
                .strong()
                .color(theme::TEXT),
        );
        if !inbox {
            ui.horizontal(|ui| {
                if ui
                    .add_enabled(!ed.exporting, egui::Button::new("Save & render"))
                    .on_hover_text("⌘S")
                    .clicked()
                {
                    actions.push(EdAction::Save);
                }
                if ui.button("Reveal in Finder").clicked() {
                    actions.push(EdAction::Reveal);
                }
            });
        }
        if collections.is_empty() {
            ui.label(RichText::new("Create a collection in the sidebar first.").small().color(theme::TEXT));
            return;
        }
        if !collections.contains(&ed.file_target) {
            ed.file_target = collections[0].clone();
        }
        ui.horizontal(|ui| {
            egui::ComboBox::from_id_salt("file_target")
                .selected_text(&ed.file_target)
                .show_ui(ui, |ui| {
                    theme::popup(ui);
                    for c in collections {
                        theme::sel_value(ui, &mut ed.file_target, c.clone(), c);
                    }
                });
            if inbox && ui.button(format!("File into {}", ed.file_target)).clicked() {
                actions.push(EdAction::FileInto(ed.file_target.clone()));
            }
            if ui
                .button(format!("Save copy to {}", ed.file_target))
                .on_hover_text("⇧⌘S · :saveas — a new photo in that collection with these edits; this one stays as it is")
                .clicked()
            {
                actions.push(EdAction::SaveCopy(ed.file_target.clone()));
            }
        });
        ui.label(
            RichText::new(if inbox {
                "File moves the scan and opens the next Inbox scan. Save copy keeps it in the Inbox."
            } else {
                "Save copy makes a new numbered photo (e.g. a variant with another look)."
            })
            .small()
            .color(theme::TEXT_DIM),
        );
    });
}

// ------------------------------------------------------------------ centre

fn patch_center(ui: &mut Ui, ed: &mut Editor) {
    patch::show(ui, &mut ed.doc.graph, &mut ed.selected, &mut ed.roll);
}

fn preview_area(ui: &mut Ui, ed: &mut Editor, actions: &mut Vec<EdAction>) {
    let avail = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(avail, Sense::click_and_drag());
    let painter = ui.painter_at(avail);
    let Some(tex) = &ed.preview else {
        let msg = match &ed.load_error {
            Some(e) => format!("Couldn't load: {e}"),
            None => "Loading…".into(),
        };
        painter.text(
            avail.center(),
            egui::Align2::CENTER_CENTER,
            msg,
            egui::FontId::proportional(14.0),
            theme::TEXT_DIM,
        );
        return;
    };
    let tex_id = tex.id();
    let content = vec2(
        ed.preview_size[0].max(1) as f32,
        ed.preview_size[1].max(1) as f32,
    );
    // Zoom / pan (primary drag pans only when no tool needs the drag).
    ed.zoom.rect(avail.shrink(20.0), content);
    ed.zoom.handle(ui, &resp, ed.tool == Tool::None);
    let r = ed.zoom.rect(avail.shrink(20.0), content);
    if ed.zoom.scale() > 1.0 && resp.hovered() && ed.tool == Tool::None {
        ui.ctx().set_cursor_icon(if resp.dragged() {
            egui::CursorIcon::Grabbing
        } else {
            egui::CursorIcon::Grab
        });
    }
    painter.image(
        tex_id,
        r,
        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
        Color32::WHITE,
    );
    if ed.compare {
        painter.text(
            r.left_top() + vec2(8.0, 8.0),
            egui::Align2::LEFT_TOP,
            "BEFORE",
            egui::FontId::proportional(13.0),
            theme::TEXT_ON_CANVAS,
        );
    }

    match ed.tool {
        Tool::Crop => crop_overlay(ui, ed, &resp, r),
        Tool::Eyedropper(node) => {
            if resp.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::Crosshair);
            }
            painter.text(
                avail.center_top() + vec2(0.0, 8.0),
                egui::Align2::CENTER_TOP,
                "Click something neutral (white / grey) · Esc to cancel",
                egui::FontId::proportional(13.0),
                theme::TEXT_ON_CANVAS,
            );
            if resp.clicked()
                && let Some(p) = resp.interact_pointer_pos()
                && r.contains(p)
            {
                let u = (p.x - r.min.x) / r.width();
                let v = (p.y - r.min.y) / r.height();
                actions.push(EdAction::PickWb(node, u, v));
                ed.tool = Tool::None;
            }
        }
        Tool::None => {}
    }
}

fn crop_overlay(ui: &mut Ui, ed: &mut Editor, resp: &egui::Response, r: Rect) {
    let Some(cid) = graph::crop_node(&ed.doc.graph) else {
        ed.tool = Tool::None;
        return;
    };
    let size = ed
        .crop_src_size
        .unwrap_or((ed.preview_size[0], ed.preview_size[1]));
    let Op::Crop(c) = &mut ed.doc.graph[cid].op else {
        return;
    };
    let to_screen = |x: f32, y: f32| pos2(r.min.x + x * r.width(), r.min.y + y * r.height());
    let [x, y, w, h] = c.rect;
    let cr = Rect::from_min_max(to_screen(x, y), to_screen(x + w, y + h));
    let corners = [
        cr.left_top(),
        cr.right_top(),
        cr.right_bottom(),
        cr.left_bottom(),
    ];

    // Interaction.
    if resp.drag_started()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let handle = corners
            .iter()
            .position(|q| q.distance(p) < 14.0)
            .map(|i| i as u8 + 1)
            .unwrap_or(if cr.contains(p) { 0 } else { 9 });
        ed.crop_drag = (handle != 9).then_some(CropDrag {
            handle,
            start_rect: c.rect,
            start_pos: p,
        });
    }
    if resp.dragged()
        && let (Some(d), Some(p)) = (ed.crop_drag, resp.interact_pointer_pos())
    {
        let [sx, sy, sw, sh] = d.start_rect;
        if d.handle == 0 {
            let dx = (p.x - d.start_pos.x) / r.width();
            let dy = (p.y - d.start_pos.y) / r.height();
            c.rect = [
                (sx + dx).clamp(0.0, 1.0 - sw),
                (sy + dy).clamp(0.0, 1.0 - sh),
                sw,
                sh,
            ];
        } else {
            // Fixed (opposite) corner.
            let (fx, fy) = match d.handle {
                1 => (sx + sw, sy + sh),
                2 => (sx, sy + sh),
                3 => (sx, sy),
                _ => (sx + sw, sy),
            };
            let px = ((p.x - r.min.x) / r.width()).clamp(0.0, 1.0);
            let py = ((p.y - r.min.y) / r.height()).clamp(0.0, 1.0);
            let (mut nw, mut nh) = ((px - fx).abs().max(0.01), (py - fy).abs().max(0.01));
            if let Some(ratio) = c.aspect.ratio(size.0 as f32, size.1 as f32) {
                // Normalised ratio: w_n / h_n = ratio * H / W.
                let rn = ratio * size.1 as f32 / size.0 as f32;
                if nw / nh > rn {
                    nw = nh * rn;
                } else {
                    nh = nw / rn;
                }
                let maxw = if px < fx { fx } else { 1.0 - fx };
                let maxh = if py < fy { fy } else { 1.0 - fy };
                let s = (maxw / nw).min(maxh / nh).min(1.0);
                nw *= s;
                nh *= s;
            }
            let nx = if px < fx { fx - nw } else { fx };
            let ny = if py < fy { fy - nh } else { fy };
            c.rect = [nx, ny, nw, nh];
        }
    }
    if resp.drag_stopped() {
        ed.crop_drag = None;
    }
    if resp.double_clicked() {
        ed.tool = Tool::None;
    }

    // Drawing.
    let [x, y, w, h] = c.rect;
    let cr = Rect::from_min_max(to_screen(x, y), to_screen(x + w, y + h));
    let p = ui.painter();
    let shade = Color32::from_black_alpha(150);
    p.rect_filled(
        Rect::from_min_max(r.min, pos2(r.max.x, cr.min.y)),
        0.0,
        shade,
    );
    p.rect_filled(
        Rect::from_min_max(pos2(r.min.x, cr.max.y), r.max),
        0.0,
        shade,
    );
    p.rect_filled(
        Rect::from_min_max(pos2(r.min.x, cr.min.y), pos2(cr.min.x, cr.max.y)),
        0.0,
        shade,
    );
    p.rect_filled(
        Rect::from_min_max(pos2(cr.max.x, cr.min.y), pos2(r.max.x, cr.max.y)),
        0.0,
        shade,
    );
    let thin = Stroke::new(1.0, Color32::from_white_alpha(110));
    for k in 1..3 {
        let fx = cr.min.x + cr.width() * k as f32 / 3.0;
        let fy = cr.min.y + cr.height() * k as f32 / 3.0;
        p.line_segment([pos2(fx, cr.min.y), pos2(fx, cr.max.y)], thin);
        p.line_segment([pos2(cr.min.x, fy), pos2(cr.max.x, fy)], thin);
    }
    p.rect_stroke(
        cr,
        0.0,
        Stroke::new(1.5, theme::ACCENT),
        egui::StrokeKind::Middle,
    );
    for q in [
        cr.left_top(),
        cr.right_top(),
        cr.right_bottom(),
        cr.left_bottom(),
    ] {
        p.rect_filled(
            Rect::from_center_size(q, vec2(10.0, 10.0)),
            2.0,
            Color32::WHITE,
        );
    }
    let px_w = (w * size.0 as f32).round();
    let px_h = (h * size.1 as f32).round();
    p.text(
        cr.center_bottom() + vec2(0.0, 6.0),
        egui::Align2::CENTER_TOP,
        format!(
            "{} · {px_w:.0}×{px_h:.0} preview px · double-click or Esc when done",
            c.aspect.label()
        ),
        egui::FontId::proportional(12.0),
        Color32::WHITE,
    );
}
