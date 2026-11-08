//! Executes keyboard actions and `:` commands.

use crate::app::{CTool, FoundApp, Source, Tool, View};
use crate::graph::{self, Graph};
use crate::keys::{Act, Mode};
use crate::library::{Item, ItemRef};
use crate::ops::{Aspect, Category, Op, Param};
use crate::presets;
use crate::settings::SortKey;
use egui_snarl::NodeId;
use std::path::PathBuf;

/// Editor slider rows: (node, param index), or (node, None) for param-less nodes (Crop).
pub fn rows(g: &Graph) -> Vec<(NodeId, Option<usize>)> {
    let mut ids = graph::chain(g);
    ids.extend(graph::off_chain(g));
    let mut out = vec![];
    for id in ids {
        let Some(n) = g.get_node(id) else { continue };
        if matches!(n.op, Op::Scan | Op::Output) {
            continue;
        }
        let count = n.op.clone().params().len();
        if count == 0 {
            out.push((id, None));
        } else {
            out.extend((0..count).map(|i| (id, Some(i))));
        }
    }
    out
}

fn default_for(op: &Op) -> Op {
    Op::catalog()
        .into_iter()
        .find(|d| d.name() == op.name())
        .unwrap_or_else(|| op.clone())
}

fn adjust(op: &mut Op, idx: usize, steps: i32, coarse: bool) {
    if let Some(p) = op.params().into_iter().nth(idx) {
        match p {
            Param::F { v, min, max, .. } => {
                let st = (max - min) / 200.0 * if coarse { 10.0 } else { 1.0 };
                *v = (((*v + st * steps as f32) / st).round() * st).clamp(min, max);
            }
            Param::I { v, min, max, .. } => {
                *v = (*v + steps * if coarse { 10 } else { 1 }).clamp(min, max)
            }
            Param::B { v, .. } => {
                if steps % 2 != 0 {
                    *v = !*v;
                }
            }
            Param::C { v, options, .. } => {
                *v = (*v as i32 + steps).rem_euclid(options.len() as i32) as usize
            }
        }
    }
}

fn reset_param(op: &mut Op, idx: usize) {
    let mut d = default_for(op);
    let dv = d.params().into_iter().nth(idx).map(|p| match p {
        Param::F { v, .. } => (*v, 0, false, 0),
        Param::I { v, .. } => (0.0, *v, false, 0),
        Param::B { v, .. } => (0.0, 0, *v, 0),
        Param::C { v, .. } => (0.0, 0, false, *v),
    });
    if let (Some((f, i, b, c)), Some(p)) = (dv, op.params().into_iter().nth(idx)) {
        match p {
            Param::F { v, .. } => *v = f,
            Param::I { v, .. } => *v = i,
            Param::B { v, .. } => *v = b,
            Param::C { v, .. } => *v = c,
        }
    }
}

fn set_value(op: &mut Op, idx: usize, s: &str) -> Result<(), String> {
    let s = s.trim();
    let Some(p) = op.params().into_iter().nth(idx) else {
        return Err("no slider here".into());
    };
    match p {
        Param::F { v, min, max, .. } => {
            *v = s
                .parse::<f32>()
                .map_err(|_| format!("not a number: {s}"))?
                .clamp(min, max)
        }
        Param::I { v, min, max, .. } => {
            *v = s
                .parse::<f32>()
                .map_err(|_| format!("not a number: {s}"))?
                .round()
                .clamp(min as f32, max as f32) as i32
        }
        Param::B { v, .. } => {
            *v = match s.to_lowercase().as_str() {
                "1" | "on" | "true" | "yes" | "y" => true,
                "0" | "off" | "false" | "no" | "n" => false,
                _ => return Err(format!("expected on/off: {s}")),
            }
        }
        Param::C { v, options, .. } => {
            if let Ok(n) = s.parse::<usize>() {
                *v = n.saturating_sub(1).min(options.len() - 1);
            } else {
                let i = find_ci(options.iter().copied(), s)
                    .and_then(|m| options.iter().position(|o| *o == m))
                    .ok_or(format!("no option {s}"))?;
                *v = i;
            }
        }
    }
    Ok(())
}

/// Case-insensitive best match: exact, prefix, then substring.
pub fn find_ci<'a>(cands: impl Iterator<Item = &'a str> + Clone, q: &str) -> Option<&'a str> {
    let q = q.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    cands
        .clone()
        .find(|c| c.to_lowercase() == q)
        .or_else(|| cands.clone().find(|c| c.to_lowercase().starts_with(&q)))
        .or_else(|| cands.clone().find(|c| c.to_lowercase().contains(&q)))
}

fn parse_aspect(s: &str) -> Option<Aspect> {
    Some(match s.trim().to_lowercase().as_str() {
        "free" | "f" => Aspect::Free,
        "1:1" | "square" | "s" | "1" => Aspect::Square,
        "3:2" | "w" | "wide" => Aspect::R3x2,
        "2:3" | "t" | "tall" => Aspect::R2x3,
        "orig" | "original" | "o" => Aspect::Original,
        _ => return None,
    })
}

/// Recipe paste: everything from `src`, except the target keeps its own framing.
pub fn with_crop_of(src: &Graph, target: &Graph) -> Graph {
    let mut g = src.clone();
    if let (Some(tc), Some(gc)) = (graph::crop_node(target), graph::crop_node(&g)) {
        g[gc] = target[tc].clone();
    }
    g
}

impl FoundApp {
    pub fn cursor_row(&self) -> Option<(NodeId, Option<usize>)> {
        let ed = self.editor.as_ref()?;
        rows(&ed.doc.graph).get(ed.param_cursor).copied()
    }

    pub fn cursor_param_label(&self) -> Option<String> {
        let ed = self.editor.as_ref()?;
        let (id, p) = self.cursor_row()?;
        let mut op = ed.doc.graph.get_node(id)?.op.clone();
        let name = op.name();
        let label = p.and_then(|i| {
            op.params().into_iter().nth(i).map(|p| match p {
                Param::F { label, .. }
                | Param::I { label, .. }
                | Param::B { label, .. }
                | Param::C { label, .. } => label,
            })
        });
        Some(match label {
            Some(l) => format!("{name} › {l}"),
            None => name.to_string(),
        })
    }

    fn set_cursor_node(&mut self, id: NodeId) {
        if let Some(ed) = &mut self.editor {
            if let Some(i) = rows(&ed.doc.graph).iter().position(|(n, _)| *n == id) {
                ed.param_cursor = i;
            }
            ed.selected = Some(id);
            ed.cursor_moved = true;
        }
    }

    fn clamp_cursor(&mut self) {
        if let Some(ed) = &mut self.editor {
            let n = rows(&ed.doc.graph).len();
            ed.param_cursor = ed.param_cursor.min(n.saturating_sub(1));
            ed.selected = rows(&ed.doc.graph).get(ed.param_cursor).map(|r| r.0);
            ed.cursor_moved = true;
        }
    }

    fn end_visual(&mut self) {
        self.visual_anchor = None;
        if self.keys.mode == Mode::Visual {
            self.keys.mode = Mode::Normal;
        }
    }

    fn selected_items(&self) -> Vec<Item> {
        self.selection()
            .into_iter()
            .filter_map(|i| self.items.get(i).cloned())
            .collect()
    }

    pub fn act(&mut self, ctx: &egui::Context, a: Act) {
        use Act::*;
        let view = self.view;
        match a {
            Undo => self.undo(),
            Redo => self.redo(),
            Help => self.help = !self.help,
            ToggleSidebar => {
                self.settings.sidebar_compact = !self.settings.sidebar_compact;
                self.save_settings();
            }
            Back => {
                if view != View::Library {
                    // Unsaved edits are caught by the leave guard (save / don't save / cancel).
                    self.view = View::Library;
                    self.refresh();
                }
            }
            Escape => {
                if self.confirm.is_some() {
                    self.confirm = None;
                } else if self.help {
                    self.help = false;
                } else if let Some(ed) = self
                    .editor
                    .as_mut()
                    .filter(|e| view == View::Edit && e.tool != Tool::None)
                {
                    ed.tool = Tool::None;
                } else if let Some(c) = self.collage.as_mut().filter(|c| {
                    view == View::Collage && (c.tool != CTool::Move || !c.lasso.is_empty())
                }) {
                    c.tool = CTool::Move;
                    c.lasso.clear();
                } else if self.search.is_some() {
                    self.search = None;
                } else if !self.marks.is_empty() {
                    self.marks.clear();
                }
            }
            Go(g) => match g {
                crate::keys::Go::Inbox => self.show_source(Source::Inbox),
                crate::keys::Go::Library => self.view = View::Library,
                crate::keys::Go::Edit => {
                    if self.editor.is_some() {
                        self.view = View::Edit;
                    }
                }
                crate::keys::Go::Collage => {
                    if self.collage.is_some() {
                        self.view = View::Collage;
                    } else if let Some(p) = self.collage_files.first().cloned() {
                        self.open_collage(p);
                    }
                }
                crate::keys::Go::Settings => self.view = View::Settings,
                crate::keys::Go::Trash => self.show_source(Source::Trash),
            },
            CollectionStep(n) => {
                let mut list = vec![Source::Inbox];
                list.extend(self.collections.iter().cloned().map(Source::Collection));
                let cur = list.iter().position(|s| *s == self.source).unwrap_or(0) as i32;
                let next = (cur + n).rem_euclid(list.len() as i32) as usize;
                self.show_source(list[next].clone());
            }
            ZoomIn | ZoomOut | ZoomFit | Zoom100 => self.zoom(&a),
            RunCommand(line) => self.run_command(ctx, &line),
            Search(q) => {
                self.search = (!q.is_empty()).then(|| q.to_lowercase());
                if view == View::Library && self.search.is_some() {
                    self.jump_match(0, 1);
                }
            }
            SearchNext(n) => self.jump_match(if n > 0 { 1 } else { -1 }, n.signum()),
            Complete(line) => {
                let (line, cands) = self.complete(&line);
                self.keys.line = line;
                self.completions = cands;
            }
            // ---------------- library
            Move { dx, dy } if view == View::Library => {
                let n = self.items.len();
                if n == 0 {
                    return;
                }
                let mut c = (self.cursor as i64 + dx as i64).clamp(0, n as i64 - 1) as usize;
                for _ in 0..dy.unsigned_abs() {
                    c = self.vertical_neighbor(c, dy > 0);
                }
                self.cursor = c;
                self.cursor_moved = true;
            }
            RowStart => self.cursor = self.row_bounds(self.cursor).0,
            RowEnd => self.cursor = self.row_bounds(self.cursor).1,
            First => match view {
                View::Library => {
                    self.cursor = 0;
                    self.cursor_moved = true;
                }
                View::Edit => {
                    if let Some(ed) = &mut self.editor {
                        ed.param_cursor = 0;
                    }
                    self.clamp_cursor();
                }
                _ => {}
            },
            Last => match view {
                View::Library => {
                    self.cursor = self.items.len().saturating_sub(1);
                    self.cursor_moved = true;
                }
                View::Edit => {
                    if let Some(ed) = &mut self.editor {
                        ed.param_cursor = usize::MAX;
                    }
                    self.clamp_cursor();
                }
                _ => {}
            },
            Page(p) => self.act(ctx, Move { dx: 0, dy: p * 2 }),
            Open => {
                if let Some(it) = self.items.get(self.cursor).cloned() {
                    self.end_visual();
                    self.open_item(ctx, it);
                }
            }
            ToggleMark => {
                if let Some(it) = self.items.get(self.cursor) {
                    let p = it.orig.clone();
                    if !self.marks.remove(&p) {
                        self.marks.insert(p);
                    }
                }
            }
            VisualStart => self.visual_anchor = Some(self.cursor),
            VisualEnd => self.visual_anchor = None,
            YankRecipe => {
                let g = match view {
                    View::Edit => self.editor.as_ref().map(|e| e.doc.graph.clone()),
                    _ => self.lib.as_ref().and_then(|l| {
                        self.items
                            .get(self.cursor)
                            .map(|it| l.read_sidecar(it).graph)
                    }),
                };
                if let Some(g) = g {
                    self.yank_recipe = Some(g);
                    self.status = "Recipe yanked — p in the library pastes it onto the selection (crop is kept)".into();
                }
                self.end_visual();
            }
            Yank => match view {
                View::Edit => {
                    if let Some((id, _)) = self.cursor_row()
                        && let Some(n) = self.editor.as_ref().and_then(|e| e.doc.graph.get_node(id))
                    {
                        self.status = format!("Yanked node {}", n.op.name());
                        self.yank_node = Some(n.clone());
                    }
                }
                View::Collage => {
                    if let Some(c) = &self.collage
                        && let Some(l) = c
                            .selected
                            .and_then(|id| c.doc.layers.iter().find(|l| l.id == id))
                    {
                        self.yank_layer = Some(l.clone());
                        self.status = format!("Yanked layer {}", l.name);
                    }
                }
                _ => self.act(ctx, YankRecipe),
            },
            Paste { before } => match view {
                View::Library => self.paste_recipe(ctx),
                View::Edit => self.paste_node(before),
                View::Collage => {
                    if let (Some(c), Some(l)) = (&mut self.collage, self.yank_layer.clone()) {
                        let mut l = l;
                        l.id = c.doc.next_id;
                        c.doc.next_id += 1;
                        l.cx += c.doc.width as f32 * 0.02;
                        l.cy += c.doc.height as f32 * 0.02;
                        l.cell = None;
                        c.selected = Some(l.id);
                        c.doc.layers.push(l);
                    }
                }
                _ => {}
            },
            Delete => match view {
                View::Edit => {
                    if let Some((id, _)) = self.cursor_row()
                        && let Some(ed) = &mut self.editor
                    {
                        self.yank_node = ed.doc.graph.get_node(id).cloned(); // dd yanks, like vim
                        graph::remove_bridging(&mut ed.doc.graph, id);
                        self.clamp_cursor();
                    }
                }
                View::Collage => {
                    if let Some(c) = &mut self.collage
                        && let Some(i) = c
                            .selected
                            .and_then(|id| c.doc.layers.iter().position(|l| l.id == id))
                    {
                        self.yank_layer = Some(c.doc.layers.remove(i));
                        c.selected = c.doc.layers.get(i.saturating_sub(1)).map(|l| l.id);
                    }
                }
                View::Library => {
                    let items = self.selected_items();
                    if self.source == Source::Trash {
                        let keys = FoundApp::trash_keys_of(&items);
                        if !keys.is_empty() {
                            self.confirm = Some(crate::app::Confirm::Purge(keys));
                        }
                    } else {
                        self.trash_items(items);
                    }
                }
                _ => {}
            },
            Restore => {
                if self.view == View::Library && self.source == Source::Trash {
                    let keys = FoundApp::trash_keys_of(&self.selected_items());
                    self.restore_keys(&keys);
                    self.end_visual();
                } else {
                    self.status = "r restores photos in the trash (gx)".into();
                }
            }
            ItemStep(n) => {
                let cur = self
                    .editor
                    .as_ref()
                    .and_then(|e| self.items.iter().position(|i| i.orig == e.item.orig))
                    .unwrap_or(self.cursor) as i64;
                let next = cur + n as i64;
                if next >= 0
                    && let Some(it) = self.items.get(next as usize).cloned()
                {
                    self.cursor = next as usize;
                    self.open_item(ctx, it);
                }
            }
            // ---------------- editor
            ParamStep(n) => {
                if let Some(ed) = &mut self.editor {
                    ed.param_cursor = (ed.param_cursor as i64 + n as i64).max(0) as usize;
                }
                self.clamp_cursor();
            }
            NodeStep(n) => {
                if let Some(ed) = &mut self.editor {
                    let r = rows(&ed.doc.graph);
                    let cur = r.get(ed.param_cursor).map(|x| x.0);
                    let mut nodes: Vec<NodeId> = vec![];
                    for (id, _) in &r {
                        if nodes.last() != Some(id) {
                            nodes.push(*id);
                        }
                    }
                    let ci = cur
                        .and_then(|c| nodes.iter().position(|x| *x == c))
                        .unwrap_or(0) as i64;
                    let ni = (ci + n as i64).clamp(0, nodes.len() as i64 - 1).max(0) as usize;
                    if let Some(id) = nodes.get(ni).copied() {
                        self.set_cursor_node(id);
                    }
                }
            }
            Adjust { steps, coarse } => {
                if let Some(ed) = &mut self.editor
                    && let Some((id, p)) = rows(&ed.doc.graph).get(ed.param_cursor).copied()
                {
                    match p {
                        Some(i) => adjust(&mut ed.doc.graph[id].op, i, steps, coarse),
                        None => {
                            // Crop row: h / l cycles the aspect ratio.
                            let src = ed
                                .crop_src_size
                                .or_else(|| ed.proxy.as_ref().map(|p| (p.w, p.h)));
                            if let Op::Crop(c) = &mut ed.doc.graph[id].op {
                                let all = Aspect::ALL;
                                let k = all.iter().position(|x| *x == c.aspect).unwrap_or(0) as i32;
                                c.aspect = all[(k + steps).rem_euclid(all.len() as i32) as usize];
                                if let Some((w, h)) = src {
                                    let (rw, rh) = c.rotated_size(w, h);
                                    c.apply_aspect(rw, rh);
                                }
                            }
                        }
                    }
                    ed.last_adjust = Some(a.clone());
                }
            }
            Repeat => {
                if let Some(last) = self.editor.as_ref().and_then(|e| e.last_adjust.clone()) {
                    self.act(ctx, last);
                }
            }
            ResetParam => {
                if let Some(ed) = &mut self.editor
                    && let Some((id, Some(i))) = rows(&ed.doc.graph).get(ed.param_cursor).copied()
                {
                    reset_param(&mut ed.doc.graph[id].op, i);
                }
            }
            ResetNode => {
                if let Some(ed) = &mut self.editor
                    && let Some((id, _)) = rows(&ed.doc.graph).get(ed.param_cursor).copied()
                {
                    let d = match &ed.doc.graph[id].op {
                        Op::Crop(_) => Op::Crop(Default::default()),
                        op => default_for(op),
                    };
                    ed.doc.graph[id].op = d;
                }
            }
            SetValue(s) => {
                if let Some(ed) = &mut self.editor
                    && let Some((id, Some(i))) = rows(&ed.doc.graph).get(ed.param_cursor).copied()
                    && let Err(e) = set_value(&mut ed.doc.graph[id].op, i, &s)
                {
                    self.status = e;
                }
            }
            NodeMove(n) => {
                if let Some((id, _)) = self.cursor_row()
                    && let Some(ed) = &mut self.editor
                {
                    for _ in 0..n.unsigned_abs() {
                        graph::move_in_chain(&mut ed.doc.graph, id, n.signum());
                    }
                    self.set_cursor_node(id);
                }
            }
            Bypass => {
                if let Some((id, _)) = self.cursor_row()
                    && let Some(ed) = &mut self.editor
                {
                    let n = &mut ed.doc.graph[id];
                    n.bypass = !n.bypass;
                }
            }
            ToggleCrop => {
                if let Some(ed) = &mut self.editor {
                    ed.tool = if ed.tool == Tool::Crop {
                        Tool::None
                    } else {
                        Tool::Crop
                    };
                    ed.patch = false;
                }
            }
            ToggleCompare => {
                if let Some(ed) = &mut self.editor {
                    ed.compare = !ed.compare;
                }
            }
            TogglePatch => {
                if let Some(ed) = &mut self.editor {
                    ed.patch = !ed.patch;
                }
            }
            AutoWb(white) => {
                let wb = self.editor.as_ref().and_then(|e| {
                    graph::find(&e.doc.graph, |o| matches!(o, Op::WhiteBalance { .. }))
                });
                match wb {
                    Some(id) => self.wb_auto(ctx, id, white),
                    None => self.status = "No White Balance node (o → add White Balance)".into(),
                }
            }
            Eyedropper => {
                if let Some(ed) = &mut self.editor {
                    match graph::find(&ed.doc.graph, |o| matches!(o, Op::WhiteBalance { .. })) {
                        Some(id) => {
                            ed.tool = Tool::Eyedropper(id);
                            ed.patch = false;
                        }
                        None => self.status = "No White Balance node".into(),
                    }
                }
            }
            Randomize => {
                if let Some((id, _)) = self.cursor_row()
                    && let Some(ed) = &mut self.editor
                    && ed.doc.graph[id].op.category() == Category::Experimental
                {
                    ed.roll = ed.roll.wrapping_add(1);
                    let r = ed.roll;
                    ed.doc.graph[id].op.randomize(r);
                }
            }
            Mutate => {
                if let Some(ed) = &mut self.editor {
                    let ids: Vec<NodeId> = ed.doc.graph.node_ids().map(|(i, _)| i).collect();
                    for id in ids {
                        if ed.doc.graph[id].op.category() == Category::Experimental {
                            ed.roll = ed.roll.wrapping_add(1);
                            let r = ed.roll;
                            ed.doc.graph[id].op.randomize(r);
                        }
                    }
                }
            }
            // ---------------- crop tool
            CropMove { dx, dy } => self.with_crop(|c, _| {
                let [x, y, w, h] = c.rect;
                c.rect = [
                    (x + dx as f32 * 0.01).clamp(0.0, 1.0 - w),
                    (y + dy as f32 * 0.01).clamp(0.0, 1.0 - h),
                    w,
                    h,
                ];
            }),
            CropResize(n) => self.with_crop(|c, _| {
                let [x, y, w, h] = c.rect;
                let s = (1.0 + 0.02 * n as f32).max(0.05);
                let s = s.min(1.0 / w).min(1.0 / h);
                let (nw, nh) = (w * s, h * s);
                let (cx, cy) = (x + w * 0.5, y + h * 0.5);
                c.rect = [
                    (cx - nw * 0.5).clamp(0.0, 1.0 - nw),
                    (cy - nh * 0.5).clamp(0.0, 1.0 - nh),
                    nw,
                    nh,
                ];
            }),
            CropAspect(asp) => self.with_crop(|c, size| {
                c.aspect = asp;
                if let Some((w, h)) = size {
                    c.apply_aspect(w, h);
                }
            }),
            CropRotate(n) => self.with_crop(|c, size| {
                c.rot90 = ((c.rot90 as i32 + n).rem_euclid(4)) as u8;
                c.rect = [0.0, 0.0, 1.0, 1.0];
                if n % 2 != 0
                    && let Some((w, h)) = size
                {
                    c.apply_aspect(h, w);
                }
            }),
            CropFlip => self.with_crop(|c, _| c.flip_h = !c.flip_h),
            CropDone => {
                if let Some(ed) = &mut self.editor {
                    ed.tool = Tool::None;
                }
            }
            // ---------------- collage
            LayerStep(n) => {
                if let Some(c) = &mut self.collage
                    && !c.doc.layers.is_empty()
                {
                    let len = c.doc.layers.len() as i32;
                    let cur = c
                        .selected
                        .and_then(|id| c.doc.layers.iter().position(|l| l.id == id))
                        .map(|i| i as i32)
                        .unwrap_or(-1);
                    // Tab walks from the top of the stack down (like the layers list).
                    let next = (cur - n).rem_euclid(len) as usize;
                    c.selected = Some(c.doc.layers[next].id);
                }
            }
            LayerNudge { dx, dy } => self.with_layer(|l, short| {
                l.cx += dx as f32 * short * 0.01;
                l.cy += dy as f32 * short * 0.01;
            }),
            LayerScale(n) => {
                self.with_layer(|l, _| l.width = (l.width * 1.05f32.powi(n)).max(10.0))
            }
            LayerRotate(n) => self.with_layer(|l, _| {
                l.rotation = (l.rotation + 5.0 * n as f32 + 180.0).rem_euclid(360.0) - 180.0
            }),
            LayerOrder(n) => {
                if let Some(c) = &mut self.collage
                    && let Some(i) = c
                        .selected
                        .and_then(|id| c.doc.layers.iter().position(|l| l.id == id))
                {
                    let j = (i as i32 + n).clamp(0, c.doc.layers.len() as i32 - 1) as usize;
                    let l = c.doc.layers.remove(i);
                    c.doc.layers.insert(j, l);
                }
            }
            ToolLasso | ToolRect | ToolMove => {
                if let Some(c) = &mut self.collage {
                    c.tool = match a {
                        ToolLasso => CTool::Lasso,
                        ToolRect => CTool::RectMask,
                        _ => CTool::Move,
                    };
                    c.lasso.clear();
                }
            }
            ClearMask => self.with_layer(|l, _| l.mask = crate::collage::Mask::None),
            FitLayer => {
                if let Some(c) = &mut self.collage
                    && let Some(i) = c
                        .selected
                        .and_then(|id| c.doc.layers.iter().position(|l| l.id == id))
                {
                    let mut l = c.doc.layers[i].clone();
                    c.doc.fit_layer(&mut l);
                    c.doc.layers[i] = l;
                }
            }
            Move { .. } => {}
        }
    }

    fn with_crop(&mut self, f: impl FnOnce(&mut crate::ops::Crop, Option<(usize, usize)>)) {
        let Some(ed) = &mut self.editor else { return };
        let Some(id) = graph::crop_node(&ed.doc.graph) else {
            return;
        };
        let size = ed
            .crop_src_size
            .or_else(|| ed.proxy.as_ref().map(|p| (p.w, p.h)));
        if let Op::Crop(c) = &mut ed.doc.graph[id].op {
            let size = size.map(|(w, h)| {
                if ed.crop_src_size.is_some() {
                    (w, h)
                } else {
                    c.rotated_size(w, h)
                }
            });
            f(c, size);
        }
    }

    fn with_layer(&mut self, f: impl FnOnce(&mut crate::collage::Layer, f32)) {
        let Some(c) = &mut self.collage else { return };
        let short = c.doc.width.min(c.doc.height) as f32;
        if let Some(l) = c
            .selected
            .and_then(|id| c.doc.layers.iter_mut().find(|l| l.id == id))
            && !l.locked
        {
            f(l, short);
        }
    }

    fn zoom(&mut self, a: &Act) {
        let apply = |z: &mut crate::ui::zoom::ZoomPan| match a {
            Act::ZoomIn => z.zoom_in(),
            Act::ZoomOut => z.zoom_out(),
            Act::ZoomFit => z.fit(),
            _ => z.actual(),
        };
        match self.view {
            View::Edit => {
                if let Some(ed) = &mut self.editor {
                    apply(&mut ed.zoom);
                }
            }
            View::Collage => {
                if let Some(c) = &mut self.collage {
                    apply(&mut c.zoom);
                }
            }
            View::Library => {
                use crate::settings::{TILE_MAX, TILE_MIN};
                use crate::ui::library_ui::TILE_DEFAULT;
                let t = self.settings.tile_size;
                self.settings.tile_size = match a {
                    Act::ZoomIn => t * 1.25,
                    Act::ZoomOut => t * 0.8,
                    Act::ZoomFit => TILE_DEFAULT,
                    _ => 360.0,
                }
                .clamp(TILE_MIN, TILE_MAX);
                self.save_settings();
                self.cursor_moved = true;
            }
            View::Settings => {}
        }
    }

    fn jump_match(&mut self, start_offset: i64, dir: i32) {
        let n = self.items.len() as i64;
        if n == 0 || self.search.is_none() {
            return;
        }
        let dir = if dir < 0 { -1 } else { 1 };
        for k in 0..n {
            let i = (self.cursor as i64 + start_offset * dir as i64 + k * dir as i64).rem_euclid(n)
                as usize;
            if self.matches_search(i) {
                self.cursor = i;
                self.cursor_moved = true;
                return;
            }
        }
        self.status = format!(
            "Pattern not found: {}",
            self.search.clone().unwrap_or_default()
        );
    }

    fn paste_node(&mut self, before: bool) {
        let Some(node) = self.yank_node.clone() else {
            self.status = "Nothing yanked (yy on a node first)".into();
            return;
        };
        let Some(ed) = &mut self.editor else { return };
        let g = &mut ed.doc.graph;
        let chain = graph::chain(g);
        let cur = rows(g).get(ed.param_cursor).map(|r| r.0);
        let after = match (cur, before) {
            (Some(c), false) => Some(c),
            (Some(c), true) => graph::input_of(g, c, 0),
            (None, _) => chain.len().checked_sub(2).map(|i| chain[i]),
        };
        if let Some(after) = after {
            let id = graph::insert_after(g, after, node.op.clone());
            g[id].bypass = node.bypass;
            self.set_cursor_node(id);
        }
    }

    fn add_node(&mut self, name: &str, before: bool) {
        let catalog = Op::catalog();
        let names: Vec<&str> = catalog.iter().map(|o| o.name()).collect();
        let Some(found) = find_ci(names.iter().copied(), name) else {
            self.status = format!("No node called {name}");
            return;
        };
        let op = catalog.iter().find(|o| o.name() == found).cloned().unwrap();
        let Some(ed) = &mut self.editor else { return };
        let g = &mut ed.doc.graph;
        let chain = graph::chain(g);
        let cur = rows(g).get(ed.param_cursor).map(|r| r.0);
        let after = match (cur, before) {
            (Some(c), false) => Some(c),
            (Some(c), true) => graph::input_of(g, c, 0),
            (None, _) => chain.len().checked_sub(2).map(|i| chain[i]),
        };
        if let Some(after) = after {
            let id = graph::insert_after(g, after, op);
            self.set_cursor_node(id);
        }
    }

    /// Paste the yanked recipe onto the library selection (each keeps its own crop).
    fn paste_recipe(&mut self, ctx: &egui::Context) {
        let Some(src) = self.yank_recipe.clone() else {
            self.status = "Nothing yanked (yy on a photo, or Y in the editor)".into();
            return;
        };
        self.apply_graph_to_selection(ctx, |target| with_crop_of(&src, target), "Pasted recipe");
    }

    fn apply_graph_to_selection(
        &mut self,
        ctx: &egui::Context,
        f: impl Fn(&Graph) -> Graph,
        what: &str,
    ) {
        let Some(lib) = self.lib.clone() else { return };
        let items = self.selected_items();
        let mut jobs = vec![];
        for it in &items {
            let mut sc = lib.read_sidecar(it);
            sc.graph = f(&sc.graph);
            if lib.write_sidecar(it, &sc).is_ok()
                && let Some(dest) = lib.edit_path(it)
            {
                jobs.push((it.r.clone(), it.orig.clone(), dest, sc.graph.clone()));
            }
            if let Some(ed) = &mut self.editor
                && ed.item.orig == it.orig
            {
                ed.doc.graph = sc.graph.clone();
            }
        }
        self.status = format!("{what} onto {} photo(s)", items.len());
        self.render_batch(ctx, jobs);
        self.end_visual();
        self.marks.clear();
    }

    pub fn render_batch(
        &mut self,
        ctx: &egui::Context,
        jobs: Vec<(ItemRef, PathBuf, PathBuf, Graph)>,
    ) {
        if jobs.is_empty() {
            return;
        }
        let q = self.settings.jpeg_quality.clamp(50, 100);
        self.status = format!("Rendering {} photo(s)…", jobs.len());
        // One job, processed sequentially: full-res renders are memory-heavy.
        self.worker.spawn(ctx, move || {
            crate::app::Msg::Batch(
                jobs.into_iter()
                    .map(|(r, orig, dest, g)| {
                        let res = (|| -> anyhow::Result<PathBuf> {
                            let src = crate::io::load_full(&orig)?;
                            let out = graph::render_full(&g, src)
                                .ok_or_else(|| anyhow::anyhow!("graph has no connected output"))?;
                            crate::io::save_jpeg(&out, &dest, q)?;
                            Ok(dest)
                        })();
                        (r, res.map_err(|e| format!("{e:#}")))
                    })
                    .collect(),
            )
        });
    }

    /// File Inbox scans into `collection` (rendering their edits) and move filed photos there
    /// from other collections. Used by `m` / `:file` and by dragging onto a collection.
    pub fn file_selection(&mut self, ctx: &egui::Context, collection: &str) {
        let items = self.selected_items();
        self.file_items(ctx, items, collection);
    }

    pub fn file_items(&mut self, ctx: &egui::Context, items: Vec<Item>, collection: &str) {
        let Some(lib) = self.lib.clone() else { return };
        let Some(col) =
            find_ci(self.collections.iter().map(|s| s.as_str()), collection).map(String::from)
        else {
            self.status = format!("No collection {collection} (:new {collection} creates it)");
            return;
        };
        let mut jobs = vec![];
        let (mut moved, mut err) = (0, None);
        let mut path_map: Vec<(String, String)> = vec![];
        for it in items {
            match &it.r {
                ItemRef::Inbox(_) => {
                    let sc = lib.read_sidecar(&it);
                    match lib.file_into(&it, &col, &sc) {
                        Ok(new) => {
                            if let Some(dest) = lib.edit_path(&new) {
                                jobs.push((
                                    new.r.clone(),
                                    new.orig.clone(),
                                    dest,
                                    sc.graph.clone(),
                                ));
                            }
                        }
                        Err(e) => err = Some(format!("{e:#}")),
                    }
                }
                ItemRef::Filed {
                    collection: from, ..
                } if *from != col => match lib.move_to(&it, &col) {
                    Ok((new, map)) => {
                        moved += 1;
                        if let Some(ed) = &mut self.editor
                            && ed.item.orig == it.orig
                        {
                            ed.item = new;
                        }
                        path_map.extend(map);
                    }
                    Err(e) => err = Some(format!("{e:#}")),
                },
                _ => {}
            }
        }
        // The open collage (possibly unsaved) follows moved photos too.
        if let Some(c) = &mut self.collage {
            for doc in [&mut c.doc, &mut c.saved] {
                for l in &mut doc.layers {
                    if let Some((_, n)) = path_map.iter().find(|(o, _)| *o == l.source) {
                        l.source = n.clone();
                    }
                }
            }
        }
        let n = jobs.len();
        self.end_visual();
        self.marks.clear();
        self.thumbs.clear();
        self.refresh();
        self.render_batch(ctx, jobs);
        let mut msg = match (n, moved) {
            (0, 0) => "Nothing to move".to_string(),
            (n, 0) => format!("Filed {n} scan(s) into {col}, rendering…"),
            (0, m) => format!("Moved {m} photo(s) to {col}"),
            (n, m) => format!("Filed {n} scan(s) and moved {m} photo(s) into {col}"),
        };
        if let Some(e) = err {
            msg.push_str(&format!(" — {e}"));
        }
        self.status = msg;
    }

    fn tag_selection(&mut self, tags: &[String], add: bool) {
        let Some(lib) = self.lib.clone() else { return };
        let items = self.selected_items();
        for it in &items {
            let mut sc = lib.read_sidecar(it);
            for t in tags {
                if add && !sc.tags.contains(t) {
                    sc.tags.push(t.clone());
                } else if !add {
                    sc.tags.retain(|x| x != t);
                }
            }
            let _ = lib.write_sidecar(it, &sc);
        }
        self.status = format!(
            "{} {} on {} photo(s)",
            if add { "Tagged" } else { "Untagged" },
            tags.join(", "),
            items.len()
        );
        self.end_visual();
        self.refresh();
    }

    /// `:` commands.
    pub fn run_command(&mut self, ctx: &egui::Context, line: &str) {
        let line = line.trim().trim_start_matches(':');
        if let Ok(n) = line.parse::<usize>() {
            // :N jumps to the Nth photo / slider, like a line number.
            match self.view {
                View::Library => {
                    self.cursor = n.saturating_sub(1).min(self.items.len().saturating_sub(1));
                    self.cursor_moved = true;
                }
                View::Edit => {
                    if let Some(ed) = &mut self.editor {
                        ed.param_cursor = n.saturating_sub(1);
                    }
                    self.clamp_cursor();
                }
                _ => {}
            }
            return;
        }
        let (cmd, arg) = line
            .split_once(' ')
            .map(|(a, b)| (a, b.trim()))
            .unwrap_or((line, ""));
        let tags = || -> Vec<String> {
            arg.split([',', ' '])
                .map(|t| t.trim().trim_start_matches('#').to_lowercase())
                .filter(|t| !t.is_empty())
                .collect()
        };
        match cmd {
            "w" | "write" => match self.view {
                View::Edit => self.save_current(ctx),
                View::Collage => {
                    self.save_collage();
                    self.status = "Collage saved (:export renders the JPEG)".into();
                }
                _ => self.status = "Nothing to write here".into(),
            },
            "q" | "quit" | "close" => self.act(ctx, Act::Back),
            "q!" | "quit!" => {
                // vim: leave without saving.
                match self.view {
                    View::Edit => self.discard_editor(),
                    View::Collage => self.discard_collage(),
                    _ => {}
                }
                self.act(ctx, Act::Back);
            }
            "e!" | "revert" => {
                match self.view {
                    View::Edit => self.discard_editor(),
                    View::Collage => self.discard_collage(),
                    _ => {}
                }
                self.status = "Reverted to the last saved version".into();
            }
            "saveas" | "sav" | "copy" => {
                if self.view != View::Edit {
                    self.status =
                        ":saveas works in the editor (saves a copy into a collection)".into();
                } else if arg.is_empty() {
                    self.status = "Usage: :saveas Collection".into();
                } else {
                    self.save_copy(ctx, arg);
                }
            }
            "wq" | "x" => {
                self.run_command(ctx, "w");
                self.act(ctx, Act::Back);
            }
            "tag" | "untag" => {
                let t = tags();
                if t.is_empty() {
                    self.status = format!("Usage: :{cmd} name [name…]");
                    return;
                }
                let add = cmd == "tag";
                match self.view {
                    View::Edit => {
                        if let Some(ed) = &mut self.editor {
                            for x in t {
                                if add && !ed.doc.tags.contains(&x) {
                                    ed.doc.tags.push(x);
                                } else if !add {
                                    ed.doc.tags.retain(|y| *y != x);
                                }
                            }
                        }
                    }
                    _ => self.tag_selection(&t, add),
                }
            }
            "file" | "f" | "mv" => {
                let target = if arg.is_empty() {
                    self.editor
                        .as_ref()
                        .map(|e| e.file_target.clone())
                        .unwrap_or_default()
                } else {
                    arg.to_string()
                };
                if target.is_empty() {
                    self.status = "Usage: :file Collection".into();
                    return;
                }
                if self.view == View::Edit {
                    let Some(col) = find_ci(self.collections.iter().map(|s| s.as_str()), &target)
                        .map(String::from)
                    else {
                        self.status = format!("No collection {target}");
                        return;
                    };
                    self.file_current(ctx, &col);
                    if let Some(next) = self.next_inbox_item() {
                        self.open_item(ctx, next);
                    }
                } else {
                    self.file_selection(ctx, &target);
                }
            }
            "rename" | "mvcol" if self.view == View::Collage => {
                if let Some(p) = self.collage.as_ref().map(|c| c.path.clone()) {
                    if arg.is_empty() {
                        self.status = "Usage: :rename New name".into();
                    } else {
                        self.rename_collage(&p, arg);
                    }
                }
            }
            "rename" | "mvcol" => {
                // :rename New — renames the collection you're looking at (or the open photo's).
                let current = match (
                    &self.source,
                    self.view,
                    self.editor.as_ref().map(|e| &e.item.r),
                ) {
                    (_, View::Edit, Some(ItemRef::Filed { collection, .. })) => {
                        Some(collection.clone())
                    }
                    (Source::Collection(c), _, _) => Some(c.clone()),
                    _ => None,
                };
                match current {
                    Some(old) if !arg.is_empty() => self.rename_collection(&old, arg),
                    Some(old) => self.keys.prompt(&format!("rename {old}")),
                    None => self.status = "Open a collection first, then :rename NewName".into(),
                }
            }
            "new" | "mkcol" => {
                if let Some(lib) = &self.lib {
                    match lib.create_collection(arg) {
                        Ok(n) => {
                            self.status = format!("Created collection {n}");
                            if let Some(ed) = &mut self.editor
                                && ed.file_target.is_empty()
                            {
                                ed.file_target = n;
                            }
                        }
                        Err(e) => self.status = format!("{e:#}"),
                    }
                    self.refresh();
                }
            }
            "add" | "addb" => {
                if self.view != View::Edit {
                    self.status = ":add works in the editor".into();
                } else {
                    self.add_node(arg, cmd == "addb");
                }
            }
            "preset" | "pr" => {
                let names: Vec<String> = self.presets.iter().map(|p| p.name.clone()).collect();
                let Some(name) = find_ci(names.iter().map(|s| s.as_str()), arg).map(String::from)
                else {
                    self.status = format!("No preset {arg}");
                    return;
                };
                let preset = self
                    .presets
                    .iter()
                    .find(|p| p.name == name)
                    .cloned()
                    .unwrap();
                if self.view == View::Edit {
                    if let Some(ed) = &mut self.editor {
                        ed.doc.graph = presets::apply(&ed.doc.graph, &preset.graph);
                    }
                    self.clamp_cursor();
                    self.status = format!("Applied preset {name}");
                    self.remember_recipe(name.clone(), preset.graph.clone());
                } else {
                    self.apply_graph_to_selection(
                        ctx,
                        |g| presets::apply(g, &preset.graph),
                        &format!("Applied {name}"),
                    );
                }
            }
            "savepreset" | "sp" => {
                if let (Some(lib), Some(ed)) = (&self.lib, &self.editor)
                    && !arg.is_empty()
                {
                    self.status = match presets::save_user(&lib.presets_dir(), arg, &ed.doc.graph) {
                        Ok(_) => format!("Saved preset {arg}"),
                        Err(e) => format!("{e:#}"),
                    };
                    self.refresh();
                } else {
                    self.status = "Usage (in the editor): :savepreset Name".into();
                }
            }
            "crop" => match parse_aspect(arg) {
                Some(a) => {
                    self.act(ctx, Act::CropAspect(a));
                    if let Some(ed) = &mut self.editor {
                        ed.tool = Tool::Crop;
                    }
                }
                None => self.status = "Usage: :crop 1:1|3:2|2:3|free|orig".into(),
            },
            "sort" => {
                for w in arg.split_whitespace() {
                    match w {
                        "date" | "d" => self.settings.sort = SortKey::Date,
                        "name" | "n" => self.settings.sort = SortKey::Name,
                        "asc" | "a" | "old" => self.settings.sort_desc = false,
                        "desc" | "new" => self.settings.sort_desc = true,
                        "!" | "rev" | "reverse" => {
                            self.settings.sort_desc = !self.settings.sort_desc
                        }
                        _ => {}
                    }
                }
                self.save_settings();
                self.refresh();
            }
            "group" => {
                use crate::settings::GroupBy;
                let g = match arg {
                    "day" | "d" | "on" => GroupBy::Day,
                    "month" | "m" => GroupBy::Month,
                    "collection" | "col" | "c" => GroupBy::Collection,
                    "none" | "off" | "no" => GroupBy::None,
                    // Bare :group toggles grouping on / off.
                    _ if self.group_by() == GroupBy::None => GroupBy::Day,
                    _ => GroupBy::None,
                };
                self.set_group_by(g);
            }
            "layout" | "view" => {
                use crate::settings::GridLayout;
                let l = match arg {
                    "grid" | "g" => Some(GridLayout::Grid),
                    "rows" | "r" | "justified" => Some(GridLayout::Rows),
                    "list" | "l" => Some(GridLayout::List),
                    _ => None,
                };
                match l {
                    Some(l) => {
                        self.settings.grid_layout = l;
                        self.save_settings();
                    }
                    None => self.status = "Usage: :layout grid|rows|list".into(),
                }
            }
            "collections" => self.show_source(Source::AllCollections),
            "last" | "applylast" => {
                if self.view == View::Edit {
                    self.apply_last();
                    self.clamp_cursor();
                } else {
                    self.status = ":last works in the editor".into();
                }
            }
            "collage" => self.new_collage_named((!arg.is_empty()).then_some(arg)),
            "export" => {
                if self.collage.is_some() {
                    self.export_collage(ctx);
                }
            }
            "inbox" => self.show_source(Source::Inbox),
            "settings" | "set" if arg.is_empty() => self.view = View::Settings,
            "set" => {
                for w in arg.split_whitespace() {
                    match w {
                        "vim" => self.settings.vim_keys = true,
                        "novim" => self.settings.vim_keys = false,
                        "compact" => self.settings.sidebar_compact = true,
                        "nocompact" => self.settings.sidebar_compact = false,
                        _ => self.status = format!("Unknown option: {w}"),
                    }
                }
                self.save_settings();
            }
            "library" | "lib" => self.close_library(),
            "noh" | "nohlsearch" => self.search = None,
            "help" | "h" => self.help = true,
            "yank" => self.act(ctx, Act::YankRecipe),
            "paste" => self.act(ctx, Act::Paste { before: false }),
            "import" => {
                if let Some(files) = rfd::FileDialog::new()
                    .add_filter("Scans", &["jpg", "jpeg", "png", "tif", "tiff", "pdf"])
                    .pick_files()
                {
                    self.import(files);
                }
            }
            "trash" | "rm" | "del" => {
                if self.view == View::Collage
                    && let Some(p) = self.collage.as_ref().map(|c| c.path.clone())
                {
                    self.trash_collage(p);
                } else if self.view == View::Library {
                    self.act(ctx, Act::Delete);
                } else if self.view == View::Edit
                    && let Some(it) = self.editor.as_ref().map(|e| e.item.clone())
                {
                    // Trash the photo being edited and move on to the next one (like filing).
                    let idx = self.items.iter().position(|i| i.orig == it.orig);
                    self.trash_items(vec![it]);
                    match idx.and_then(|i| {
                        self.items
                            .get(i.min(self.items.len().saturating_sub(1)))
                            .cloned()
                    }) {
                        Some(next) => self.open_item(ctx, next),
                        None => self.view = View::Library,
                    }
                }
            }
            "restore" => {
                if self.source == Source::Trash {
                    self.view = View::Library;
                    self.act(ctx, Act::Restore);
                } else {
                    self.restore_last();
                }
            }
            "emptytrash" => self.confirm = Some(crate::app::Confirm::EmptyTrash),
            "selectall" | "all" => {
                if self.view == View::Library {
                    self.marks = self.items.iter().map(|i| i.orig.clone()).collect();
                }
            }
            _ => self.status = format!("E492: Not an editor command: {line}"),
        }
        if self.keys.mode == Mode::Visual
            && !matches!(cmd, "sort" | "group" | "noh" | "nohlsearch" | "help" | "h")
        {
            self.end_visual();
        }
    }

    /// Tab completion: returns the new line and the candidates.
    pub fn complete(&self, line: &str) -> (String, Vec<String>) {
        let (cmd, arg) = match line.split_once(' ') {
            Some((c, a)) => (c, Some(a)),
            None => (line, None),
        };
        let cands: Vec<String> = match arg {
            None => crate::keys::COMMANDS
                .iter()
                .map(|s| s.to_string())
                .collect(),
            Some(_) => match cmd {
                "add" | "addb" => Op::catalog().iter().map(|o| o.name().to_string()).collect(),
                "preset" | "pr" => self.presets.iter().map(|p| p.name.clone()).collect(),
                "file" | "f" | "mv" | "saveas" | "sav" | "copy" => self.collections.clone(),
                "tag" | "untag" => self.tags.clone(),
                "sort" => ["date", "name", "asc", "desc"].map(String::from).to_vec(),
                "group" => ["day", "month", "collection", "none"]
                    .map(String::from)
                    .to_vec(),
                "layout" | "view" => ["grid", "rows", "list"].map(String::from).to_vec(),
                "crop" => ["1:1", "3:2", "2:3", "free", "orig"]
                    .map(String::from)
                    .to_vec(),
                "set" => ["vim", "novim", "compact", "nocompact"]
                    .map(String::from)
                    .to_vec(),
                _ => vec![],
            },
        };
        let typed = arg.unwrap_or(cmd).to_lowercase();
        let mut m: Vec<String> = cands
            .iter()
            .filter(|c| c.to_lowercase().starts_with(&typed))
            .cloned()
            .collect();
        if m.is_empty() && !typed.is_empty() {
            m = cands
                .iter()
                .filter(|c| c.to_lowercase().contains(&typed))
                .cloned()
                .collect();
        }
        m.truncate(10);
        let completed = match m.as_slice() {
            [one] => Some(one.clone()),
            [first, rest @ ..] => {
                let mut p = first.clone();
                for r in rest {
                    while !r.to_lowercase().starts_with(&p.to_lowercase()) {
                        p.pop();
                    }
                }
                (p.len() > typed.len()).then_some(p)
            }
            [] => None,
        };
        let line = match (completed, arg) {
            (Some(c), None) => format!("{c} "),
            (Some(c), Some(_)) => format!("{cmd} {c}"),
            (None, _) => line.to_string(),
        };
        (line, m)
    }
}
