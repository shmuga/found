//! Collage workspace: canvas with layers, templates, lasso / rect masks.

use super::theme;
use crate::app::{CDrag, CTool, CollageEd, FoundApp, Msg};
use crate::collage::{self, CANVAS_PRESETS, Mask, Stack, Template};
use egui::{Color32, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use std::sync::Arc;

const PREVIEW_LONG: f32 = 1600.0;
const SOURCE_EDGE: usize = 1600;

enum CAction {
    Export,
    Rename(String),
    Add {
        rel: String,
        name: String,
        aspect: f32,
    },
}

impl FoundApp {
    /// Collage tools in the bottom bar: left = tools + history; right = save, export, trash, zoom.
    pub fn collage_tools(&mut self, ui: &mut Ui, ctx: &egui::Context, right: bool) {
        use theme::{Icon, tool};
        let dirty = self.collage_dirty();
        let Some(c) = &mut self.collage else { return };
        let zoom = c.zoom.percent();
        let (mut undo, mut redo, mut save, mut export, mut trash, mut fit) =
            (false, false, false, false, false, false);
        if !right {
            for (t, icon, l, tip) in [
                (
                    CTool::Move,
                    Icon::Move,
                    "Move",
                    "Move (s): drag layers; corner scales, top handle rotates",
                ),
                (
                    CTool::Lasso,
                    Icon::Lasso,
                    "Lasso cut",
                    "Lasso cut (m): draw around the part of the selected layer to keep",
                ),
                (
                    CTool::RectMask,
                    Icon::RectMask,
                    "Rect mask",
                    "Rect mask (M): drag a rectangle on the selected layer to keep",
                ),
            ] {
                if tool(ui, icon, l, tip, Some(c.tool == t), true).clicked() {
                    c.tool = t;
                    c.lasso.clear();
                }
            }
            ui.add_space(8.0);
            undo = tool(
                ui,
                Icon::Undo,
                "Undo",
                "Undo (u · ⌘Z)",
                None,
                c.history.can_undo(),
            )
            .clicked();
            redo = tool(
                ui,
                Icon::Redo,
                "Redo",
                "Redo (Ctrl-r · ⇧⌘Z)",
                None,
                c.history.can_redo(),
            )
            .clicked();
        } else {
            fit = tool(ui, Icon::Fit, "Fit", "Fit (= · ⌘0)", None, true).clicked();
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
                "Move this collage (and its JPEG) to Trash (⌘Backspace)",
                None,
                true,
            )
            .clicked();
            export = tool(
                ui,
                Icon::Export,
                "Export collage",
                "Render the full-resolution JPEG (:export)",
                None,
                !c.doc.layers.is_empty() && !c.exporting,
            )
            .clicked();
            save = tool(ui, Icon::Save, "Save", "Save (⌘S · :w)", None, dirty).clicked();
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
        if save {
            self.save_collage();
            self.status = "Collage saved".into();
        }
        if export {
            self.export_collage(ctx);
        }
        if fit {
            self.act(ctx, crate::keys::Act::ZoomFit);
        }
        if trash && let Some(p) = self.collage.as_ref().map(|c| c.path.clone()) {
            self.trash_collage(p);
        }
    }

    pub fn collage_view(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.collage.is_none() {
            self.view = crate::app::View::Library;
            return;
        }
        let Some(root) = self.lib.as_ref().map(|l| l.root.clone()) else {
            return;
        };
        let mut actions = vec![];

        // Ensure layer sources are loading.
        {
            let c = self.collage.as_mut().unwrap();
            for l in &c.doc.layers {
                if !c.sources.contains_key(&l.source) && c.pending.insert(l.source.clone()) {
                    let key = l.source.clone();
                    let path = root.join(&l.source);
                    self.worker.spawn_pooled(&ctx, move || {
                        Msg::CollageSrc(
                            key,
                            crate::io::load_scaled(&path, SOURCE_EDGE)
                                .ok()
                                .map(Arc::new),
                        )
                    });
                }
            }
        }

        // Picker data (thumbnails need &mut self, so collect first).
        let pick = self
            .collage
            .as_ref()
            .map(|c| c.pick_collection.clone())
            .unwrap_or_default();
        let pick_items = self
            .lib
            .as_ref()
            .map(|l| l.items(&pick))
            .unwrap_or_default();
        let mut thumbs = vec![];
        for it in &pick_items {
            let t = self.thumb(&ctx, it.display_path());
            let size = self.thumbs.get(it.display_path()).map(|t| t.size_vec2());
            thumbs.push((t, size));
        }
        let collections = self.collections.clone();
        let busy = self.collage.as_ref().is_some_and(|c| c.exporting);

        egui::Panel::right("collage_panel")
            .resizable(true)
            .default_size(320.0)
            .frame(
                egui::Frame::new()
                    .fill(theme::PANEL)
                    .inner_margin(egui::Margin::same(8)),
            )
            .show(ui, |ui| {
                let c = self.collage.as_mut().unwrap();
                egui::ScrollArea::vertical().show(ui, |ui| {
                    canvas_section(ui, c, busy, &mut actions);
                    ui.add_space(8.0);
                    layers_section(ui, c);
                    ui.add_space(8.0);
                    picker_section(
                        ui,
                        c,
                        &collections,
                        &pick_items,
                        &thumbs,
                        &root,
                        &mut actions,
                    );
                });
            });
        egui::CentralPanel::default()
            .frame(egui::Frame::new().fill(theme::CANVAS))
            .show(ui, |ui| {
                let c = self.collage.as_mut().unwrap();
                canvas(ui, c);
            });

        for a in actions {
            match a {
                CAction::Add { rel, name, aspect } => {
                    if let Some(c) = &mut self.collage {
                        let id = c.doc.add_layer(rel, name, aspect);
                        c.selected = Some(id);
                    }
                }
                CAction::Export => self.export_collage(&ctx),
                CAction::Rename(n) => {
                    if let Some(p) = self.collage.as_ref().map(|c| c.path.clone()) {
                        self.rename_collage(&p, &n);
                    }
                }
            }
        }
    }

    pub fn export_collage(&mut self, ctx: &egui::Context) {
        let Some(root) = self.lib.as_ref().map(|l| l.root.clone()) else {
            return;
        };
        let quality = self.settings.jpeg_quality.clamp(50, 100);
        let Some(c) = &mut self.collage else { return };
        if c.doc.layers.is_empty() || c.exporting {
            return;
        }
        c.exporting = true;
        let doc = c.doc.clone();
        let out = c.path.with_extension("jpg");
        let _ = collage::save(&c.path, &c.doc);
        self.status = "Exporting collage at full resolution…".into();
        self.worker.spawn(ctx, move || {
            let img = collage::composite(&doc, 1.0, 4096, |l| {
                crate::io::load_full(&root.join(&l.source))
                    .ok()
                    .map(Arc::new)
            });
            Msg::CollageExported(
                crate::io::save_jpeg(&img, &out, quality)
                    .map(|_| out)
                    .map_err(|e| format!("{e:#}")),
            )
        });
    }
}

fn canvas_section(ui: &mut Ui, c: &mut CollageEd, busy: bool, actions: &mut Vec<CAction>) {
    theme::section(theme::TINT_IO).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.horizontal(|ui| {
            ui.label(RichText::new("CANVAS").small().strong().color(theme::TEXT));
            ui.label(RichText::new(format!("{}×{}", c.doc.width, c.doc.height)).small());
        });
        ui.horizontal(|ui| {
            ui.label("Name");
            let r = ui.add(egui::TextEdit::singleline(&mut c.doc.name).id_salt("collage_name"));
            // Committing a new name renames the collage's files too.
            let stem = c
                .path
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or_default()
                .to_string();
            if r.lost_focus() && collage::file_stem_for(&c.doc.name) != stem {
                actions.push(CAction::Rename(c.doc.name.clone()));
            }
        })
        .response
        .on_hover_text("Renames the collage (and its files) when you press Enter · :rename");
        let cur = CANVAS_PRESETS
            .iter()
            .find(|p| p.w == c.doc.width && p.h == c.doc.height)
            .map(|p| p.name)
            .unwrap_or("Custom");
        egui::ComboBox::from_id_salt("canvas_size")
            .selected_text(cur)
            .show_ui(ui, |ui| {
                theme::popup(ui);
                for p in CANVAS_PRESETS {
                    if theme::sel(ui, cur == p.name, p.name).clicked() {
                        c.doc.width = p.w;
                        c.doc.height = p.h;
                        c.doc.refit_all();
                    }
                }
            });
        ui.horizontal(|ui| {
            ui.label("Background");
            ui.color_edit_button_srgb(&mut c.doc.background);
        });
        let mut t = c.doc.template;
        egui::ComboBox::from_label("Template")
            .selected_text(t.label())
            .show_ui(ui, |ui| {
                theme::popup(ui);
                for x in Template::ALL {
                    theme::sel_value(ui, &mut t, x, x.label());
                }
            });
        if t != c.doc.template {
            c.doc.retemplate(t);
        }
        if c.doc.template != Template::Free {
            let g = ui
                .add(egui::Slider::new(&mut c.doc.gutter, 0.0..=0.1).text("Gutter"))
                .changed();
            let m = ui
                .add(egui::Slider::new(&mut c.doc.margin, 0.0..=0.15).text("Margin"))
                .changed();
            if g || m {
                c.doc.refit_all();
            }
        }
        ui.add_space(4.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(
                    !busy && !c.doc.layers.is_empty(),
                    egui::Button::new("Export JPEG"),
                )
                .clicked()
            {
                actions.push(CAction::Export);
            }
            if busy {
                ui.spinner();
            }
            if let Some(p) = &c.last_export
                && ui.small_button("Reveal").clicked()
            {
                let _ = std::process::Command::new("open").arg("-R").arg(p).spawn();
            }
        });
    });
}

fn layers_section(ui: &mut Ui, c: &mut CollageEd) {
    theme::section(theme::TINT_BASIC).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(RichText::new("LAYERS").small().strong().color(theme::TEXT));
        if c.doc.layers.is_empty() {
            ui.label(RichText::new("Add photos from the list below.").small());
        }
        // Top of the list = front. Drag the grip to reorder; right-click for stacking.
        let n = c.doc.layers.len();
        let mut delete: Option<u32> = None;
        let mut stack: Option<(u32, Stack)> = None;
        let mut rows: Vec<(u32, Rect)> = vec![];
        for i in (0..n).rev() {
            let id = c.doc.layers[i].id;
            let row = ui.horizontal(|ui| {
                let (grip, gr) = ui.allocate_exact_size(vec2(12.0, 18.0), Sense::drag());
                let col = if gr.hovered() || c.layer_drag == Some(id) {
                    theme::TEXT
                } else {
                    theme::TEXT_DIM
                };
                for k in 0..3 {
                    let y = grip.center().y - 4.0 + k as f32 * 4.0;
                    ui.painter()
                        .hline(grip.x_range().shrink(1.0), y, Stroke::new(1.5, col));
                }
                if gr.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
                }
                if gr.drag_started() {
                    c.layer_drag = Some(id);
                    c.selected = Some(id);
                }
                let l = &mut c.doc.layers[i];
                theme::check(ui, &mut l.visible, "").on_hover_text("Visible");
                let sel = c.selected == Some(l.id);
                let mut label = l.name.clone();
                if l.mask != Mask::None {
                    label.push_str(" ✂");
                }
                if l.locked {
                    label.push_str(" 🔒");
                }
                let r = theme::sel(ui, sel, label);
                if r.clicked() || r.secondary_clicked() {
                    c.selected = Some(id);
                }
                r.context_menu(|ui| {
                    theme::popup(ui);
                    if let Some(s) = stack_menu(ui) {
                        stack = Some((id, s));
                    }
                    ui.separator();
                    if ui.button("Delete layer").clicked() {
                        delete = Some(id);
                    }
                });
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 2.0;
                    if ui.small_button("🗙").on_hover_text("Delete layer").clicked() {
                        delete = Some(id);
                    }
                    if ui
                        .small_button("⏷")
                        .on_hover_text("Send backward")
                        .clicked()
                    {
                        stack = Some((id, Stack::Backward));
                    }
                    if ui
                        .small_button("⏶")
                        .on_hover_text("Bring forward")
                        .clicked()
                    {
                        stack = Some((id, Stack::Forward));
                    }
                });
            });
            rows.push((id, row.response.rect));
        }
        // While dragging: show where it will land; drop moves it there.
        if let Some(drag) = c.layer_drag {
            let pointer = ui.input(|i| i.pointer.hover_pos());
            // rows are listed front → back; slot k means "above row k".
            let slot = pointer.map(|p| {
                rows.iter()
                    .position(|(_, r)| p.y < r.center().y)
                    .unwrap_or(rows.len())
            });
            if let Some(k) = slot {
                let y = if k < rows.len() {
                    rows[k].1.top() - 1.0
                } else {
                    rows.last().map(|r| r.1.bottom() + 1.0).unwrap_or(0.0)
                };
                let x = rows
                    .first()
                    .map(|r| r.1.x_range())
                    .unwrap_or(ui.max_rect().x_range());
                ui.painter().hline(x, y, Stroke::new(2.0, theme::ACCENT));
            }
            if !ui.input(|i| i.pointer.primary_down()) {
                c.layer_drag = None;
                if let Some(k) = slot {
                    // List slot k (front→back, excluding the dragged row) → vec index (back→front).
                    let above: Vec<u32> = rows
                        .iter()
                        .take(k)
                        .map(|r| r.0)
                        .filter(|x| *x != drag)
                        .collect();
                    let to = n - 1 - above.len();
                    c.doc.move_layer(drag, to);
                }
            }
        }
        if let Some((id, s)) = stack {
            c.doc.restack(id, s);
        }
        if let Some(id) = delete {
            c.doc.layers.retain(|l| l.id != id);
            if c.selected == Some(id) {
                c.selected = None;
            }
        }

        let cells = c.doc.cells();
        let Some(idx) = c
            .selected
            .and_then(|id| c.doc.layers.iter().position(|l| l.id == id))
        else {
            return;
        };
        ui.separator();
        let max_w = c.doc.width.max(c.doc.height) as f32 * 2.0;
        let mut refit = false;
        {
            let l = &mut c.doc.layers[idx];
            ui.add(
                egui::Slider::new(&mut l.width, 20.0..=max_w)
                    .text("Width px")
                    .logarithmic(true),
            );
            ui.add(egui::Slider::new(&mut l.rotation, -180.0..=180.0).text("Rotation"));
            ui.add(egui::Slider::new(&mut l.opacity, 0.0..=1.0).text("Opacity"));
            theme::check(ui, &mut l.locked, "Locked");
            if !cells.is_empty() {
                let mut cell = l.cell;
                egui::ComboBox::from_id_salt("layer_cell")
                    .selected_text(
                        cell.map(|x| format!("Frame {}", x + 1))
                            .unwrap_or("Free".into()),
                    )
                    .show_ui(ui, |ui| {
                        theme::popup(ui);
                        theme::sel_value(ui, &mut cell, None, "Free");
                        for k in 0..cells.len() {
                            theme::sel_value(ui, &mut cell, Some(k), format!("Frame {}", k + 1));
                        }
                    });
                if cell != l.cell {
                    l.cell = cell;
                    refit = true;
                }
            }
            ui.horizontal(|ui| {
                if ui.button("Fit").clicked() {
                    refit = true;
                }
                if ui
                    .add_enabled(l.mask != Mask::None, egui::Button::new("Clear mask"))
                    .clicked()
                {
                    l.mask = Mask::None;
                }
            });
        }
        if refit {
            let mut l = c.doc.layers[idx].clone();
            c.doc.fit_layer(&mut l);
            c.doc.layers[idx] = l;
        }
    });
}

fn picker_section(
    ui: &mut Ui,
    c: &mut CollageEd,
    collections: &[String],
    items: &[crate::library::Item],
    thumbs: &[(Option<egui::TextureId>, Option<egui::Vec2>)],
    root: &std::path::Path,
    actions: &mut Vec<CAction>,
) {
    theme::section(theme::TINT_EXP).show(ui, |ui| {
        ui.set_width(ui.available_width());
        ui.label(
            RichText::new("ADD PHOTOS")
                .small()
                .strong()
                .color(theme::TEXT),
        );
        egui::ComboBox::from_id_salt("pick_collection")
            .selected_text(&c.pick_collection)
            .show_ui(ui, |ui| {
                theme::popup(ui);
                for col in collections {
                    theme::sel_value(ui, &mut c.pick_collection, col.clone(), col);
                }
            });
        for (it, (tex, size)) in items.iter().zip(thumbs) {
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(48.0, 48.0), Sense::hover());
                if let (Some(t), Some(s)) = (tex, size) {
                    let k = (48.0 / s.x).min(48.0 / s.y);
                    ui.painter().image(
                        *t,
                        Rect::from_center_size(rect.center(), *s * k),
                        Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                        Color32::WHITE,
                    );
                } else {
                    ui.painter().rect_filled(rect, 3.0, theme::BOX);
                }
                let title = it.title();
                if ui.button(format!("Add {title}")).clicked() {
                    let path = it.display_path();
                    let rel = path
                        .strip_prefix(root)
                        .unwrap_or(path)
                        .to_string_lossy()
                        .to_string();
                    let aspect = crate::io::image_size(path)
                        .map(|(w, h)| w as f32 / h as f32)
                        .unwrap_or(1.5);
                    actions.push(CAction::Add {
                        rel,
                        name: title,
                        aspect,
                    });
                }
            });
        }
    });
}

fn canvas(ui: &mut Ui, c: &mut CollageEd) {
    let avail = ui.available_rect_before_wrap();
    let resp = ui.allocate_rect(avail, Sense::click_and_drag());
    let (cw, ch) = (c.doc.width as f32, c.doc.height as f32);
    let view = avail.shrink(24.0);
    // Zoom / pan: pinch or ⌘-scroll zooms, scroll / middle-drag / Space-drag pans.
    c.zoom.rect(view, vec2(cw, ch));
    c.zoom.handle(ui, &resp, false);
    let r = c.zoom.rect(view, vec2(cw, ch));
    let s = r.width() / cw;
    let to_canvas = |p: Pos2| ((p.x - r.min.x) / s, (p.y - r.min.y) / s);
    let to_screen = |x: f32, y: f32| pos2(r.min.x + x * s, r.min.y + y * s);

    // Re-composite the preview when anything changed.
    let key = format!(
        "{}|{}",
        serde_json::to_string(&c.doc).unwrap_or_default(),
        c.sources.len()
    );
    // Render the preview at roughly the displayed resolution (in coarse steps), capped.
    let ppp = ui.ctx().pixels_per_point();
    let max_side = ui.ctx().input(|i| i.max_texture_side).min(4096) as f32;
    let want = (r.width().max(r.height()) * ppp).clamp(PREVIEW_LONG, max_side);
    let bucket = (want / 512.0).ceil() * 512.0;
    let key = format!("{key}|{bucket}");
    if key != c.preview_key {
        c.preview_key = key;
        let scale = (bucket / cw.max(ch)).min(1.0);
        let sources = &c.sources;
        let img = collage::composite(&c.doc, scale, 1024, |l| sources.get(&l.source).cloned());
        let ci = img.to_color_image();
        match &mut c.preview {
            Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
            None => {
                c.preview = Some(
                    ui.ctx()
                        .load_texture("collage", ci, egui::TextureOptions::LINEAR),
                )
            }
        }
    }
    let painter = ui.painter_at(avail);
    painter.rect_filled(
        r.translate(vec2(3.0, 4.0)),
        0.0,
        Color32::from_black_alpha(50),
    );
    if let Some(t) = &c.preview {
        painter.image(
            t.id(),
            r,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );
    }
    // Template frames.
    for (i, [x, y, w, h]) in c.doc.cells().into_iter().enumerate() {
        let fr = Rect::from_min_max(to_screen(x, y), to_screen(x + w, y + h));
        painter.rect_stroke(
            fr,
            0.0,
            Stroke::new(1.0, Color32::from_black_alpha(60)),
            egui::StrokeKind::Inside,
        );
        if !c.doc.layers.iter().any(|l| l.cell == Some(i)) {
            painter.text(
                fr.center(),
                egui::Align2::CENTER_CENTER,
                format!("Frame {}", i + 1),
                egui::FontId::proportional(13.0),
                Color32::from_black_alpha(110),
            );
        }
    }

    let sel_idx = c
        .selected
        .and_then(|id| c.doc.layers.iter().position(|l| l.id == id));
    let handles = sel_idx.map(|i| {
        let l = &c.doc.layers[i];
        let cs = l.corners().map(|(x, y)| to_screen(x, y));
        let (tx, ty) = l.uv_to_canvas(0.5, 0.0);
        let top = to_screen(tx, ty);
        let centre = to_screen(l.cx, l.cy);
        let dir = (top - centre).normalized();
        (cs, top + dir * 22.0, centre)
    });

    // ---- right-click: select the layer under the pointer, then a stacking menu.
    if resp.secondary_clicked()
        && let Some(p) = resp.interact_pointer_pos()
    {
        let (px, py) = to_canvas(p);
        if let Some(id) = c
            .doc
            .layers
            .iter()
            .rev()
            .find(|l| l.visible && l.hit(px, py, None))
            .map(|l| l.id)
        {
            c.selected = Some(id);
        }
    }
    let mut stack = None;
    let mut delete = false;
    resp.context_menu(|ui| {
        theme::popup(ui);
        if c.selected.is_none() {
            ui.label(RichText::new("Right-click a layer").color(theme::TEXT_DIM));
            return;
        }
        stack = stack_menu(ui);
        ui.separator();
        if ui.button("Delete layer").clicked() {
            delete = true;
        }
    });
    if let Some(id) = c.selected {
        if let Some(s) = stack {
            c.doc.restack(id, s);
        }
        if delete {
            c.doc.layers.retain(|l| l.id != id);
            c.selected = None;
        }
    }

    // ---- interaction
    let pointer = resp.interact_pointer_pos();
    let panning =
        ui.input(|i| i.key_down(egui::Key::Space)) || resp.dragged_by(egui::PointerButton::Middle);
    let tool = if panning { None } else { Some(c.tool) };
    match tool {
        None => {}
        Some(CTool::Move) => {
            if resp.drag_started()
                && let Some(p) = pointer
            {
                c.drag = None;
                if let (Some(i), Some((cs, rot, _))) = (sel_idx, handles) {
                    let l = &c.doc.layers[i];
                    if !l.locked {
                        if cs[2].distance(p) < 14.0 {
                            let (px, py) = to_canvas(p);
                            let d = ((px - l.cx).powi(2) + (py - l.cy).powi(2)).sqrt().max(1.0);
                            c.drag = Some(CDrag::Scale {
                                id: l.id,
                                start_dist: d,
                                width: l.width,
                            });
                        } else if rot.distance(p) < 14.0 {
                            c.drag = Some(CDrag::Rotate { id: l.id });
                        }
                    }
                }
                if c.drag.is_none() {
                    let (px, py) = to_canvas(p);
                    let hit = c
                        .doc
                        .layers
                        .iter()
                        .rev()
                        .find(|l| l.visible && l.hit(px, py, None))
                        .map(|l| (l.id, l.locked, l.cx, l.cy));
                    c.selected = hit.map(|h| h.0);
                    if let Some((id, false, cx, cy)) = hit {
                        c.drag = Some(CDrag::Move {
                            id,
                            start: p,
                            cx,
                            cy,
                        });
                    }
                }
            }
            if resp.clicked()
                && let Some(p) = pointer
            {
                let (px, py) = to_canvas(p);
                c.selected = c
                    .doc
                    .layers
                    .iter()
                    .rev()
                    .find(|l| l.visible && l.hit(px, py, None))
                    .map(|l| l.id);
            }
            if resp.dragged()
                && let (Some(d), Some(p)) = (c.drag, pointer)
            {
                let shift = ui.input(|i| i.modifiers.shift);
                let id = match d {
                    CDrag::Move { id, .. } | CDrag::Scale { id, .. } | CDrag::Rotate { id } => id,
                };
                if let Some(l) = c.doc.layers.iter_mut().find(|l| l.id == id) {
                    let (px, py) = to_canvas(p);
                    match d {
                        CDrag::Move { start, cx, cy, .. } => {
                            l.cx = cx + (p.x - start.x) / s;
                            l.cy = cy + (p.y - start.y) / s;
                        }
                        CDrag::Scale {
                            start_dist, width, ..
                        } => {
                            let dist = ((px - l.cx).powi(2) + (py - l.cy).powi(2)).sqrt();
                            l.width = (width * dist / start_dist).max(10.0);
                        }
                        CDrag::Rotate { .. } => {
                            let mut deg = (px - l.cx).atan2(-(py - l.cy)).to_degrees();
                            if shift {
                                deg = (deg / 15.0).round() * 15.0;
                            }
                            l.rotation = deg;
                        }
                    }
                }
            }
            if resp.drag_stopped() {
                c.drag = None;
            }
            // ⌥-scroll scales the selected layer (plain scroll pans the view).
            if resp.hovered()
                && let Some(i) = sel_idx
            {
                let dy = ui.input(|i| {
                    if i.modifiers.alt {
                        i.smooth_scroll_delta.y + i.smooth_scroll_delta.x
                    } else {
                        0.0
                    }
                });
                if dy != 0.0 && !c.doc.layers[i].locked {
                    c.doc.layers[i].width = (c.doc.layers[i].width * (dy * 0.003).exp()).max(10.0);
                }
            }
        }
        Some(CTool::Lasso) => {
            if let Some(i) = sel_idx {
                if resp.drag_started() {
                    c.lasso.clear();
                }
                if resp.dragged()
                    && let Some(p) = pointer
                {
                    let (px, py) = to_canvas(p);
                    let (u, v) = c.doc.layers[i].canvas_to_uv(px, py);
                    let pt = [u.clamp(0.0, 1.0), v.clamp(0.0, 1.0)];
                    let far = c
                        .lasso
                        .last()
                        .is_none_or(|q| (q[0] - pt[0]).abs() + (q[1] - pt[1]).abs() > 0.003);
                    if far {
                        c.lasso.push(pt);
                    }
                }
                if resp.drag_stopped() {
                    if c.lasso.len() >= 3 {
                        c.doc.layers[i].mask = Mask::Poly {
                            pts: std::mem::take(&mut c.lasso),
                        };
                    }
                    c.lasso.clear();
                }
            }
        }
        Some(CTool::RectMask) => {
            if let Some(i) = sel_idx {
                if resp.drag_started() {
                    c.rect_start = pointer;
                }
                if resp.drag_stopped()
                    && let (Some(a), Some(b)) = (c.rect_start.take(), pointer)
                {
                    let l = &c.doc.layers[i];
                    let pts = [a, b, pos2(a.x, b.y), pos2(b.x, a.y)].map(|p| {
                        let (x, y) = to_canvas(p);
                        l.canvas_to_uv(x, y)
                    });
                    let u0 = pts
                        .iter()
                        .map(|p| p.0)
                        .fold(f32::MAX, f32::min)
                        .clamp(0.0, 1.0);
                    let v0 = pts
                        .iter()
                        .map(|p| p.1)
                        .fold(f32::MAX, f32::min)
                        .clamp(0.0, 1.0);
                    let u1 = pts
                        .iter()
                        .map(|p| p.0)
                        .fold(f32::MIN, f32::max)
                        .clamp(0.0, 1.0);
                    let v1 = pts
                        .iter()
                        .map(|p| p.1)
                        .fold(f32::MIN, f32::max)
                        .clamp(0.0, 1.0);
                    if u1 - u0 > 0.01 && v1 - v0 > 0.01 {
                        c.doc.layers[i].mask = Mask::Rect {
                            r: [u0, v0, u1 - u0, v1 - v0],
                        };
                    }
                }
            }
        }
    }

    // ---- overlays
    if let Some((cs, rot, centre)) = handles {
        let col = theme::ACCENT;
        painter.add(egui::Shape::closed_line(cs.to_vec(), Stroke::new(1.5, col)));
        if c.tool == CTool::Move {
            painter.rect_filled(
                Rect::from_center_size(cs[2], vec2(11.0, 11.0)),
                2.0,
                Color32::WHITE,
            );
            painter.rect_stroke(
                Rect::from_center_size(cs[2], vec2(11.0, 11.0)),
                2.0,
                Stroke::new(1.0, col),
                egui::StrokeKind::Middle,
            );
            let top_mid = pos2((cs[0].x + cs[1].x) * 0.5, (cs[0].y + cs[1].y) * 0.5);
            painter.line_segment([top_mid, rot], Stroke::new(1.0, col));
            painter.circle(rot, 6.0, Color32::WHITE, Stroke::new(1.0, col));
        }
        let _ = centre;
        if c.tool == CTool::Lasso
            && c.lasso.len() > 1
            && let Some(i) = sel_idx
        {
            let l = &c.doc.layers[i];
            let pts: Vec<Pos2> = c
                .lasso
                .iter()
                .map(|q| {
                    let (x, y) = l.uv_to_canvas(q[0], q[1]);
                    to_screen(x, y)
                })
                .collect();
            painter.add(egui::Shape::line(
                pts,
                Stroke::new(2.0, Color32::from_rgb(255, 80, 120)),
            ));
        }
    }
    if c.tool == CTool::RectMask
        && let (Some(a), Some(b)) = (c.rect_start, pointer)
    {
        painter.rect_stroke(
            Rect::from_two_pos(a, b),
            0.0,
            Stroke::new(1.5, Color32::from_rgb(255, 80, 120)),
            egui::StrokeKind::Middle,
        );
    }
    if c.tool != CTool::Move && sel_idx.is_none() {
        painter.text(
            avail.center_top() + vec2(0.0, 6.0),
            egui::Align2::CENTER_TOP,
            "Select a layer first (Layers panel)",
            egui::FontId::proportional(13.0),
            theme::TEXT,
        );
    }
}

/// Bring to front / forward, send backward / to back (shared by the Layers panel and canvas).
fn stack_menu(ui: &mut Ui) -> Option<Stack> {
    let mut out = None;
    for (s, label) in [
        (Stack::Front, "Bring to front"),
        (Stack::Forward, "Bring forward"),
        (Stack::Backward, "Send backward"),
        (Stack::Back, "Send to back"),
    ] {
        if ui.button(label).clicked() {
            out = Some(s);
        }
    }
    out
}
