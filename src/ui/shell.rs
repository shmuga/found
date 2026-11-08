//! Window chrome: inline title-bar toolbar, sidebar (full / compact), vim-style status line,
//! help overlay and keyboard capture.

use super::theme;
use crate::app::{FoundApp, RenameTarget, Source, View};
use crate::keys::{self, Act, Ctx, Go, K, Mode};
use egui::{Color32, Event, Key, RichText, Sense, Ui, vec2};

pub const TOOLBAR_H: f32 = 38.0;
pub const COMPACT_W: f32 = 52.0;
const ROW_H: f32 = 26.0;
/// Room for the macOS traffic lights when the title bar is transparent.
const TRAFFIC_LIGHTS_W: f32 = 72.0;

enum CmdKey {
    A(Act),
    Prompt(&'static str),
    Search,
}

/// ⌘ shortcuts (work with or without vim keys).
fn cmd_shortcut(key: Key, shift: bool) -> Option<CmdKey> {
    use CmdKey::*;
    Some(match (key, shift) {
        (Key::Z, false) => A(Act::Undo),
        (Key::Z, true) => A(Act::Redo),
        (Key::S, false) => A(Act::RunCommand("w".into())),
        (Key::S, true) => Prompt("saveas "),
        (Key::K, _) | (Key::P, true) => Prompt(""),
        (Key::F, _) => Search,
        (Key::Backslash, _) | (Key::B, _) => A(Act::ToggleSidebar),
        (Key::Num1, _) => A(Act::Go(Go::Library)),
        (Key::Num2, _) => A(Act::Go(Go::Edit)),
        (Key::Num3, _) => A(Act::Go(Go::Collage)),
        (Key::Comma, _) | (Key::Num4, _) => A(Act::Go(Go::Settings)),
        (Key::Equals | Key::Plus, _) => A(Act::ZoomIn),
        (Key::Minus, _) => A(Act::ZoomOut),
        (Key::Num0, _) => A(Act::ZoomFit),
        (Key::Num9, _) => A(Act::Zoom100),
        (Key::O, true) => A(Act::RunCommand("library".into())),
        (Key::I, _) => A(Act::RunCommand("import".into())),
        (Key::N, _) => A(Act::RunCommand("collage".into())),
        (Key::Enter, _) => A(Act::RunCommand("file".into())),
        (Key::OpenBracket, _) => A(Act::ItemStep(-1)),
        (Key::CloseBracket, _) => A(Act::ItemStep(1)),
        (Key::Slash, _) => A(Act::Help),
        (Key::A, _) => A(Act::RunCommand("selectall".into())),
        (Key::L, _) => A(Act::RunCommand("last".into())),
        (Key::Backspace | Key::Delete, _) => A(Act::RunCommand("trash".into())),
        _ => return None,
    })
}

fn letter(key: Key) -> Option<char> {
    let n = key.name();
    (n.len() == 1)
        .then(|| n.chars().next().unwrap().to_ascii_lowercase())
        .filter(|c| c.is_ascii_alphabetic() || *c == '[')
}

impl FoundApp {
    pub fn key_ctx(&self) -> Ctx {
        if self.lib.is_none() {
            return Ctx::Other;
        }
        match self.view {
            View::Library => Ctx::Library,
            View::Edit => match &self.editor {
                Some(ed) if ed.tool == crate::app::Tool::Crop => Ctx::Crop,
                Some(_) => Ctx::Edit,
                None => Ctx::Other,
            },
            View::Collage if self.collage.is_some() => Ctx::Collage,
            _ => Ctx::Other,
        }
    }

    /// Turn this frame's keyboard events into actions (and swallow them so widgets don't also
    /// react). Nothing is captured while a text field has focus.
    pub fn capture_keys(&mut self, ctx: &egui::Context) -> Vec<Act> {
        // Use last frame's focus: egui has already moved focus on Tab by now, which would
        // otherwise steal the key from us.
        if self.text_focus {
            return vec![];
        }
        let vim = self.settings.vim_keys;
        let mut keys: Vec<K> = vec![];
        let mut cmd: Vec<CmdKey> = vec![];
        ctx.input_mut(|i| {
            let held = i.modifiers;
            i.events.retain(|e| match e {
                Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } => {
                    if modifiers.command {
                        if let Some(c) = cmd_shortcut(*key, modifiers.shift) {
                            cmd.push(c);
                            return false;
                        }
                        return true;
                    }
                    if modifiers.ctrl {
                        if let Some(c) = letter(*key) {
                            keys.push(K::Ctrl(c));
                            return false;
                        }
                        return true;
                    }
                    let k = match key {
                        Key::Escape => K::Esc,
                        Key::Enter => K::Enter,
                        Key::Tab if modifiers.shift => K::BackTab,
                        Key::Tab => K::Tab,
                        Key::Backspace => K::Backspace,
                        Key::Delete => K::Delete,
                        Key::ArrowUp => K::Up,
                        Key::ArrowDown => K::Down,
                        Key::ArrowLeft => K::Left,
                        Key::ArrowRight => K::Right,
                        // Printable keys arrive as Text events; swallow the Key so e.g. Space
                        // doesn't press a focused button.
                        _ => return false,
                    };
                    keys.push(k);
                    false
                }
                Event::Text(t) => {
                    if !(held.command || held.ctrl) {
                        keys.extend(t.chars().map(K::Ch));
                    }
                    false
                }
                _ => true,
            });
        });
        // We own Tab / arrows: cancel egui's focus navigation for them.
        if keys.iter().any(|k| {
            matches!(
                k,
                K::Tab | K::BackTab | K::Up | K::Down | K::Left | K::Right
            )
        }) {
            ctx.memory_mut(|m| {
                m.move_focus(egui::FocusDirection::None);
                if let Some(id) = m.focused() {
                    m.surrender_focus(id);
                }
            });
        }
        let kctx = self.key_ctx();
        let mut acts = vec![];
        for c in cmd {
            match c {
                CmdKey::A(a) => acts.push(a),
                CmdKey::Prompt(p) => self.keys.prompt(p),
                CmdKey::Search => {
                    self.keys.prompt("");
                    self.keys.mode = Mode::Search;
                }
            }
        }
        let typing_line = matches!(self.keys.mode, Mode::Command | Mode::Search | Mode::Insert);
        for k in keys {
            // Without vim keys only arrows, Enter, Esc, Tab and a few symbols are live.
            if !vim && !typing_line {
                let simple = match k {
                    K::Ch(c) => "+-=? ".contains(c),
                    K::Ctrl(_) => false,
                    _ => true,
                };
                if !simple {
                    continue;
                }
            }
            acts.extend(self.keys.feed(k, kctx));
        }
        // Live completion hints while typing a command.
        self.completions = if self.keys.mode == Mode::Command {
            self.complete(&self.keys.line.clone()).1
        } else {
            vec![]
        };
        if !acts.is_empty() {
            ctx.request_repaint();
        }
        acts
    }

    // ------------------------------------------------------------- toolbar (title bar)

    pub fn toolbar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        // The bar doubles as the window title bar: drag to move, double-click to zoom.
        let bg = ui.interact(
            ui.max_rect(),
            ui.id().with("titlebar"),
            Sense::click_and_drag(),
        );
        if bg.drag_started_by(egui::PointerButton::Primary) {
            ctx.send_viewport_cmd(egui::ViewportCommand::StartDrag);
        }
        if bg.double_clicked() {
            let max = ctx.input(|i| i.viewport().maximized.unwrap_or(false));
            ctx.send_viewport_cmd(egui::ViewportCommand::Maximized(!max));
        }
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, theme::LINE),
        );
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 4.0;
            if ctx.os() == egui::os::OperatingSystem::Mac {
                ui.add_space(TRAFFIC_LIGHTS_W);
            }
            if self.lib.is_none() {
                ui.label(RichText::new("Found").strong().color(theme::TEXT));
                return;
            }
            let glyph = if self.settings.sidebar_compact {
                "⏵"
            } else {
                "⏴"
            };
            if theme::icon(ui, glyph, "Toggle sidebar (⌘\\ or gb)").clicked() {
                self.act(&ctx, Act::ToggleSidebar);
            }
            ui.add_space(4.0);
            for (v, label, tip) in [
                (View::Library, "Library", "⌘1 · gl"),
                (View::Edit, "Edit", "⌘2 · ge"),
                (View::Collage, "Collage", "⌘3 · gc"),
            ] {
                let enabled = match v {
                    View::Edit => self.editor.is_some(),
                    View::Collage => self.collage.is_some(),
                    _ => true,
                };
                ui.add_enabled_ui(enabled, |ui| {
                    if theme::sel(ui, self.view == v, label)
                        .on_hover_text(tip)
                        .clicked()
                    {
                        self.view = v;
                    }
                });
            }
            ui.add_space(6.0);
            ui.separator();
            ui.add_space(2.0);
            match self.view {
                View::Edit | View::Collage => {}
                View::Library => {
                    if ui
                        .button("Import…")
                        .on_hover_text(
                            "⌘I · copy scans or PDFs into the Inbox (or drop files on the window)",
                        )
                        .clicked()
                    {
                        self.act(&ctx, Act::RunCommand("import".into()));
                    }
                    if ui.button("Reveal Inbox").clicked()
                        && let Some(lib) = &self.lib
                    {
                        let _ = std::process::Command::new("open")
                            .arg(lib.inbox_dir())
                            .spawn();
                    }
                }
                View::Settings => {}
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if theme::sel(ui, self.view == View::Settings, "Settings")
                    .on_hover_text("⌘, · gs")
                    .clicked()
                {
                    self.view = View::Settings;
                }
                if theme::icon(ui, "?", "Keyboard shortcuts (? or ⌘/)").clicked() {
                    self.help = !self.help;
                }
                if self.worker.busy() || self.pdf_busy {
                    ui.spinner();
                }
            });
        });
    }

    // ------------------------------------------------------------- sidebar

    /// One row of a sidebar list: full width, hairline below, blue text when selected.
    fn nav_row(
        ui: &mut Ui,
        selected: bool,
        label: &str,
        count: Option<usize>,
        indent: bool,
    ) -> egui::Response {
        let w = ui.available_width();
        let (rect, resp) = ui.allocate_exact_size(vec2(w, ROW_H), Sense::click());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        let p = ui.painter();
        if selected {
            p.rect_filled(rect, 0.0, theme::LIST_SEL);
        } else if resp.hovered() {
            p.rect_filled(rect, 0.0, theme::LIST_SEP);
        }
        p.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, theme::LIST_SEP),
        );
        let x = if indent { 28.0 } else { 14.0 };
        let font = if selected {
            egui::FontId::new(13.0, egui::FontFamily::Proportional)
        } else {
            egui::FontId::proportional(13.0)
        };
        let fg = if selected {
            theme::SEL_TEXT
        } else {
            theme::TEXT
        };
        let galley = p.layout_no_wrap(label.to_string(), font, fg);
        let clip = egui::Rect::from_min_max(rect.min, egui::pos2(rect.max.x - 30.0, rect.max.y));
        let pos = egui::pos2(rect.min.x + x, rect.center().y - galley.size().y / 2.0);
        p.with_clip_rect(clip).galley(pos, galley.clone(), fg);
        if selected {
            // Faux-bold: the current item is blue; a 1 px double strike makes it read.
            p.with_clip_rect(clip)
                .galley(pos + vec2(0.6, 0.0), galley, fg);
        }
        if let Some(n) = count.filter(|n| *n > 0) {
            p.text(
                rect.right_center() - vec2(12.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                n.to_string(),
                egui::FontId::proportional(11.0),
                theme::TEXT_DIM,
            );
        }
        resp
    }

    /// Group title row (bold, flush left, slightly darker).
    fn group_row(ui: &mut Ui, label: &str) {
        Self::group_row_ex(ui, label, None);
    }

    /// Group title row (bold, flush left). With `Some(selected)` it is
    /// clickable (e.g. "Collections" → all filed photos).
    fn group_row_ex(ui: &mut Ui, label: &str, link: Option<bool>) -> egui::Response {
        let w = ui.available_width();
        let sense = if link.is_some() {
            Sense::click()
        } else {
            Sense::hover()
        };
        let (rect, resp) = ui.allocate_exact_size(vec2(w, ROW_H), sense);
        if link.is_some() {
            resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, label));
        }
        let p = ui.painter();
        let selected = link == Some(true);
        let bg = if selected {
            theme::LIST_SEL
        } else if link.is_some() && resp.hovered() {
            theme::LIST_SEP
        } else {
            theme::SIDEBAR.gamma_multiply(0.97)
        };
        p.rect_filled(rect, 0.0, bg);
        p.hline(
            rect.x_range(),
            rect.bottom() - 0.5,
            egui::Stroke::new(1.0, theme::LIST_SEP),
        );
        let fg = if selected {
            theme::SEL_TEXT
        } else {
            theme::TEXT
        };
        for dx in [14.0, 14.6] {
            p.text(
                rect.left_center() + vec2(dx, 0.0),
                egui::Align2::LEFT_CENTER,
                label,
                egui::FontId::new(13.0, egui::FontFamily::Proportional),
                fg,
            );
        }
        if link.is_some() {
            p.text(
                rect.right_center() - vec2(12.0, 0.0),
                egui::Align2::RIGHT_CENTER,
                "all",
                egui::FontId::proportional(11.0),
                theme::TEXT_DIM,
            );
        }
        resp
    }

    /// Inline rename field in place of a sidebar row. Returns true if it drew the row.
    /// Enter applies; Esc or clicking elsewhere cancels.
    fn rename_row(&mut self, ui: &mut Ui, target: &RenameTarget) -> bool {
        let Some((t, buf)) = self.renaming.as_mut() else {
            return false;
        };
        if t != target {
            return false;
        }
        let mut done: Option<bool> = None;
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            let r = ui.add(
                egui::TextEdit::singleline(buf)
                    .id_salt("sidebar_rename")
                    .desired_width(ui.available_width() - 8.0),
            );
            if !r.has_focus() && !r.lost_focus() {
                r.request_focus();
            }
            if r.lost_focus() {
                done = Some(ui.input(|i| i.key_pressed(Key::Enter)));
            }
        });
        ui.add_space(2.0);
        match done {
            Some(true) => {
                let (t, new) = self.renaming.take().unwrap();
                match t {
                    RenameTarget::Collection(old) => self.rename_collection(&old, &new),
                    RenameTarget::Collage(p) => self.rename_collage(&p, &new),
                }
            }
            Some(false) => self.renaming = None,
            None => {}
        }
        true
    }

    /// A low-key "+ New …" field: frameless, dim hint; the button only appears once you type.
    /// Returns true when Enter or the button submits.
    fn quiet_input(ui: &mut Ui, buf: &mut String, hint: &str, button: &str, salt: &str) -> bool {
        let mut go = false;
        ui.horizontal(|ui| {
            ui.add_space(24.0);
            // egui paints hints in the "weak" colour, which is the sidebar grey: darken it here.
            ui.visuals_mut().weak_text_color = Some(theme::TEXT_DIM);
            let typing = !buf.trim().is_empty();
            let w = ui.available_width()
                - if typing {
                    8.0 + button.len() as f32 * 7.0 + 12.0
                } else {
                    8.0
                };
            let r = ui.add(
                egui::TextEdit::singleline(buf)
                    .id_salt(salt)
                    .frame(egui::Frame::NONE)
                    .text_color(theme::TEXT)
                    .font(egui::FontId::proportional(12.5))
                    .hint_text(RichText::new(hint).size(12.5).color(theme::TEXT_DIM))
                    .desired_width(w.max(40.0)),
            );
            if r.has_focus() || r.hovered() {
                ui.painter().hline(
                    r.rect.x_range(),
                    r.rect.bottom(),
                    egui::Stroke::new(1.0, theme::LINE),
                );
            }
            if r.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter)) {
                go = true;
            }
            if typing && ui.small_button(button).clicked() {
                go = true;
            }
        });
        go && !buf.trim().is_empty()
    }

    pub fn sidebar(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let root = self
            .lib
            .as_ref()
            .map(|l| l.root.clone())
            .unwrap_or_default();
        ui.spacing_mut().item_spacing.y = 0.0;
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.add_space(10.0);
            ui.label(
                RichText::new(
                    root.file_name()
                        .and_then(|n| n.to_str())
                        .unwrap_or("Library"),
                )
                .strong()
                .color(theme::TEXT),
            )
            .on_hover_text(root.display().to_string());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.add_space(4.0);
                if ui
                    .small_button("Switch…")
                    .on_hover_text(format!("{}\nOpen another library (⇧⌘O)", root.display()))
                    .clicked()
                {
                    self.close_library();
                }
            });
        });
        ui.add_space(6.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                let (view, source) = (self.view, self.source.clone());
                let lib_sel = move |s: &Source| view == View::Library && source == *s;
                let inbox_sel = lib_sel(&Source::Inbox);
                Self::group_row(ui, "Scans");
                if Self::nav_row(ui, inbox_sel, "Inbox", Some(self.inbox_count), true).clicked() {
                    self.act(&ctx, Act::Go(Go::Inbox));
                }
                let trash_sel = lib_sel(&Source::Trash);
                let r = Self::nav_row(ui, trash_sel, "Trash", Some(self.trash_count), true)
                    .on_hover_text(
                        "gx · removed photos, restorable · drop photos here to remove them",
                    );
                self.drop_zone(ui, r.rect, crate::app::DropTarget::Trash);
                if r.clicked() {
                    self.act(&ctx, Act::Go(Go::Trash));
                }
                let all_sel = lib_sel(&Source::AllCollections);
                if Self::group_row_ex(ui, "Collections", Some(all_sel)).clicked() {
                    self.show_source(Source::AllCollections);
                }
                for c in self.collections.clone() {
                    let target = RenameTarget::Collection(c.clone());
                    if self.rename_row(ui, &target) {
                        continue;
                    }
                    let sel = lib_sel(&Source::Collection(c.clone()));
                    let r = Self::nav_row(ui, sel, &c, None, true);
                    self.drop_zone(ui, r.rect, crate::app::DropTarget::Collection(c.clone()));
                    if r.double_clicked() {
                        self.renaming = Some((target.clone(), c.clone()));
                    } else if r.clicked() {
                        self.show_source(Source::Collection(c.clone()));
                    }
                    r.context_menu(|ui| {
                        theme::popup(ui);
                        if ui.button("Rename…").clicked() {
                            self.renaming = Some((target.clone(), c.clone()));
                        }
                    });
                }
                let mut buf = std::mem::take(&mut self.new_collection);
                if Self::quiet_input(ui, &mut buf, "+ New collection", "Create", "new_collection")
                    && !buf.trim().is_empty()
                {
                    let name = std::mem::take(&mut buf);
                    self.act(&ctx, Act::RunCommand(format!("new {name}")));
                }
                self.new_collection = buf;
                if !self.tags.is_empty() {
                    Self::group_row(ui, "Tags");
                    for t in self.tags.clone() {
                        let sel = lib_sel(&Source::Tag(t.clone()));
                        if Self::nav_row(ui, sel, &format!("#{t}"), None, true).clicked() {
                            self.show_source(Source::Tag(t));
                        }
                    }
                }
                Self::group_row(ui, "Collages");
                for p in self.collage_files.clone() {
                    let name = p
                        .file_stem()
                        .and_then(|s| s.to_str())
                        .unwrap_or("?")
                        .to_string();
                    let target = RenameTarget::Collage(p.clone());
                    if self.rename_row(ui, &target) {
                        continue;
                    }
                    let sel = self.view == View::Collage
                        && self.collage.as_ref().is_some_and(|c| c.path == p);
                    let r = Self::nav_row(ui, sel, &name, None, true);
                    if r.double_clicked() {
                        self.renaming = Some((target.clone(), name.clone()));
                    } else if r.clicked() {
                        self.open_collage(p.clone());
                    }
                    r.context_menu(|ui| {
                        theme::popup(ui);
                        if ui.button("Open").clicked() {
                            self.open_collage(p.clone());
                        }
                        if ui.button("Rename…").clicked() {
                            self.renaming = Some((target.clone(), name.clone()));
                        }
                        if ui.button("Move collage to trash").clicked() {
                            self.trash_collage(p.clone());
                        }
                    });
                }
                let mut buf = std::mem::take(&mut self.new_collage_name);
                if Self::quiet_input(
                    ui,
                    &mut buf,
                    "+ New collage",
                    "Create collage",
                    "new_collage",
                ) {
                    let name = std::mem::take(&mut buf);
                    self.new_collage_named(Some(&name));
                }
                self.new_collage_name = buf;
            });
    }

    /// Icon rail (compact sidebar): glyph + short label, blue when current.
    pub fn sidebar_compact(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        ui.vertical_centered(|ui| {
            ui.spacing_mut().item_spacing.y = 2.0;
            let tile = |ui: &mut Ui,
                        selected: bool,
                        text: &str,
                        tip: &str,
                        badge: Option<usize>|
             -> egui::Response {
                let (rect, resp) =
                    ui.allocate_exact_size(vec2(COMPACT_W - 8.0, 36.0), Sense::click());
                resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tip));
                let p = ui.painter();
                if selected {
                    p.rect_filled(rect, 4.0, theme::LIST_SEL);
                } else if resp.hovered() {
                    p.rect_filled(rect, 4.0, theme::HOVER);
                }
                let fg = if selected {
                    theme::SEL_TEXT
                } else {
                    theme::TEXT
                };
                p.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    text,
                    egui::FontId::proportional(13.0),
                    fg,
                );
                if let Some(n) = badge.filter(|n| *n > 0) {
                    let c = rect.right_top() + vec2(-6.0, 7.0);
                    p.circle_filled(c, 7.0, theme::ORANGE);
                    p.text(
                        c,
                        egui::Align2::CENTER_CENTER,
                        n.min(99).to_string(),
                        egui::FontId::proportional(9.0),
                        theme::TEXT,
                    );
                }
                resp.on_hover_text(tip)
            };
            ui.add_space(4.0);
            let inbox_sel = self.view == View::Library && self.source == Source::Inbox;
            if tile(ui, inbox_sel, "In", "Inbox", Some(self.inbox_count)).clicked() {
                self.act(&ctx, Act::Go(Go::Inbox));
            }
            ui.add_space(4.0);
            for c in self.collections.clone() {
                let sel =
                    self.view == View::Library && self.source == Source::Collection(c.clone());
                let initials: String = c
                    .chars()
                    .filter(|ch| ch.is_alphanumeric())
                    .take(2)
                    .collect();
                let r = tile(ui, sel, &initials, &c, None);
                self.drop_zone(ui, r.rect, crate::app::DropTarget::Collection(c.clone()));
                if r.clicked() {
                    self.show_source(Source::Collection(c));
                }
            }
            ui.add_space(4.0);
            for p in self.collage_files.clone() {
                let n: String = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .chars()
                    .rev()
                    .take(2)
                    .collect::<Vec<_>>()
                    .into_iter()
                    .rev()
                    .collect();
                let sel = self.view == View::Collage
                    && self.collage.as_ref().is_some_and(|c| c.path == p);
                let tip = p
                    .file_stem()
                    .and_then(|s| s.to_str())
                    .unwrap_or("collage")
                    .to_string();
                let r = tile(ui, sel, &format!("C{n}"), &tip, None);
                if r.clicked() {
                    self.open_collage(p.clone());
                }
                r.context_menu(|ui| {
                    theme::popup(ui);
                    if ui.button("Move collage to trash").clicked() {
                        self.trash_collage(p.clone());
                    }
                });
            }
            ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
                ui.add_space(6.0);
                let trash_sel = self.view == View::Library && self.source == Source::Trash;
                let r = tile(ui, trash_sel, "🗑", "Trash", Some(self.trash_count));
                self.drop_zone(ui, r.rect, crate::app::DropTarget::Trash);
                if r.clicked() {
                    self.act(&ctx, Act::Go(Go::Trash));
                }
            });
        });
    }

    pub fn show_source(&mut self, s: Source) {
        if self.source != s {
            self.cursor = 0;
            self.marks.clear();
            self.visual_anchor = None;
            if self.keys.mode == Mode::Visual {
                self.keys.mode = Mode::Normal;
            }
        }
        self.source = s;
        self.view = View::Library;
        self.refresh();
    }

    // ------------------------------------------------------------- status line

    /// The one bottom bar: mode, the current view's tools, the status / command line, and on
    /// the right save / trash / zoom plus a little context.
    pub fn statusline(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let rect = ui.max_rect();
        ui.painter().hline(
            rect.x_range(),
            rect.top() + 0.5,
            egui::Stroke::new(1.0, theme::LINE),
        );
        let view = if self.lib.is_some() {
            Some(self.view)
        } else {
            None
        };
        ui.horizontal_centered(|ui| {
            ui.spacing_mut().item_spacing.x = 2.0;
            let mode = self.keys.mode;
            if self.settings.vim_keys {
                let (bg, fg) = match mode {
                    Mode::Normal => (theme::ACCENT, Color32::WHITE),
                    Mode::Visual => (theme::ORANGE, theme::TEXT),
                    Mode::Insert => (theme::GREEN, theme::TEXT),
                    Mode::Command | Mode::Search => (theme::BOX, theme::BOX_TEXT),
                };
                // Fixed-size pill (not stretched to the bar height).
                let g = ui.painter().layout_no_wrap(
                    mode.label().to_string(),
                    egui::FontId::monospace(11.0),
                    fg,
                );
                let (r, _) = ui.allocate_exact_size(g.size() + vec2(14.0, 6.0), Sense::hover());
                ui.painter().rect_filled(r, 3.0, bg);
                ui.painter().galley(r.min + vec2(7.0, 3.0), g, fg);
                ui.add_space(10.0);
            } else {
                ui.add_space(4.0);
            }
            match view {
                Some(View::Edit) => {
                    self.edit_tools(ui, &ctx, false);
                    ui.add_space(12.0);
                }
                Some(View::Collage) => {
                    self.collage_tools(ui, &ctx, false);
                    ui.add_space(12.0);
                }
                _ => {}
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.spacing_mut().item_spacing.x = 2.0;
                ui.add_space(8.0);
                let info = self.context_info();
                if !info.is_empty() {
                    ui.label(RichText::new(info).small().color(theme::TEXT_DIM));
                    ui.add_space(10.0);
                }
                let sc = self.keys.showcmd();
                if !sc.is_empty() {
                    ui.label(RichText::new(sc).monospace().color(theme::TEXT));
                    ui.add_space(8.0);
                }
                match view {
                    Some(View::Edit) => self.edit_tools(ui, &ctx, true),
                    Some(View::Collage) => self.collage_tools(ui, &ctx, true),
                    _ => {}
                }
                ui.add_space(10.0);
                // Whatever space is left: status text, or the command line while typing.
                ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                    ui.spacing_mut().item_spacing.x = 8.0;
                    let mono = |s: String| RichText::new(s).monospace().color(theme::TEXT);
                    match mode {
                        Mode::Command => {
                            ui.label(mono(format!(":{}▏", self.keys.line)));
                            if !self.completions.is_empty() {
                                ui.add(
                                    egui::Label::new(
                                        RichText::new(self.completions.join("  "))
                                            .monospace()
                                            .small()
                                            .color(theme::TEXT_DIM),
                                    )
                                    .truncate(),
                                );
                            }
                        }
                        Mode::Search => {
                            ui.label(mono(format!("/{}▏", self.keys.line)));
                        }
                        Mode::Insert => {
                            let what = self.cursor_param_label().unwrap_or_default();
                            ui.label(mono(format!("{what} = {}▏", self.keys.line)));
                        }
                        _ => {
                            ui.add(
                                egui::Label::new(
                                    RichText::new(&self.status).small().color(theme::TEXT),
                                )
                                .truncate(),
                            );
                        }
                    }
                });
            });
        });
    }

    fn context_info(&self) -> String {
        match self.view {
            View::Library => {
                let sel = self.selection().len();
                let mut s = if self.items.is_empty() {
                    "empty".into()
                } else {
                    format!("{}/{}", self.cursor + 1, self.items.len())
                };
                if sel > 1 {
                    s.push_str(&format!(" · {sel} selected"));
                }
                if let Some(q) = &self.search {
                    s.push_str(&format!(" · /{q}"));
                }
                s
            }
            View::Edit => self
                .editor
                .as_ref()
                .map(|ed| {
                    format!(
                        "{}{}×{} preview · {:.0} ms",
                        if ed.dirty() { "[+] " } else { "" },
                        ed.preview_size[0],
                        ed.preview_size[1],
                        ed.render_ms
                    )
                })
                .unwrap_or_default(),
            View::Collage => self
                .collage
                .as_ref()
                .map(|c| format!("{}×{}", c.doc.width, c.doc.height))
                .unwrap_or_default(),
            View::Settings => String::new(),
        }
    }

    // ------------------------------------------------------------- help

    pub fn help_overlay(&mut self, ctx: &egui::Context) {
        if !self.help {
            return;
        }
        let kctx = self.key_ctx();
        let vim = self.settings.vim_keys;
        let mut open = true;
        egui::Window::new("Keyboard")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .open(&mut open)
            .show(ctx, |ui| {
                ui.set_min_width(560.0);
                ui.horizontal_top(|ui| {
                    if vim {
                        ui.vertical(|ui| {
                            theme::heading(ui, "Vim keys (this view)");
                            egui::Grid::new("vimhelp")
                                .num_columns(2)
                                .spacing([14.0, 3.0])
                                .show(ui, |ui| {
                                    for (k, d) in keys::help(kctx) {
                                        ui.label(
                                            RichText::new(k)
                                                .monospace()
                                                .strong()
                                                .color(theme::TEXT),
                                        );
                                        ui.label(d);
                                        ui.end_row();
                                    }
                                });
                        });
                        ui.add_space(16.0);
                    }
                    ui.vertical(|ui| {
                        theme::heading(ui, "Everywhere");
                        egui::Grid::new("cmdhelp")
                            .num_columns(2)
                            .spacing([14.0, 3.0])
                            .show(ui, |ui| {
                                for (k, d) in [
                                    ("⌘K", "command line"),
                                    ("⌘F", "search"),
                                    ("⌘Z / ⇧⌘Z", "undo / redo"),
                                    ("⌘S", "save (photo: + render)"),
                                    ("⇧⌘S", "save a copy into a collection"),
                                    ("⌘1 2 3 ,", "library, edit, collage, settings"),
                                    ("⌘\\", "sidebar"),
                                    ("⌘+ ⌘- ⌘0 ⌘9", "zoom in, out, fit, 100%"),
                                    ("⌘[ ⌘]", "previous / next photo"),
                                    ("⌘↩", "file into collection"),
                                    ("⌘I ⌘N", "import, new collage"),
                                    ("⌘A", "select all"),
                                    ("Arrows ↩ Esc", "move, open, cancel"),
                                    ("pinch / ⌘-scroll", "zoom; scroll or Space-drag pans"),
                                ] {
                                    ui.label(
                                        RichText::new(k).monospace().strong().color(theme::TEXT),
                                    );
                                    ui.label(d);
                                    ui.end_row();
                                }
                            });
                    });
                });
            });
        if !open {
            self.help = false;
        }
    }
}
