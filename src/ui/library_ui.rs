//! Library grid: sort / group by day, keyboard cursor, marks, visual selection, search.

use super::theme;
use crate::app::{FoundApp, Source};
use crate::library::Item;
use crate::settings::{GridLayout, GroupBy, Settings, SortKey};
use chrono::{Datelike, Local, TimeZone};
use egui::{Color32, Rect, RichText, Sense, Ui, pos2, vec2};

pub const TILE_DEFAULT: f32 = 168.0;
const CAPTION_H: f32 = 20.0;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GridAct {
    Open,
    Reveal,
    Trash,
    Restore,
    Purge,
    EmptyTrash,
}

#[derive(Default)]
struct GridOut {
    rects: Vec<(usize, Rect)>,
    clicked: Option<(usize, egui::Modifiers, bool)>,
    menu_act: Option<(GridAct, usize)>,
    drag_from: Option<usize>,
}

#[derive(Clone, Copy)]
struct GridCell {
    i: usize,
    size: egui::Vec2,
    layout: GridLayout,
    in_trash: bool,
    show_collection: bool,
    scroll_to: bool,
    row_even: bool,
}

/// Row height of the List layout for a given size setting.
fn list_row_h(tile: f32) -> f32 {
    (tile * 0.3).clamp(30.0, 110.0)
}

/// Lay out one group: rows of (index within group, cell size).
/// - Grid: equal square tiles, as many per row as fit.
/// - Rows: justified — every photo keeps its aspect ratio, each full row is scaled to fill the
///   width exactly (the last row keeps the target height).
/// - List: one full-width line per photo.
pub fn layout_rows(
    layout: GridLayout,
    aspects: &[f32],
    avail: f32,
    tile: f32,
    gap: f32,
) -> Vec<Vec<(usize, egui::Vec2)>> {
    let cap = CAPTION_H + 6.0;
    let avail = avail.max(tile.min(200.0));
    match layout {
        GridLayout::List => (0..aspects.len())
            .map(|k| vec![(k, vec2(avail, list_row_h(tile)))])
            .collect(),
        GridLayout::Grid => {
            // As many columns as fit at the requested size, then widen the tiles so the row
            // fills the width exactly (no dead strip on the right).
            let per = (((avail + gap) / (tile + gap)).floor() as usize).max(1);
            let w = ((avail - gap * (per - 1) as f32) / per as f32).floor();
            (0..aspects.len())
                .collect::<Vec<_>>()
                .chunks(per)
                .map(|c| c.iter().map(|&k| (k, vec2(w, w + cap))).collect())
                .collect()
        }
        GridLayout::Rows => {
            let h = tile;
            let mut rows = vec![];
            let mut cur: Vec<(usize, f32)> = vec![];
            let mut sum = 0.0;
            for (k, a) in aspects.iter().enumerate() {
                cur.push((k, *a));
                sum += h * a;
                let gaps = gap * (cur.len() - 1) as f32;
                if sum + gaps >= avail {
                    let scale = (avail - gaps) / sum;
                    rows.push(
                        cur.iter()
                            .map(|(k, a)| {
                                (*k, vec2((h * a * scale).floor(), (h * scale).floor() + cap))
                            })
                            .collect(),
                    );
                    cur.clear();
                    sum = 0.0;
                }
            }
            if !cur.is_empty() {
                rows.push(
                    cur.iter()
                        .map(|(k, a)| (*k, vec2((h * a).floor(), h + cap)))
                        .collect(),
                );
            }
            rows
        }
    }
}

/// List layout: striped rows (console-style) with a small thumbnail and details.
fn paint_list_row(
    p: &egui::Painter,
    rect: Rect,
    t: TileLook,
    collection: &str,
    tags: &str,
    even: bool,
) {
    let bg = if t.selected {
        theme::ACCENT_SOFT
    } else if t.hovered {
        Color32::from_rgb(210, 213, 218)
    } else if even {
        theme::CANVAS
    } else {
        Color32::from_rgb(188, 191, 196)
    };
    p.rect_filled(rect, 0.0, bg);
    let h = rect.height();
    let thumb = Rect::from_min_size(rect.min + vec2(6.0, 3.0), vec2(h * 1.4, h - 6.0));
    match t.tex {
        Some((id, size)) => {
            let s = (thumb.width() / size.x).min(thumb.height() / size.y);
            let r = Rect::from_center_size(thumb.center(), size * s);
            let tint = if t.dim {
                Color32::from_white_alpha(70)
            } else {
                Color32::WHITE
            };
            p.image(
                id,
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                tint,
            );
            p.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.0, Color32::from_rgb(70, 72, 76)),
                egui::StrokeKind::Outside,
            );
        }
        None => {
            p.rect_filled(thumb.shrink(4.0), 2.0, theme::PANEL);
        }
    }
    let text = if t.dim { theme::TEXT_DIM } else { theme::TEXT };
    let x0 = thumb.max.x + 12.0;
    let w = rect.max.x - x0;
    // Columns: name | date | collection | tags | status.
    let cols = [(0.0, 0.34), (0.34, 0.22), (0.56, 0.16), (0.72, 0.18)];
    let font = egui::FontId::proportional(12.5);
    for (k, txt) in [t.title, t.when, collection, tags].iter().enumerate() {
        if txt.is_empty() {
            continue;
        }
        let (a, b) = cols[k];
        let clip = Rect::from_min_max(
            pos2(x0 + w * a, rect.min.y),
            pos2(x0 + w * (a + b) - 8.0, rect.max.y),
        );
        let col = if k == 0 { text } else { theme::TEXT_DIM };
        let g = p.layout_no_wrap(txt.to_string(), font.clone(), col);
        let y = rect.center().y - g.size().y / 2.0;
        p.with_clip_rect(clip).galley(pos2(clip.min.x, y), g, col);
    }
    if t.edited {
        let c = pos2(rect.max.x - 60.0, rect.center().y);
        p.circle_filled(c, 4.0, theme::GREEN);
        p.text(
            c + vec2(8.0, 0.0),
            egui::Align2::LEFT_CENTER,
            "edited",
            egui::FontId::proportional(11.0),
            theme::TEXT_DIM,
        );
    }
    if t.cursor {
        p.rect_stroke(
            rect,
            2.0,
            egui::Stroke::new(2.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
}

struct TileLook<'a> {
    tex: Option<(egui::TextureId, egui::Vec2)>,
    title: &'a str,
    when: &'a str,
    edited: bool,
    cursor: bool,
    selected: bool,
    hovered: bool,
    dim: bool,
}

/// A photo as a patch object: the image in a thin dark frame above a dark
/// object box with the name; the box's inlet bar is green once an edit has been rendered.
fn paint_tile(p: &egui::Painter, rect: Rect, t: TileLook) {
    let bg = if t.selected {
        Some(theme::ACCENT_SOFT)
    } else if t.hovered {
        Some(Color32::from_rgb(210, 213, 218))
    } else {
        None
    };
    if let Some(bg) = bg {
        p.rect_filled(rect.expand(4.0), 5.0, bg);
    }
    let w = rect.width();
    let img_rect = Rect::from_min_max(rect.min, pos2(rect.max.x, rect.max.y - CAPTION_H - 6.0));
    let tint = if t.dim {
        Color32::from_white_alpha(70)
    } else {
        Color32::WHITE
    };
    match t.tex {
        Some((id, size)) => {
            let s = (img_rect.width() / size.x).min(img_rect.height() / size.y);
            let r = Rect::from_center_size(img_rect.center(), size * s);
            p.image(
                id,
                r,
                Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
                tint,
            );
            p.rect_stroke(
                r,
                0.0,
                egui::Stroke::new(1.0, Color32::from_rgb(70, 72, 76)),
                egui::StrokeKind::Outside,
            );
        }
        None => {
            p.rect_filled(
                img_rect.shrink(img_rect.width().min(img_rect.height()) * 0.2),
                3.0,
                theme::PANEL,
            );
        }
    }
    let cap = Rect::from_min_size(pos2(rect.min.x, rect.max.y - CAPTION_H), vec2(w, CAPTION_H));
    p.rect_filled(cap, 3.0, if t.dim { theme::HOVER } else { theme::BOX });
    theme::object_bars(
        p,
        cap,
        if t.edited {
            theme::GREEN
        } else {
            Color32::from_rgb(132, 134, 138)
        },
    );
    let font = egui::FontId::proportional(11.5);
    let when = p.layout_no_wrap(
        t.when.to_string(),
        egui::FontId::proportional(10.0),
        Color32::from_rgb(190, 192, 196),
    );
    let when_w = if w > 130.0 { when.size().x + 8.0 } else { 0.0 };
    let caption = p.layout_no_wrap(t.title.to_string(), font, theme::BOX_TEXT);
    let clip = Rect::from_min_max(
        cap.min + vec2(6.0, 0.0),
        pos2(cap.max.x - 6.0 - when_w, cap.max.y),
    );
    p.with_clip_rect(clip).galley(
        pos2(
            cap.min.x + 6.0,
            cap.center().y - caption.size().y / 2.0 + 1.0,
        ),
        caption,
        theme::BOX_TEXT,
    );
    if when_w > 0.0 {
        p.galley(
            pos2(
                cap.max.x - 6.0 - when.size().x,
                cap.center().y - when.size().y / 2.0 + 1.0,
            ),
            when,
            theme::BOX_TEXT,
        );
    }
    if t.cursor {
        p.rect_stroke(
            rect.expand(4.0),
            5.0,
            egui::Stroke::new(2.0, theme::ACCENT),
            egui::StrokeKind::Inside,
        );
    }
    if t.selected {
        let c = img_rect.right_top() + vec2(-12.0, 12.0);
        p.circle_filled(c, 8.0, theme::ACCENT);
        p.circle_stroke(c, 8.0, egui::Stroke::new(1.5, Color32::WHITE));
        p.text(
            c,
            egui::Align2::CENTER_CENTER,
            "✔",
            egui::FontId::proportional(10.0),
            Color32::WHITE,
        );
    }
}

pub fn sort_items(items: &mut [Item], s: &Settings) {
    match s.sort {
        SortKey::Date => items.sort_by(|a, b| {
            a.modified
                .cmp(&b.modified)
                .then_with(|| a.orig.cmp(&b.orig))
        }),
        SortKey::Name => items.sort_by_key(|a| natural_key(&a.title())),
    }
    if s.sort_desc {
        items.reverse();
    }
}

/// Case-insensitive, with digit runs compared numerically ("scan2" < "scan10").
fn natural_key(s: &str) -> Vec<(u8, u64, String)> {
    let mut out = vec![];
    let mut chars = s.chars().peekable();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            let mut n = String::new();
            while let Some(&d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                n.push(d);
                chars.next();
            }
            out.push((0, n.parse().unwrap_or(u64::MAX), String::new()));
        } else {
            let mut t = String::new();
            while let Some(&d) = chars.peek().filter(|d| !d.is_ascii_digit()) {
                t.push(d.to_ascii_lowercase());
                chars.next();
            }
            out.push((1, 0, t));
        }
    }
    out
}

pub fn day_label(ts: i64, today: chrono::NaiveDate) -> String {
    let Some(d) = Local.timestamp_opt(ts, 0).single().map(|t| t.date_naive()) else {
        return "Unknown date".into();
    };
    let days = (today - d).num_days();
    match days {
        0 => "Today".into(),
        1 => "Yesterday".into(),
        2..=6 => d.format("%A").to_string(),
        _ if d.year() == today.year() => d.format("%a %-d %b").to_string(),
        _ => d.format("%-d %b %Y").to_string(),
    }
}

/// Which collection a photo belongs to (group label).
pub fn collection_of(it: &Item) -> String {
    match &it.r {
        crate::library::ItemRef::Filed { collection, .. } => collection.clone(),
        crate::library::ItemRef::Inbox(_) => "Inbox".into(),
        crate::library::ItemRef::Trashed { .. } => "Trash".into(),
    }
}

/// Contiguous groups (label, first index, count). Grouping by collection re-orders the items
/// (stable, so the sort order holds inside each collection); day / month grouping only
/// applies when sorted by date.
pub fn make_groups(items: &mut [Item], by: GroupBy, s: &Settings) -> Vec<(String, usize, usize)> {
    if items.is_empty() {
        return vec![];
    }
    let today = Local::now().date_naive();
    let label: Box<dyn Fn(&Item) -> String> = match by {
        GroupBy::None => return vec![(String::new(), 0, items.len())],
        GroupBy::Day | GroupBy::Month if s.sort != SortKey::Date => {
            return vec![(String::new(), 0, items.len())];
        }
        GroupBy::Day => Box::new(move |it: &Item| day_label(it.modified, today)),
        GroupBy::Month => Box::new(move |it: &Item| {
            Local
                .timestamp_opt(it.modified, 0)
                .single()
                .map(|t| {
                    if t.year() == today.year() && t.month() == today.month() {
                        "This month".into()
                    } else {
                        t.format("%B %Y").to_string()
                    }
                })
                .unwrap_or_else(|| "Unknown date".into())
        }),
        GroupBy::Collection => {
            items.sort_by_cached_key(|it| natural_key(&collection_of(it)));
            Box::new(collection_of)
        }
    };
    let mut out: Vec<(String, usize, usize)> = vec![];
    for (i, it) in items.iter().enumerate() {
        let l = label(it);
        match out.last_mut() {
            Some((x, _, n)) if *x == l => *n += 1,
            _ => out.push((l, i, 1)),
        }
    }
    out
}

impl FoundApp {
    /// Items the next operator applies to: visual range ∪ marks, else the cursor item.
    pub fn selection(&self) -> Vec<usize> {
        let mut set: std::collections::BTreeSet<usize> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, it)| self.marks.contains(&it.orig))
            .map(|(i, _)| i)
            .collect();
        if let Some(a) = self.visual_anchor {
            let (lo, hi) = (a.min(self.cursor), a.max(self.cursor));
            set.extend(lo..=hi.min(self.items.len().saturating_sub(1)));
        }
        if set.is_empty() && !self.items.is_empty() {
            set.insert(self.cursor.min(self.items.len() - 1));
        }
        set.into_iter().collect()
    }

    pub fn matches_search(&self, i: usize) -> bool {
        let Some(q) = &self.search else { return true };
        let it = &self.items[i];
        it.title().to_lowercase().contains(q)
            || it
                .orig
                .file_name()
                .is_some_and(|n| n.to_string_lossy().to_lowercase().contains(q))
            || self
                .item_tags
                .get(i)
                .is_some_and(|t| t.iter().any(|t| t.contains(q.as_str())))
    }

    pub fn library_grid(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        let in_trash = self.source == Source::Trash;
        let title = match &self.source {
            Source::Inbox => "Inbox".to_string(),
            Source::Collection(c) => c.clone(),
            Source::Tag(t) => format!("#{t}"),
            Source::Trash => "Trash".to_string(),
            Source::AllCollections => "All collections".to_string(),
        };
        let n_sel = if self.items.is_empty() {
            0
        } else {
            self.selection().len()
        };
        let mut header_act: Option<GridAct> = None;
        ui.horizontal(|ui| {
            ui.heading(RichText::new(&title).strong().color(theme::TEXT));
            ui.label(RichText::new(format!("{}", self.items.len())).color(theme::TEXT_DIM));
            ui.add_space(10.0);
            if let Source::Collection(c) = &self.source
                && ui
                    .small_button("Rename…")
                    .on_hover_text(":rename · or double-click it in the sidebar")
                    .clicked()
            {
                self.renaming = Some((crate::app::RenameTarget::Collection(c.clone()), c.clone()));
                if self.settings.sidebar_compact {
                    self.settings.sidebar_compact = false;
                    self.save_settings();
                }
            }
            if in_trash {
                ui.add_enabled_ui(n_sel > 0, |ui| {
                    if ui
                        .button(format!("Restore {n_sel}"))
                        .on_hover_text("r · :restore — put back where it came from")
                        .clicked()
                    {
                        header_act = Some(GridAct::Restore);
                    }
                    if ui
                        .button("Delete forever…")
                        .on_hover_text("Backspace · permanently delete the selection")
                        .clicked()
                    {
                        header_act = Some(GridAct::Purge);
                    }
                });
                ui.add_enabled_ui(!self.items.is_empty(), |ui| {
                    if ui
                        .button("Empty trash…")
                        .on_hover_text(":emptytrash")
                        .clicked()
                    {
                        header_act = Some(GridAct::EmptyTrash);
                    }
                });
            } else if !self.items.is_empty()
                && ui
                    .button(if n_sel > 1 {
                        format!("Move {n_sel} to trash")
                    } else {
                        "Move to trash".into()
                    })
                    .on_hover_text("Backspace · dd · x — restorable from Trash (u undoes)")
                    .clicked()
            {
                header_act = Some(GridAct::Trash);
            }
            if let Some(q) = self.search.clone() {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .button(format!("/{q}  🗙"))
                        .on_hover_text("Clear search (Esc or :noh)")
                        .clicked()
                    {
                        self.search = None;
                    }
                });
            }
        });
        // Second row: how the grid is organised (toolbar strip).
        ui.add_space(2.0);
        ui.horizontal(|ui| {
            let mut changed = false;
            for l in GridLayout::ALL {
                if theme::sel(ui, self.settings.grid_layout == l, l.label())
                    .on_hover_text(match l {
                        GridLayout::Grid => "Equal tiles · :layout grid",
                        GridLayout::Rows => {
                            "Rows of equal height, photos at their own shape · :layout rows"
                        }
                        GridLayout::List => "One line per photo with details · :layout list",
                    })
                    .clicked()
                {
                    self.settings.grid_layout = l;
                    self.save_settings();
                }
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Size").color(theme::TEXT_DIM));
            let r = ui
                .add(
                    egui::Slider::new(
                        &mut self.settings.tile_size,
                        crate::settings::TILE_MIN..=crate::settings::TILE_MAX,
                    )
                    .show_value(false)
                    .logarithmic(true),
                )
                .on_hover_text("Thumbnail size · + / - · ⌘+ ⌘- · ⌘-scroll / pinch on the grid");
            if r.drag_stopped() || (r.changed() && !r.dragged()) {
                self.save_settings();
            }
            if in_trash {
                return;
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Group").color(theme::TEXT_DIM));
            let g = self.group_by();
            let mut pick = g;
            egui::ComboBox::from_id_salt("group")
                .selected_text(g.label())
                .show_ui(ui, |ui| {
                    theme::popup(ui);
                    for x in GroupBy::ALL {
                        theme::sel_value(ui, &mut pick, x, x.label());
                    }
                });
            if pick != g {
                self.set_group_by(pick);
            }
            if matches!(g, GroupBy::Day | GroupBy::Month) && self.settings.sort != SortKey::Date {
                ui.label(
                    RichText::new("(sort by date to group by time)")
                        .small()
                        .color(theme::TEXT_DIM),
                );
            }
            ui.add_space(8.0);
            ui.label(RichText::new("Sort").color(theme::TEXT_DIM));
            let label = match (self.settings.sort, self.settings.sort_desc) {
                (SortKey::Date, true) => "Newest first",
                (SortKey::Date, false) => "Oldest first",
                (SortKey::Name, false) => "Name A–Z",
                (SortKey::Name, true) => "Name Z–A",
            };
            egui::ComboBox::from_id_salt("sort")
                .selected_text(label)
                .show_ui(ui, |ui| {
                    theme::popup(ui);
                    for (k, d, l) in [
                        (SortKey::Date, true, "Newest first"),
                        (SortKey::Date, false, "Oldest first"),
                        (SortKey::Name, false, "Name A–Z"),
                        (SortKey::Name, true, "Name Z–A"),
                    ] {
                        let on = self.settings.sort == k && self.settings.sort_desc == d;
                        if theme::sel(ui, on, l).clicked() {
                            self.settings.sort = k;
                            self.settings.sort_desc = d;
                            changed = true;
                        }
                    }
                });
            if changed {
                self.save_settings();
                self.refresh();
            }
        });
        ui.add_space(6.0);
        if let Some(a) = header_act {
            self.grid_act(&ctx, a, None);
            return;
        }
        if self.items.is_empty() {
            ui.add_space(40.0);
            ui.vertical_centered(|ui| {
                ui.label(
                    RichText::new(match self.source {
                        Source::Inbox => "Inbox is empty — copy scans (JPEG, PNG, TIFF or PDF) into the Inbox folder, drop files here, or use Import…",
                        Source::AllCollections => "No filed photos yet — file scans from the Inbox into a collection.",
                        Source::Trash => "Trash is empty. Photos you remove (Backspace, dd, x) land here and can be restored.",
                        _ => "Nothing here yet.",
                    })
                    .color(theme::TEXT_DIM),
                );
            });
            return;
        }
        let tile = self.settings.tile_size;
        let groups = self.groups.clone();
        let items = self.items.clone();
        let selection: std::collections::HashSet<usize> =
            if self.visual_anchor.is_some() || !self.marks.is_empty() {
                self.selection().into_iter().collect()
            } else {
                Default::default()
            };
        let scroll_to = std::mem::take(&mut self.cursor_moved);
        let mut out = GridOut::default();

        // Background: rubber-band selection (registered first so tiles stay on top for clicks;
        // tiles don't sense drags, so a drag starting on a tile also draws a band).
        let grid_rect = ui.available_rect_before_wrap();
        let bg = ui.interact(grid_rect, ui.id().with("grid_bg"), Sense::drag());
        let mods = ui.input(|i| i.modifiers);
        // Pinch / ⌘-scroll over the grid resizes thumbnails (saved with the other settings on
        // exit or the next settings change).
        if ui.rect_contains_pointer(grid_rect) {
            let z = ui.input(|i| i.zoom_delta());
            if z != 1.0 {
                self.settings.tile_size = (self.settings.tile_size * z)
                    .clamp(crate::settings::TILE_MIN, crate::settings::TILE_MAX);
            }
        }
        // Drag-only widgets start "dragging" on press; only draw a band once the pointer
        // has really moved, so plain clicks on tiles stay clicks.
        if self.marquee.is_none()
            && bg.dragged_by(egui::PointerButton::Primary)
            && ui.input(|i| i.pointer.is_decidedly_dragging())
            && let Some(p) = ui.input(|i| i.pointer.press_origin())
        {
            let keep = mods.command || mods.shift;
            self.marquee = Some(crate::app::Marquee {
                start: p,
                base: if keep {
                    self.marks.clone()
                } else {
                    Default::default()
                },
            });
            self.visual_anchor = None;
        }

        let layout = self.settings.grid_layout;
        let gap = if layout == GridLayout::List {
            0.0
        } else {
            12.0
        };
        let show_collection = matches!(self.source, Source::AllCollections | Source::Tag(_))
            && self.group_by() != GroupBy::Collection;
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let avail = ui.available_width() - 4.0;
                for (label, start, n) in groups {
                    if !label.is_empty() {
                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&label).strong().color(theme::TEXT));
                            ui.label(RichText::new(n.to_string()).small().color(theme::TEXT_DIM));
                        });
                        ui.add_space(2.0);
                    }
                    let aspects: Vec<f32> = (start..start + n)
                        .map(|i| {
                            // Thumbnails load lazily; until then assume a 3:2 scan.
                            self.thumbs
                                .get(items[i].display_path())
                                .map(|t| t.size_vec2())
                                .map(|v| (v.x / v.y.max(1.0)).clamp(0.25, 4.0))
                                .unwrap_or(1.5)
                        })
                        .collect();
                    let rows = layout_rows(layout, &aspects, avail, tile, gap);
                    for row in rows {
                        ui.horizontal(|ui| {
                            ui.spacing_mut().item_spacing = vec2(gap, 0.0);
                            for (k, size) in row {
                                let i = start + k;
                                let cell = GridCell {
                                    i,
                                    size,
                                    layout,
                                    in_trash,
                                    show_collection,
                                    scroll_to,
                                    row_even: i % 2 == 0,
                                };
                                self.grid_cell(ui, &ctx, &items[i], cell, &selection, &mut out);
                            }
                        });
                        ui.add_space(gap);
                    }
                    ui.add_space(if layout == GridLayout::List { 6.0 } else { 0.0 });
                }
            });
        let GridOut {
            rects,
            clicked,
            menu_act,
            drag_from,
        } = out;

        // Rubber band: select every tile it touches (added to the previous marks with ⌘ / ⇧).
        if let Some(m) = self.marquee.clone() {
            let cur = ui.input(|i| i.pointer.interact_pos()).unwrap_or(m.start);
            let band = Rect::from_two_pos(m.start, cur);
            let mut marks = m.base.clone();
            for (i, r) in &rects {
                if r.intersects(band) {
                    marks.insert(items[*i].orig.clone());
                }
            }
            if let Some(last) = rects
                .iter()
                .filter(|(_, r)| r.intersects(band))
                .map(|(i, _)| *i)
                .max()
            {
                self.cursor = last;
            }
            self.marks = marks;
            let p = ctx
                .layer_painter(egui::LayerId::new(
                    egui::Order::Foreground,
                    ui.id().with("band"),
                ))
                .with_clip_rect(grid_rect);
            p.rect_filled(band, 0.0, theme::ACCENT.gamma_multiply(0.18));
            p.rect_stroke(
                band,
                0.0,
                egui::Stroke::new(1.0, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
            if !ui.input(|i| i.pointer.primary_down()) {
                self.marquee = None;
            }
        }
        // Click on empty space (not a tile, not the scroll bar) clears the selection.
        let empty_click = ui.input(|i| i.pointer.primary_clicked())
            && ui.input(|i| i.pointer.interact_pos()).is_some_and(|p| {
                grid_rect.shrink2(vec2(0.0, 0.0)).contains(p)
                    && p.x < grid_rect.right() - 14.0
                    && !rects.iter().any(|(_, r)| r.expand(4.0).contains(p))
            })
            && ctx.layer_id_at(ui.input(|i| i.pointer.interact_pos()).unwrap_or_default())
                == Some(ui.layer_id());
        if empty_click && clicked.is_none() && !(mods.command || mods.shift) {
            self.marks.clear();
            self.visual_anchor = None;
        }
        self.tile_rects = rects;
        // Dragging a tile drags the selection (or just that photo if it isn't selected) — drop
        // it on a collection or Trash in the sidebar.
        if let Some(i) = drag_from {
            if !selection.contains(&i) {
                self.marks.clear();
                self.visual_anchor = None;
                self.cursor = i;
            }
            let origs: Vec<std::path::PathBuf> = self
                .selection()
                .into_iter()
                .filter_map(|k| self.items.get(k))
                .map(|it| it.orig.clone())
                .collect();
            if !origs.is_empty() && self.source != Source::Trash {
                self.dragging = Some(origs);
            }
        }

        if let Some((a, i)) = menu_act {
            self.grid_act(&ctx, a, Some(i));
            return;
        }
        if let Some((i, m, double)) = clicked {
            if double {
                self.cursor = i;
                self.marks.clear();
                let it = self.items[i].clone();
                self.open_item(&ctx, it);
            } else if m.command {
                // ⌘-click toggles; the photo you clicked before stays selected (Finder).
                if self.marks.is_empty()
                    && self.cursor != i
                    && let Some(c) = self.items.get(self.cursor)
                {
                    self.marks.insert(c.orig.clone());
                }
                let p = self.items[i].orig.clone();
                if !self.marks.remove(&p) {
                    self.marks.insert(p);
                }
                self.cursor = i;
            } else if m.shift {
                let (lo, hi) = (self.cursor.min(i), self.cursor.max(i));
                for k in lo..=hi {
                    self.marks.insert(self.items[k].orig.clone());
                }
                self.cursor = i;
            } else {
                self.marks.clear();
                self.visual_anchor = None;
                self.cursor = i;
            }
        }
    }

    /// One photo in the grid / rows / list: paint it and collect its interactions.
    fn grid_cell(
        &mut self,
        ui: &mut Ui,
        ctx: &egui::Context,
        item: &Item,
        c: GridCell,
        selection: &std::collections::HashSet<usize>,
        out: &mut GridOut,
    ) {
        let i = c.i;
        let tex = self.thumb(ctx, item.display_path());
        let title = item.title();
        let (rect, resp) = ui.allocate_exact_size(c.size, Sense::click_and_drag());
        resp.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, &title));
        out.rects.push((i, rect));
        if c.scroll_to && i == self.cursor {
            ui.scroll_to_rect(rect, None);
        }
        let tex_size = self.thumbs.get(item.display_path()).map(|t| t.size_vec2());
        let when = Local
            .timestamp_opt(item.modified, 0)
            .single()
            .map(|t| {
                t.format(if c.layout == GridLayout::List {
                    "%-d %b %Y · %H:%M"
                } else {
                    "%H:%M · %-d %b"
                })
                .to_string()
            })
            .unwrap_or_default();
        let look = TileLook {
            tex: tex.zip(tex_size),
            title: &title,
            when: &when,
            edited: item.edit.is_some(),
            cursor: i == self.cursor,
            selected: selection.contains(&i),
            hovered: resp.hovered(),
            dim: !self.matches_search(i),
        };
        match c.layout {
            GridLayout::List => {
                let tags = self
                    .item_tags
                    .get(i)
                    .map(|t| {
                        t.iter()
                            .map(|t| format!("#{t}"))
                            .collect::<Vec<_>>()
                            .join(" ")
                    })
                    .unwrap_or_default();
                let col = if c.show_collection {
                    collection_of(item)
                } else {
                    String::new()
                };
                paint_list_row(ui.painter(), rect, look, &col, &tags, c.row_even);
            }
            _ => paint_tile(ui.painter(), rect, look),
        }
        let resp = resp.on_hover_text(item.orig.display().to_string());
        if resp.double_clicked() || resp.triple_clicked() {
            out.clicked = Some((i, egui::Modifiers::NONE, true));
        } else if resp.clicked() {
            out.clicked = Some((i, click_modifiers(ui), false));
        }
        if resp.drag_started_by(egui::PointerButton::Primary) {
            out.drag_from = Some(i);
        }
        if resp.secondary_clicked() && !selection.contains(&i) {
            // Right-click outside the selection selects just that photo.
            self.marks.clear();
            self.visual_anchor = None;
            self.cursor = i;
        }
        resp.context_menu(|ui| {
            theme::popup(ui);
            let n = if selection.contains(&i) {
                selection.len()
            } else {
                1
            };
            if c.in_trash {
                if ui
                    .button(if n > 1 {
                        format!("Restore {n}")
                    } else {
                        "Restore".into()
                    })
                    .clicked()
                {
                    out.menu_act = Some((GridAct::Restore, i));
                }
                if ui.button("Delete forever…").clicked() {
                    out.menu_act = Some((GridAct::Purge, i));
                }
            } else {
                if ui.button("Open").clicked() {
                    out.menu_act = Some((GridAct::Open, i));
                }
                if ui.button("Reveal in Finder").clicked() {
                    out.menu_act = Some((GridAct::Reveal, i));
                }
                ui.separator();
                if ui
                    .button(if n > 1 {
                        format!("Move {n} to trash")
                    } else {
                        "Move to trash".into()
                    })
                    .clicked()
                {
                    out.menu_act = Some((GridAct::Trash, i));
                }
            }
        });
    }

    /// Mouse actions from the grid header / context menu.
    fn grid_act(&mut self, ctx: &egui::Context, a: GridAct, _on: Option<usize>) {
        use crate::keys::Act;
        match a {
            GridAct::Open => self.act(ctx, Act::Open),
            GridAct::Reveal => {
                if let Some(it) = self.items.get(self.cursor) {
                    let _ = std::process::Command::new("open")
                        .arg("-R")
                        .arg(&it.orig)
                        .spawn();
                }
            }
            GridAct::Trash | GridAct::Purge => self.act(ctx, Act::Delete),
            GridAct::Restore => self.act(ctx, Act::Restore),
            GridAct::EmptyTrash => self.confirm = Some(crate::app::Confirm::EmptyTrash),
        }
    }

    /// Spatial neighbour for j / k using last frame's tile rects.
    pub fn vertical_neighbor(&self, from: usize, down: bool) -> usize {
        let Some(cur) = self
            .tile_rects
            .iter()
            .find(|(i, _)| *i == from)
            .map(|(_, r)| *r)
        else {
            return if down {
                (from + 1).min(self.items.len().saturating_sub(1))
            } else {
                from.saturating_sub(1)
            };
        };
        let cy = cur.center().y;
        let cands = self.tile_rects.iter().filter(|(_, r)| {
            if down {
                r.center().y > cy + 4.0
            } else {
                r.center().y < cy - 4.0
            }
        });
        // Nearest row first, then nearest column.
        cands
            .min_by(|(_, a), (_, b)| {
                let ka = (
                    (a.center().y - cy).abs().round() as i64,
                    (a.center().x - cur.center().x).abs().round() as i64,
                );
                let kb = (
                    (b.center().y - cy).abs().round() as i64,
                    (b.center().x - cur.center().x).abs().round() as i64,
                );
                ka.cmp(&kb)
            })
            .map(|(i, _)| *i)
            .unwrap_or(from)
    }

    pub fn row_bounds(&self, from: usize) -> (usize, usize) {
        let Some(cur) = self
            .tile_rects
            .iter()
            .find(|(i, _)| *i == from)
            .map(|(_, r)| *r)
        else {
            return (from, from);
        };
        let row: Vec<usize> = self
            .tile_rects
            .iter()
            .filter(|(_, r)| (r.center().y - cur.center().y).abs() < 4.0)
            .map(|(i, _)| *i)
            .collect();
        (
            *row.iter().min().unwrap_or(&from),
            *row.iter().max().unwrap_or(&from),
        )
    }
}

/// Modifiers held for this frame's click (from the release event itself, so a quick
/// ⌘-click whose key-up arrives in the same frame still counts).
fn click_modifiers(ui: &Ui) -> egui::Modifiers {
    ui.input(|i| {
        i.events
            .iter()
            .rev()
            .find_map(|e| match e {
                egui::Event::PointerButton {
                    pressed: false,
                    modifiers,
                    ..
                } => Some(*modifiers),
                _ => None,
            })
            .map(|m| m | i.modifiers)
            .unwrap_or(i.modifiers)
    })
}
