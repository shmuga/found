//! Application state, message handling, sidebar, toolbar and the library grid.

use crate::collage::{self, Collage};
use crate::graph::{self, Evaluator, Graph};
use crate::img::Img;
use crate::library::{Item, ItemRef, Library};
use crate::presets::{self, Preset};
use crate::settings::{Layout, Settings};
use crate::ui::theme;
use crate::undo::History;
use crate::worker::Worker;
use egui::{TextureHandle, Ui};
use egui_snarl::NodeId;
use std::collections::{BTreeSet, HashMap, HashSet};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

#[derive(Clone, Debug, Default)]
pub struct AppConfig {
    /// Open this library directly (e.g. from the command line), skipping the startup picker.
    pub library: Option<PathBuf>,
    /// Where settings are read/written. `None` = in-memory only (tests).
    pub settings_file: Option<PathBuf>,
    /// Run background jobs inline (tests).
    pub sync: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum View {
    Library,
    Edit,
    Collage,
    Settings,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    Inbox,
    Collection(String),
    Tag(String),
    Trash,
    /// Every filed photo (sidebar "Collections" heading), grouped by collection by default.
    AllCollections,
}

/// Where the user wanted to go when unsaved changes stopped them (see `leave_modal`).
#[derive(Clone, Debug)]
pub enum Leave {
    View { from: View, to: View },
    OpenItem(Item),
    OpenCollage(PathBuf),
    NewCollage(Option<String>),
    CloseLibrary,
    SetLibrary(PathBuf),
    Quit,
}

impl Leave {
    /// Which open documents this move would abandon: (photo edit, collage).
    fn abandons(&self) -> (bool, bool) {
        match self {
            Leave::View { from, .. } => (*from == View::Edit, *from == View::Collage),
            Leave::OpenItem(_) => (true, false),
            Leave::OpenCollage(_) | Leave::NewCollage(_) => (false, true),
            Leave::CloseLibrary | Leave::SetLibrary(_) | Leave::Quit => (true, true),
        }
    }
}

/// What the sidebar's inline rename field is renaming.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum RenameTarget {
    Collection(String),
    Collage(PathBuf),
}

/// Where dragged library photos can be dropped (sidebar rows).
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DropTarget {
    Collection(String),
    Trash,
}

/// A destructive action waiting for confirmation (permanent delete from the trash).
#[derive(Clone, Debug, PartialEq)]
pub enum Confirm {
    Purge(Vec<String>),
    EmptyTrash,
}

/// Mouse rubber-band selection in the library grid.
#[derive(Clone, Debug)]
pub struct Marquee {
    pub start: egui::Pos2,
    /// Marks before the drag (kept when ⌘ / ⇧ is held).
    pub base: BTreeSet<PathBuf>,
}

pub enum Msg {
    Thumb(PathBuf, Option<egui::ColorImage>),
    Proxy(ItemRef, Result<Arc<Img>, String>),
    Preview {
        generation: u64,
        img: Option<(egui::ColorImage, [usize; 2])>,
        ms: f32,
        crop: bool,
    },
    Wb {
        node: NodeId,
        temp: f32,
        tint: f32,
    },
    Exported {
        r: ItemRef,
        result: Result<PathBuf, String>,
    },
    CollageSrc(String, Option<Arc<Img>>),
    CollageExported(Result<PathBuf, String>),
    /// Background batch render (filing / pasting recipes onto several photos).
    Batch(Vec<(ItemRef, Result<PathBuf, String>)>),
    PdfExtracted(Vec<(PathBuf, Result<usize, String>)>),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Tool {
    None,
    Crop,
    Eyedropper(NodeId),
}

#[derive(Clone, Debug)]
pub struct Doc {
    pub graph: Graph,
    pub tags: Vec<String>,
}

impl Doc {
    pub fn key(&self) -> String {
        format!("{}|{}", graph::graph_json(&self.graph), self.tags.join(","))
    }
}

pub struct Editor {
    pub item: Item,
    pub doc: Doc,
    pub history: History<Doc>,
    pub proxy: Option<Arc<Img>>,
    pub proxy_key: u64,
    pub load_error: Option<String>,
    pub preview: Option<TextureHandle>,
    pub preview_size: [usize; 2],
    /// Size of the crop node's (rotated) input, for aspect math.
    pub crop_src_size: Option<(usize, usize)>,
    pub evaluator: Arc<Mutex<Evaluator>>,
    pub latest_gen: Arc<AtomicU64>,
    pub requested: String,
    pub shown_gen: u64,
    pub render_ms: f32,
    pub patch: bool,
    pub tool: Tool,
    pub compare: bool,
    pub selected: Option<NodeId>,
    pub roll: u32,
    pub tag_input: String,
    pub file_target: String,
    pub preset_pick: usize,
    pub preset_name: String,
    /// The rendered edit.jpg no longer matches the recipe.
    pub stale: bool,
    pub exporting: bool,
    pub crop_drag: Option<CropDrag>,
    /// Keyboard cursor over the slider rows (see `ui::vim::rows`).
    pub param_cursor: usize,
    pub cursor_moved: bool,
    pub zoom: crate::ui::zoom::ZoomPan,
    pub last_adjust: Option<crate::keys::Act>,
    /// The recipe as last saved to disk; edits stay in memory until saved (⌘S / :w).
    pub saved: Doc,
}

impl Editor {
    pub fn dirty(&self) -> bool {
        self.doc.key() != self.saved.key()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct CropDrag {
    pub handle: u8, // 0 move, 1..=4 corners TL,TR,BR,BL
    pub start_rect: [f32; 4],
    pub start_pos: egui::Pos2,
}

pub struct CollageEd {
    pub path: PathBuf,
    pub doc: Collage,
    pub history: History<Collage>,
    pub selected: Option<u32>,
    pub tool: CTool,
    pub lasso: Vec<[f32; 2]>,
    pub rect_start: Option<egui::Pos2>,
    pub sources: HashMap<String, Arc<Img>>,
    pub pending: HashSet<String>,
    pub preview: Option<TextureHandle>,
    pub preview_key: String,
    pub pick_collection: String,
    pub drag: Option<CDrag>,
    pub exporting: bool,
    pub last_export: Option<PathBuf>,
    pub zoom: crate::ui::zoom::ZoomPan,
    /// The document as last saved.
    pub saved: Collage,
    /// Layer being dragged in the Layers panel (id).
    pub layer_drag: Option<u32>,
}

impl CollageEd {
    pub fn dirty(&self) -> bool {
        serde_json::to_string(&self.doc).ok() != serde_json::to_string(&self.saved).ok()
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CTool {
    Move,
    Lasso,
    RectMask,
}

#[derive(Clone, Copy, Debug)]
pub enum CDrag {
    Move {
        id: u32,
        start: egui::Pos2,
        cx: f32,
        cy: f32,
    },
    Scale {
        id: u32,
        start_dist: f32,
        width: f32,
    },
    Rotate {
        id: u32,
    },
}

pub struct FoundApp {
    pub lib: Option<Library>,
    pub worker: Worker<Msg>,
    pub view: View,
    pub source: Source,
    pub items: Vec<Item>,
    pub collections: Vec<String>,
    pub tags: Vec<String>,
    pub collage_files: Vec<PathBuf>,
    pub inbox_count: usize,
    pub trash_count: usize,
    /// Trash keys from the last "move to trash", for `u` / ⌘Z in the library.
    pub last_trashed: Vec<String>,
    pub confirm: Option<Confirm>,
    pub marquee: Option<Marquee>,
    /// Collection being renamed in the sidebar: (old name, text field).
    pub renaming: Option<(RenameTarget, String)>,
    /// Sidebar "new collage" field.
    pub new_collage_name: String,
    /// Photos being dragged from the grid (originals), and the sidebar row under the pointer.
    pub dragging: Option<Vec<PathBuf>>,
    pub drop_hover: Option<DropTarget>,
    /// Pending navigation blocked by unsaved changes (shows the save prompt).
    pub leave: Option<Leave>,
    /// The user confirmed quitting; let the window close.
    pub quitting: bool,
    pub thumbs: HashMap<PathBuf, TextureHandle>,
    pub thumbs_pending: HashSet<PathBuf>,
    pub editor: Option<Editor>,
    pub collage: Option<CollageEd>,
    pub presets: Vec<Preset>,
    pub status: String,
    pub new_collection: String,
    pub library_path_input: String,
    pub last_refresh: f64,
    pub themed: bool,
    pub settings: Settings,
    pub settings_file: Option<PathBuf>,
    /// Folder layout being edited on the Settings page (applied with "Apply folders").
    pub layout_draft: Option<Layout>,
    // ---- keyboard / selection
    pub keys: crate::keys::Keys,
    pub cursor: usize,
    pub cursor_moved: bool,
    pub marks: BTreeSet<PathBuf>,
    pub visual_anchor: Option<usize>,
    /// Screen rects of library tiles from the last frame (index into `items`), for j/k.
    pub tile_rects: Vec<(usize, egui::Rect)>,
    /// Day groups over `items` (label, first index, count).
    pub groups: Vec<(String, usize, usize)>,
    pub item_tags: Vec<Vec<String>>,
    pub search: Option<String>,
    pub help: bool,
    pub completions: Vec<String>,
    pub yank_recipe: Option<Graph>,
    pub yank_node: Option<crate::graph::NodeData>,
    pub yank_layer: Option<crate::collage::Layer>,
    pub pdf_busy: bool,
    pub ctx: Option<egui::Context>,
    /// A text field had keyboard focus at the end of the last frame.
    pub text_focus: bool,
}

impl FoundApp {
    pub fn new(cfg: AppConfig) -> Self {
        let settings = cfg
            .settings_file
            .as_deref()
            .map(Settings::load)
            .unwrap_or_default();
        let mut app = Self {
            lib: None,
            worker: Worker::new(cfg.sync),
            view: View::Library,
            source: Source::Inbox,
            items: vec![],
            collections: vec![],
            tags: vec![],
            collage_files: vec![],
            inbox_count: 0,
            trash_count: 0,
            last_trashed: vec![],
            confirm: None,
            marquee: None,
            renaming: None,
            new_collage_name: String::new(),
            dragging: None,
            drop_hover: None,
            leave: None,
            quitting: false,
            thumbs: HashMap::new(),
            thumbs_pending: HashSet::new(),
            editor: None,
            collage: None,
            presets: presets::builtin(),
            status: String::new(),
            new_collection: String::new(),
            library_path_input: String::new(),
            last_refresh: 0.0,
            themed: false,
            settings,
            settings_file: cfg.settings_file,
            layout_draft: None,
            keys: Default::default(),
            cursor: 0,
            cursor_moved: false,
            marks: BTreeSet::new(),
            visual_anchor: None,
            tile_rects: vec![],
            groups: vec![],
            item_tags: vec![],
            search: None,
            help: false,
            completions: vec![],
            yank_recipe: None,
            yank_node: None,
            yank_layer: None,
            pdf_busy: false,
            ctx: None,
            text_focus: false,
        };
        // Startup: an explicit library wins; otherwise reopen the last one only if the user
        // asked for that, else show the picker.
        let startup = cfg.library.or_else(|| {
            app.settings
                .open_last_on_startup
                .then(|| app.settings.last_library().cloned())
                .flatten()
                .filter(|p| p.is_dir())
        });
        if let Some(root) = startup {
            app.set_library(root);
        }
        app
    }

    pub fn from_creation_context(cc: &eframe::CreationContext<'_>, cfg: AppConfig) -> Self {
        theme::apply(&cc.egui_ctx);
        let mut app = Self::new(cfg);
        app.themed = true;
        app
    }

    pub fn save_settings(&mut self) {
        if let Some(p) = &self.settings_file
            && let Err(e) = self.settings.save(p)
        {
            self.status = format!("Couldn't save settings: {e:#}");
        }
    }

    /// The grouping that applies to the current view.
    pub fn group_by(&self) -> crate::settings::GroupBy {
        match self.source {
            Source::AllCollections => self.settings.group_all,
            Source::Trash => crate::settings::GroupBy::Day,
            _ => self.settings.group_by,
        }
    }

    pub fn set_group_by(&mut self, g: crate::settings::GroupBy) {
        match self.source {
            Source::AllCollections => self.settings.group_all = g,
            Source::Trash => {}
            _ => self.settings.group_by = g,
        }
        self.save_settings();
        self.refresh();
    }

    pub fn editor_dirty(&self) -> bool {
        self.editor.as_ref().is_some_and(|e| e.dirty())
    }

    pub fn collage_dirty(&self) -> bool {
        self.collage.as_ref().is_some_and(|c| c.dirty())
    }

    pub fn any_dirty(&self) -> bool {
        self.editor_dirty() || self.collage_dirty()
    }

    pub fn set_library(&mut self, root: PathBuf) {
        if self.any_dirty() {
            self.leave = Some(Leave::SetLibrary(root));
            return;
        }
        let layout = self.settings.layout_for(&root);
        match Library::open_with(&root, &layout) {
            Ok(l) => {
                self.lib = Some(l);
                self.editor = None;
                self.collage = None;
                self.layout_draft = None;
                self.thumbs.clear();
                self.view = View::Library;
                self.source = Source::Inbox;
                self.settings.touch_recent(&root);
                self.save_settings();
                self.refresh();
                self.status = format!("Library: {}", root.display());
            }
            Err(e) => self.status = format!("Can't open library: {e:#}"),
        }
    }

    /// Close the library and go back to the startup picker.
    pub fn close_library(&mut self) {
        if self.any_dirty() {
            self.leave = Some(Leave::CloseLibrary);
            return;
        }
        self.lib = None;
        self.editor = None;
        self.collage = None;
        self.items.clear();
        self.view = View::Library;
    }

    /// Re-read listings from disk.
    pub fn refresh(&mut self) {
        let Some(lib) = &self.lib else { return };
        self.collections = lib.collections();
        self.tags = lib.all_tags().into_iter().collect();
        self.collage_files = collage::list(&lib.collages_dir());
        let inbox = lib.inbox_items();
        self.inbox_count = inbox.len();
        self.trash_count = lib.trash_count();
        let current = self.items.get(self.cursor).map(|i| i.orig.clone());
        let mut items = match &self.source {
            Source::Inbox => inbox,
            Source::Collection(c) => lib.items(c),
            Source::Tag(t) => lib.items_with_tag(t),
            Source::Trash => lib.trash_items(),
            Source::AllCollections => lib.all_items(),
        };
        if self.source != Source::Trash {
            crate::ui::library_ui::sort_items(&mut items, &self.settings);
        }
        self.groups =
            crate::ui::library_ui::make_groups(&mut items, self.group_by(), &self.settings);
        self.item_tags = items.iter().map(|i| lib.read_sidecar(i).tags).collect();
        self.items = items;
        // Keep the cursor on the same photo across refreshes.
        if let Some(c) = current
            && let Some(i) = self.items.iter().position(|i| i.orig == c)
        {
            self.cursor = i;
        }
        self.cursor = self.cursor.min(self.items.len().saturating_sub(1));
        self.marks.retain(|m| m.exists());
        self.presets = presets::builtin();
        self.presets.extend(presets::load_user(&lib.presets_dir()));
        self.extract_pending_pdfs();
    }

    /// PDFs dropped into the Inbox are unpacked in the background.
    fn extract_pending_pdfs(&mut self) {
        let Some(lib) = self.lib.clone() else { return };
        if self.pdf_busy {
            return;
        }
        let pdfs = lib.inbox_pdfs();
        if pdfs.is_empty() {
            return;
        }
        self.pdf_busy = true;
        self.status = format!("Extracting {} PDF(s)…", pdfs.len());
        let ctx = self.ctx.clone();
        let job = move || {
            Msg::PdfExtracted(
                pdfs.into_iter()
                    .map(|p| {
                        let r = lib
                            .extract_pdf(&p)
                            .map(|v| v.len())
                            .map_err(|e| format!("{e:#}"));
                        (p, r)
                    })
                    .collect(),
            )
        };
        match ctx {
            Some(ctx) => self.worker.spawn(&ctx, job),
            None => {
                // No context yet (constructor): run inline.
                let m = job();
                self.on_pdf_extracted(m);
            }
        }
    }

    pub fn on_pdf_extracted(&mut self, m: Msg) {
        if let Msg::PdfExtracted(v) = m {
            self.pdf_busy = false;
            let ok: usize = v.iter().filter_map(|(_, r)| r.as_ref().ok()).sum();
            let bad: Vec<String> = v
                .iter()
                .filter_map(|(p, r)| {
                    r.as_ref().err().map(|e| {
                        format!(
                            "{}: {e}",
                            p.file_name().unwrap_or_default().to_string_lossy()
                        )
                    })
                })
                .collect();
            self.status = if bad.is_empty() {
                format!("Extracted {ok} scan(s) from PDF")
            } else {
                format!("Extracted {ok} scan(s); couldn't read {}", bad.join(", "))
            };
            self.refresh();
        }
    }

    // ------------------------------------------------------------- messages

    fn handle_messages(&mut self, ctx: &egui::Context) {
        for m in self.worker.poll() {
            match m {
                Msg::Thumb(p, img) => {
                    self.thumbs_pending.remove(&p);
                    if let Some(img) = img {
                        let t = ctx.load_texture(
                            p.display().to_string(),
                            img,
                            egui::TextureOptions::LINEAR,
                        );
                        self.thumbs.insert(p, t);
                    }
                }
                Msg::Proxy(r, res) => {
                    if let Some(ed) = &mut self.editor
                        && ed.item.r == r
                    {
                        match res {
                            Ok(img) => {
                                ed.proxy = Some(img);
                                ed.evaluator.lock().unwrap().clear();
                                ed.requested.clear();
                            }
                            Err(e) => ed.load_error = Some(e),
                        }
                    }
                }
                Msg::Preview {
                    generation,
                    img,
                    ms,
                    crop,
                } => {
                    if let Some(ed) = &mut self.editor
                        && generation >= ed.shown_gen
                        && img.is_some()
                    {
                        ed.shown_gen = generation;
                        ed.render_ms = ms;
                        if let Some((ci, size)) = img {
                            if crop {
                                ed.crop_src_size = Some((size[0], size[1]));
                            }
                            ed.preview_size = size;
                            match &mut ed.preview {
                                Some(t) => t.set(ci, egui::TextureOptions::LINEAR),
                                None => {
                                    ed.preview = Some(ctx.load_texture(
                                        "preview",
                                        ci,
                                        egui::TextureOptions::LINEAR,
                                    ))
                                }
                            }
                        }
                    }
                }
                Msg::Wb { node, temp, tint } => {
                    if let Some(ed) = &mut self.editor
                        && let Some(n) = ed.doc.graph.get_node_mut(node)
                        && let crate::ops::Op::WhiteBalance { temp: t, tint: n2 } = &mut n.op
                    {
                        *t = temp.round();
                        *n2 = tint.round();
                        self.status = format!("White balance: temp {temp:+.0}, tint {tint:+.0}");
                    }
                }
                Msg::Exported { r, result } => {
                    match &result {
                        Ok(p) => {
                            self.status = format!("Saved {}", p.display());
                            self.thumbs.retain(|k, _| k != p);
                        }
                        Err(e) => self.status = format!("Export failed: {e}"),
                    }
                    if let Some(ed) = &mut self.editor
                        && ed.item.r == r
                    {
                        ed.exporting = false;
                        if result.is_ok() {
                            ed.stale = false;
                            if let Some(lib) = &self.lib
                                && let Some(it) = lib.item(&r)
                            {
                                ed.item = it;
                            }
                        }
                    }
                    self.refresh();
                }
                Msg::CollageSrc(key, img) => {
                    if let Some(c) = &mut self.collage {
                        c.pending.remove(&key);
                        if let Some(img) = img {
                            c.sources.insert(key, img);
                            c.preview_key.clear();
                        }
                    }
                }
                m @ Msg::PdfExtracted(_) => self.on_pdf_extracted(m),
                Msg::Batch(results) => {
                    let ok = results.iter().filter(|(_, r)| r.is_ok()).count();
                    let errs: Vec<&String> = results
                        .iter()
                        .filter_map(|(_, r)| r.as_ref().err())
                        .collect();
                    self.status = if errs.is_empty() {
                        format!("Rendered {ok} photo(s)")
                    } else {
                        format!("Rendered {ok}, {} failed: {}", errs.len(), errs[0])
                    };
                    for (r, res) in &results {
                        if let Ok(p) = res {
                            self.thumbs.remove(p);
                        }
                        if let Some(ed) = &mut self.editor
                            && &ed.item.r == r
                            && res.is_ok()
                        {
                            ed.stale = false;
                        }
                    }
                    self.refresh();
                }
                Msg::CollageExported(res) => {
                    if let Some(c) = &mut self.collage {
                        c.exporting = false;
                        if let Ok(p) = &res {
                            c.last_export = Some(p.clone());
                        }
                    }
                    self.status = match res {
                        Ok(p) => format!("Collage exported: {}", p.display()),
                        Err(e) => format!("Collage export failed: {e}"),
                    };
                }
            }
        }
    }

    pub fn thumb(
        &mut self,
        ctx: &egui::Context,
        path: &std::path::Path,
    ) -> Option<egui::TextureId> {
        if let Some(t) = self.thumbs.get(path) {
            return Some(t.id());
        }
        if self.thumbs_pending.insert(path.to_path_buf()) {
            let p = path.to_path_buf();
            let cache = self.settings.thumb_dir();
            self.worker.spawn_pooled(ctx, move || {
                let img = crate::io::load_thumb(&p, cache.as_deref())
                    .ok()
                    .map(|i| i.to_color_image());
                Msg::Thumb(p, img)
            });
        }
        None
    }

    // ------------------------------------------------------------- editor lifecycle

    pub fn open_item(&mut self, ctx: &egui::Context, item: Item) {
        if item.is_trashed() {
            self.status = format!("{} is in the trash — restore it (r) to edit", item.title());
            return;
        }
        if self
            .editor
            .as_ref()
            .is_some_and(|e| e.dirty() && e.item.orig != item.orig)
        {
            self.leave = Some(Leave::OpenItem(item));
            return;
        }
        let Some(lib) = &self.lib else { return };
        let sc = lib.read_sidecar(&item);
        let doc = Doc {
            graph: sc.graph,
            tags: sc.tags,
        };
        let key = doc.key();
        let collection_default = match &item.r {
            ItemRef::Filed { collection, .. } => collection.clone(),
            ItemRef::Inbox(_) | ItemRef::Trashed { .. } => match &self.source {
                Source::Collection(c) => c.clone(),
                _ => self.collections.first().cloned().unwrap_or_default(),
            },
        };
        let stale = matches!(item.r, ItemRef::Filed { .. }) && item.edit.is_none();
        let ed = Editor {
            history: History::new(doc.clone(), key),
            doc: doc.clone(),
            proxy: None,
            proxy_key: path_key(&item.orig),
            load_error: None,
            preview: None,
            preview_size: [0, 0],
            crop_src_size: None,
            evaluator: Arc::new(Mutex::new(Evaluator::new(true))),
            latest_gen: Arc::new(AtomicU64::new(0)),
            requested: String::new(),
            shown_gen: 0,
            render_ms: 0.0,
            patch: self.editor.as_ref().map(|e| e.patch).unwrap_or(false),
            tool: Tool::None,
            compare: false,
            selected: None,
            roll: 1,
            tag_input: String::new(),
            file_target: collection_default,
            preset_pick: 0,
            preset_name: String::new(),
            stale,
            exporting: false,
            crop_drag: None,
            param_cursor: 0,
            cursor_moved: false,
            zoom: Default::default(),
            last_adjust: None,
            saved: doc.clone(),
            item: item.clone(),
        };
        self.editor = Some(ed);
        self.view = View::Edit;
        let r = item.r.clone();
        let orig = item.orig.clone();
        let edge = self.settings.preview_edge.clamp(800, 8192);
        self.worker.spawn(ctx, move || {
            let res = crate::io::load_scaled(&orig, edge)
                .map(Arc::new)
                .map_err(|e| format!("{e:#}"));
            Msg::Proxy(r, res)
        });
    }

    /// Write the open photo's recipe + tags to its sidecar (an explicit save).
    pub fn save_editor_sidecar(&mut self) {
        let (Some(lib), Some(ed)) = (&self.lib, &mut self.editor) else {
            return;
        };
        let mut sc = lib.read_sidecar(&ed.item);
        sc.graph = ed.doc.graph.clone();
        sc.tags = ed.doc.tags.clone();
        let saved = match lib.write_sidecar(&ed.item, &sc) {
            Ok(()) => {
                ed.saved = ed.doc.clone();
                self.tags = lib.all_tags().into_iter().collect();
                Some((
                    format!("edits of {}", ed.item.title()),
                    ed.doc.graph.clone(),
                ))
            }
            Err(e) => {
                self.status = format!("Couldn't save recipe: {e:#}");
                None
            }
        };
        if let Some((name, g)) = saved {
            self.remember_recipe(name, g);
        }
    }

    /// Remember a recipe for the editor's "Apply last" button (persisted in settings).
    pub fn remember_recipe(&mut self, name: String, graph: Graph) {
        self.settings.last_recipe = Some(crate::settings::LastRecipe { name, graph });
        self.save_settings();
    }

    /// "Apply last": the last applied preset / last saved photo's edits onto this photo. Like a
    /// preset, it keeps this photo's crop and white balance unless the recipe sets them.
    pub fn apply_last(&mut self) {
        let Some(last) = self.settings.last_recipe.clone() else {
            self.status = "Nothing to apply yet — apply a preset or save a photo first".into();
            return;
        };
        if let Some(ed) = &mut self.editor {
            ed.doc.graph = crate::presets::apply(&ed.doc.graph, &last.graph);
            ed.selected = None;
            self.status = format!("Applied last: {}", last.name);
        }
    }

    /// Throw away unsaved edits of the open photo.
    pub fn discard_editor(&mut self) {
        if let Some(ed) = &mut self.editor {
            ed.doc = ed.saved.clone();
            ed.requested.clear();
        }
    }

    pub fn discard_collage(&mut self) {
        if let Some(c) = &mut self.collage {
            c.doc = c.saved.clone();
            c.preview_key.clear();
        }
    }

    /// Copy the open photo into `collection` as a new item with the current (unsaved or saved)
    /// recipe, and render its edit. The open photo itself is left as it is.
    pub fn save_copy(&mut self, ctx: &egui::Context, collection: &str) {
        let (Some(lib), Some(ed)) = (&self.lib, &self.editor) else {
            return;
        };
        let Some(col) = self
            .collections
            .iter()
            .find(|c| c.eq_ignore_ascii_case(collection.trim()))
            .cloned()
        else {
            self.status = format!("No collection {collection}");
            return;
        };
        let mut sc = lib.read_sidecar(&ed.item);
        sc.graph = ed.doc.graph.clone();
        sc.tags = ed.doc.tags.clone();
        match lib.copy_into(&ed.item, &col, &sc) {
            Ok(item) => {
                let dest = lib.edit_path(&item).unwrap();
                let graph = ed.doc.graph.clone();
                self.render_job(ctx, item.orig.clone(), graph, dest, item.r.clone());
                self.status = format!("Saved a copy as {} — rendering…", item.title());
                self.refresh();
            }
            Err(e) => self.status = format!("Couldn't save a copy: {e:#}"),
        }
    }

    /// Full-resolution render of `orig` through `graph` into `dest` (background).
    fn render_job(
        &mut self,
        ctx: &egui::Context,
        orig: PathBuf,
        graph: Graph,
        dest: PathBuf,
        r: ItemRef,
    ) {
        let quality = self.settings.jpeg_quality.clamp(50, 100);
        self.worker.spawn(ctx, move || {
            let res = (|| -> anyhow::Result<PathBuf> {
                let src = crate::io::load_full(&orig)?;
                let out = graph::render_full(&graph, src)
                    .ok_or_else(|| anyhow::anyhow!("graph has no connected output"))?;
                crate::io::save_jpeg(&out, &dest, quality)?;
                Ok(dest)
            })();
            Msg::Exported {
                r,
                result: res.map_err(|e| format!("{e:#}")),
            }
        });
    }

    /// Move the current Inbox item into `collection`, write recipe + render edit.jpg.
    pub fn file_current(&mut self, ctx: &egui::Context, collection: &str) {
        let (Some(lib), Some(ed)) = (&self.lib, &mut self.editor) else {
            return;
        };
        if !matches!(ed.item.r, ItemRef::Inbox(_)) {
            return;
        }
        let mut sc = lib.read_sidecar(&ed.item);
        sc.tags = ed.doc.tags.clone();
        sc.graph = ed.doc.graph.clone();
        match lib.file_into(&ed.item, collection, &sc) {
            Ok(item) => {
                let id = match &item.r {
                    ItemRef::Filed { id, .. } => id.clone(),
                    _ => String::new(),
                };
                self.status = format!("Filed into {collection} as {id}");
                ed.saved = ed.doc.clone();
                self.settings.last_recipe = Some(crate::settings::LastRecipe {
                    name: format!("edits of {}", sc.source_name),
                    graph: ed.doc.graph.clone(),
                });
                ed.item = item;
                ed.stale = true;
                self.export_current(ctx);
                self.refresh();
            }
            Err(e) => self.status = format!("Filing failed: {e:#}"),
        }
    }

    /// Render the full-resolution edit for the current (filed) item in the background.
    pub fn export_current(&mut self, ctx: &egui::Context) {
        self.save_editor_sidecar();
        let (Some(lib), Some(ed)) = (&self.lib, &mut self.editor) else {
            return;
        };
        let Some(dest) = lib.edit_path(&ed.item) else {
            self.status = "File the scan into a collection first".into();
            return;
        };
        ed.exporting = true;
        let graph = ed.doc.graph.clone();
        let orig = ed.item.orig.clone();
        let r = ed.item.r.clone();
        self.status = "Rendering full resolution…".into();
        self.render_job(ctx, orig, graph, dest, r);
    }

    pub fn next_inbox_item(&self) -> Option<Item> {
        self.lib.as_ref()?.inbox_items().into_iter().next()
    }

    // ------------------------------------------------------------- trash

    /// Move these photos to the library's local trash (restorable).
    pub fn trash_items(&mut self, items: Vec<Item>) {
        let Some(lib) = self.lib.clone() else { return };
        let items: Vec<Item> = items.into_iter().filter(|i| !i.is_trashed()).collect();
        if items.is_empty() {
            return;
        }
        let mut keys = vec![];
        let mut err = None;
        for it in &items {
            match lib.trash(it) {
                Ok(k) => keys.push(k),
                Err(e) => err = Some(format!("{}: {e:#}", it.title())),
            }
            self.thumbs.remove(&it.orig);
            if let Some(e) = &it.edit {
                self.thumbs.remove(e);
            }
            if self
                .editor
                .as_ref()
                .is_some_and(|ed| ed.item.orig == it.orig)
            {
                self.editor = None;
            }
        }
        self.status = match err {
            None => format!("Moved {} photo(s) to the trash — u to undo", keys.len()),
            Some(e) => format!("Moved {} to the trash; failed: {e}", keys.len()),
        };
        self.last_trashed = keys;
        self.marks.clear();
        self.visual_anchor = None;
        if self.keys.mode == crate::keys::Mode::Visual {
            self.keys.mode = crate::keys::Mode::Normal;
        }
        self.refresh();
    }

    /// Rename a collection and keep everything that points at it working: the open photo,
    /// the open collage (in memory, saved or not), the current view and filing targets.
    pub fn rename_collection(&mut self, old: &str, new: &str) {
        let Some(lib) = self.lib.clone() else { return };
        let new = match lib.rename_collection(old, new) {
            Ok(n) => n,
            Err(e) => {
                self.status = format!("Couldn't rename {old}: {e:#}");
                return;
            }
        };
        if let Some(ed) = &mut self.editor {
            if let ItemRef::Filed { collection, id } = &ed.item.r
                && collection == old
                && let Some(it) = lib.item(&ItemRef::Filed {
                    collection: new.clone(),
                    id: id.clone(),
                })
            {
                ed.item = it;
            }
            if ed.file_target == old {
                ed.file_target = new.clone();
            }
        }
        if let Some(c) = &mut self.collage {
            let (o, n) = (format!("{old}/"), format!("{new}/"));
            for doc in [&mut c.doc, &mut c.saved] {
                for l in &mut doc.layers {
                    if let Some(rest) = l.source.strip_prefix(&o) {
                        l.source = format!("{n}{rest}");
                    }
                }
            }
            c.sources.clear();
            c.preview_key.clear();
            if c.pick_collection == old {
                c.pick_collection = new.clone();
            }
        }
        if self.source == Source::Collection(old.to_string()) {
            self.source = Source::Collection(new.clone());
        }
        let prefix = lib.root.join(old);
        self.marks = std::mem::take(&mut self.marks)
            .into_iter()
            .map(|m| {
                m.strip_prefix(&prefix)
                    .map(|r| lib.root.join(&new).join(r))
                    .unwrap_or(m)
            })
            .collect();
        self.thumbs.retain(|p, _| !p.starts_with(&prefix));
        self.status = format!("Renamed {old} → {new}");
        self.refresh();
    }

    /// Rename a collage's files (and document name). The open collage follows, keeping any
    /// unsaved edits.
    pub fn rename_collage(&mut self, path: &std::path::Path, new: &str) {
        let old = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("collage")
            .to_string();
        match collage::rename(path, new) {
            Ok(dest) => {
                if let Some(c) = &mut self.collage
                    && c.path == path
                {
                    c.path = dest;
                    c.doc.name = new.trim().to_string();
                    c.saved.name = c.doc.name.clone();
                }
                self.status = format!("Renamed collage {old} → {}", new.trim());
            }
            Err(e) => {
                // Put the name field back to the saved name.
                if let Some(c) = &mut self.collage
                    && c.path == path
                {
                    c.doc.name = c.saved.name.clone();
                }
                self.status = format!("Couldn't rename {old}: {e:#}");
            }
        }
        self.refresh();
    }

    /// Move a collage (document + exported JPEG) to the local trash.
    pub fn trash_collage(&mut self, path: PathBuf) {
        let Some(lib) = self.lib.clone() else { return };
        // Drop the open document first so autosave can't write it back.
        if self.collage.as_ref().is_some_and(|c| c.path == path) {
            self.collage = None;
            if self.view == View::Collage {
                self.view = View::Library;
            }
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("collage")
            .to_string();
        match lib.trash_collage(&path) {
            Ok(k) => {
                self.last_trashed = vec![k];
                self.status = format!("Moved collage {name} to the trash — u to undo");
            }
            Err(e) => self.status = format!("Couldn't trash {name}: {e:#}"),
        }
        self.refresh();
    }

    pub fn restore_keys(&mut self, keys: &[String]) -> usize {
        let Some(lib) = self.lib.clone() else {
            return 0;
        };
        let mut n = 0;
        let mut err = None;
        for k in keys {
            match lib.restore(k) {
                Ok(_) => n += 1,
                Err(e) => err = Some(format!("{e:#}")),
            }
        }
        self.status = match err {
            None => format!("Restored {n} photo(s)"),
            Some(e) => format!("Restored {n}; failed: {e}"),
        };
        self.marks.clear();
        self.refresh();
        n
    }

    pub fn restore_last(&mut self) {
        let keys = std::mem::take(&mut self.last_trashed);
        if keys.is_empty() {
            self.status = "Nothing to undo".into();
        } else {
            self.restore_keys(&keys);
        }
    }

    pub fn trash_keys_of(items: &[Item]) -> Vec<String> {
        items
            .iter()
            .filter_map(|i| match &i.r {
                ItemRef::Trashed { key, .. } => Some(key.clone()),
                _ => None,
            })
            .collect()
    }

    /// Run a confirmed permanent delete.
    pub fn run_confirm(&mut self, c: Confirm) {
        let Some(lib) = self.lib.clone() else { return };
        let res = match c {
            Confirm::Purge(keys) => keys.iter().try_fold(0, |n, k| lib.purge(k).map(|_| n + 1)),
            Confirm::EmptyTrash => lib.empty_trash(),
        };
        self.status = match res {
            Ok(n) => format!("Deleted {n} photo(s) permanently"),
            Err(e) => format!("Delete failed: {e:#}"),
        };
        self.last_trashed.clear();
        self.marks.clear();
        self.thumbs.retain(|p, _| p.exists());
        self.refresh();
    }

    /// Sidebar rows call this: highlight and remember the row under a dragged photo.
    pub fn drop_zone(&mut self, ui: &Ui, rect: egui::Rect, target: DropTarget) {
        if self.dragging.is_none() {
            return;
        }
        if ui
            .input(|i| i.pointer.hover_pos())
            .is_some_and(|p| rect.contains(p))
        {
            ui.painter()
                .rect_filled(rect, 3.0, theme::ACCENT.gamma_multiply(0.25));
            ui.painter().rect_stroke(
                rect.shrink(1.0),
                3.0,
                egui::Stroke::new(2.0, theme::ACCENT),
                egui::StrokeKind::Inside,
            );
            self.drop_hover = Some(target);
        }
    }

    /// Draw the drag ghost; on release over a sidebar row, move / file / trash the photos.
    fn finish_drag(&mut self, ctx: &egui::Context) {
        let Some(origs) = self.dragging.clone() else {
            return;
        };
        if ctx.input(|i| i.pointer.primary_down()) {
            if let Some(p) = ctx.input(|i| i.pointer.hover_pos()) {
                let painter = ctx.layer_painter(egui::LayerId::new(
                    egui::Order::Tooltip,
                    egui::Id::new("drag_ghost"),
                ));
                let text = match (&self.drop_hover, origs.len()) {
                    (Some(DropTarget::Collection(c)), n) => format!("Move {n} to {c}"),
                    (Some(DropTarget::Trash), n) => format!("Move {n} to trash"),
                    (None, 1) => "1 photo".into(),
                    (None, n) => format!("{n} photos"),
                };
                let galley =
                    painter.layout_no_wrap(text, egui::FontId::proportional(12.5), theme::BOX_TEXT);
                let r = egui::Rect::from_min_size(
                    p + egui::vec2(14.0, 10.0),
                    galley.size() + egui::vec2(16.0, 10.0),
                );
                painter.rect_filled(r, 4.0, theme::BOX);
                painter.rect_filled(
                    egui::Rect::from_min_max(r.min, egui::pos2(r.max.x, r.min.y + 3.0)),
                    2.0,
                    theme::ACCENT,
                );
                painter.galley(r.min + egui::vec2(8.0, 6.0), galley, theme::BOX_TEXT);
            }
            ctx.set_cursor_icon(egui::CursorIcon::Grabbing);
            return;
        }
        self.dragging = None;
        let Some(target) = self.drop_hover.take() else {
            return;
        };
        let items: Vec<Item> = self
            .items
            .iter()
            .filter(|i| origs.contains(&i.orig))
            .cloned()
            .collect();
        match target {
            DropTarget::Collection(c) => self.file_items(ctx, items, &c),
            DropTarget::Trash => self.trash_items(items),
        }
    }

    /// Leaving the editor / collage view with unsaved changes: stay, and ask.
    fn guard_view(&mut self, prev: View) {
        if prev == self.view || self.leave.is_some() {
            return;
        }
        let dirty = match prev {
            View::Edit => self.editor_dirty(),
            View::Collage => self.collage_dirty(),
            _ => false,
        };
        if dirty {
            self.leave = Some(Leave::View {
                from: prev,
                to: self.view,
            });
            self.view = prev;
        }
    }

    /// The document the pending `leave` is waiting on: (is_photo, title).
    fn leave_blocker(&self) -> Option<(bool, String)> {
        let (ed, col) = self.leave.as_ref()?.abandons();
        if ed && self.editor_dirty() {
            return self.editor.as_ref().map(|e| (true, e.item.title()));
        }
        if col && self.collage_dirty() {
            return self.collage.as_ref().map(|c| (false, c.doc.name.clone()));
        }
        None
    }

    pub fn leave_choice(&mut self, ctx: &egui::Context, choice: LeaveChoice) {
        let Some(l) = self.leave.clone() else { return };
        match (choice, self.leave_blocker()) {
            (LeaveChoice::Cancel, _) => {
                self.leave = None;
                self.status = "Still editing — ⌘S saves".into();
            }
            (LeaveChoice::Save, Some((true, _))) => self.save_current(ctx),
            (LeaveChoice::Save, Some((false, _))) => self.save_collage(),
            (LeaveChoice::Discard, Some((true, _))) => self.discard_editor(),
            (LeaveChoice::Discard, Some((false, _))) => self.discard_collage(),
            (_, None) => {}
        }
        // Another document may still need an answer (e.g. quitting with both open).
        if self.leave.is_some() && self.leave_blocker().is_none() {
            self.leave = None;
            self.run_leave(ctx, l);
        }
    }

    fn run_leave(&mut self, ctx: &egui::Context, l: Leave) {
        match l {
            Leave::View { to, .. } => {
                self.view = to;
                if to == View::Library {
                    self.refresh();
                }
            }
            Leave::OpenItem(it) => self.open_item(ctx, it),
            Leave::OpenCollage(p) => self.open_collage(p),
            Leave::NewCollage(name) => self.new_collage_named(name.as_deref()),
            Leave::CloseLibrary => self.close_library(),
            Leave::SetLibrary(p) => self.set_library(p),
            Leave::Quit => {
                self.quitting = true;
                ctx.send_viewport_cmd(egui::ViewportCommand::Close);
            }
        }
    }

    fn leave_modal(&mut self, ctx: &egui::Context) {
        if self.leave.is_none() {
            return;
        }
        let Some((photo, title)) = self.leave_blocker() else {
            // Nothing unsaved any more (saved elsewhere): just go.
            self.leave_choice(ctx, LeaveChoice::Discard);
            return;
        };
        let filed = photo
            && self
                .editor
                .as_ref()
                .is_some_and(|e| matches!(e.item.r, ItemRef::Filed { .. }));
        let mut choice = None;
        let m = egui::Modal::new(egui::Id::new("leave")).show(ctx, |ui| {
            ui.set_width(380.0);
            ui.label(egui::RichText::new(format!("Save changes to {title}?")).strong().size(15.0).color(theme::TEXT));
            ui.add_space(4.0);
            ui.label(
                egui::RichText::new(if filed {
                    "Saving writes the recipe and renders the full-resolution edit. Don't save keeps the last saved version."
                } else {
                    "Your changes haven't been saved. Don't save keeps the last saved version."
                })
                .color(theme::TEXT_DIM),
            );
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let save = egui::Button::new(egui::RichText::new("Save").color(egui::Color32::WHITE)).fill(theme::ACCENT);
                if ui.add(save).on_hover_text("Enter").clicked() {
                    choice = Some(LeaveChoice::Save);
                }
                if ui.button("Don't save").on_hover_text("N").clicked() {
                    choice = Some(LeaveChoice::Discard);
                }
                if ui.button("Cancel").on_hover_text("Esc").clicked() {
                    choice = Some(LeaveChoice::Cancel);
                }
            });
        });
        if m.should_close() && choice.is_none() {
            choice = Some(LeaveChoice::Cancel);
        }
        if let Some(c) = choice {
            self.leave_choice(ctx, c);
            ctx.request_repaint();
        }
    }

    fn confirm_modal(&mut self, ctx: &egui::Context) {
        let Some(c) = self.confirm.clone() else {
            return;
        };
        let (title, body) = match &c {
            Confirm::Purge(k) => (
                format!("Delete {} photo(s) forever?", k.len()),
                "The originals, edits and recipes are removed from disk. This can't be undone.",
            ),
            Confirm::EmptyTrash => (
                format!("Empty the trash ({} photos)?", self.trash_count),
                "Everything in the trash is removed from disk. This can't be undone.",
            ),
        };
        let mut choice = None;
        let m = egui::Modal::new(egui::Id::new("confirm")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.label(
                egui::RichText::new(title)
                    .strong()
                    .size(15.0)
                    .color(theme::TEXT),
            );
            ui.add_space(4.0);
            ui.label(egui::RichText::new(body).color(theme::TEXT_DIM));
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                let del = egui::Button::new(
                    egui::RichText::new("Delete forever").color(egui::Color32::WHITE),
                )
                .fill(theme::RED);
                if ui.add(del).on_hover_text("Enter").clicked() {
                    choice = Some(true);
                }
                if ui.button("Cancel").on_hover_text("Esc").clicked() {
                    choice = Some(false);
                }
            });
        });
        if m.should_close() && choice.is_none() {
            choice = Some(false);
        }
        match choice {
            Some(true) => {
                self.confirm = None;
                self.run_confirm(c);
            }
            Some(false) => self.confirm = None,
            None => {}
        }
    }

    // ------------------------------------------------------------- collage lifecycle

    pub fn new_collage(&mut self) {
        self.new_collage_named(None);
    }

    /// New collage; with a name its file is `<name>.json`, else `collage-NNNN.json`.
    pub fn new_collage_named(&mut self, name: Option<&str>) {
        if self.collage_dirty() {
            self.leave = Some(Leave::NewCollage(name.map(String::from)));
            return;
        }
        let Some(lib) = &self.lib else { return };
        let name = name
            .map(str::trim)
            .filter(|n| !collage::file_stem_for(n).is_empty());
        let path = match name {
            Some(n) => collage::path_for_name(&lib.collages_dir(), n),
            None => collage::next_path(&lib.collages_dir()),
        };
        let name = name.map(String::from).unwrap_or_else(|| {
            path.file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("collage")
                .to_string()
        });
        let doc = Collage::new(&name, collage::CANVAS_PRESETS[1]);
        let _ = collage::save(&path, &doc);
        self.open_collage_doc(path, doc);
        self.refresh();
    }

    pub fn open_collage(&mut self, path: PathBuf) {
        if self
            .collage
            .as_ref()
            .is_some_and(|c| c.dirty() && c.path != path)
        {
            self.leave = Some(Leave::OpenCollage(path));
            return;
        }
        if let Some(c) = &self.collage
            && c.path == path
        {
            self.view = View::Collage; // already open: keep in-memory edits
            return;
        }
        match collage::load(&path) {
            Ok(doc) => self.open_collage_doc(path, doc),
            Err(e) => self.status = format!("Can't open collage: {e:#}"),
        }
    }

    fn open_collage_doc(&mut self, path: PathBuf, doc: Collage) {
        let key = serde_json::to_string(&doc).unwrap_or_default();
        self.collage = Some(CollageEd {
            path,
            history: History::new(doc.clone(), key),
            doc: doc.clone(),
            selected: None,
            tool: CTool::Move,
            lasso: vec![],
            rect_start: None,
            sources: HashMap::new(),
            pending: HashSet::new(),
            preview: None,
            preview_key: String::new(),
            pick_collection: self.collections.first().cloned().unwrap_or_default(),
            drag: None,
            exporting: false,
            last_export: None,
            zoom: Default::default(),
            saved: doc.clone(),
            layer_drag: None,
        });
        self.view = View::Collage;
    }

    pub fn save_collage(&mut self) {
        if let Some(c) = &mut self.collage {
            match collage::save(&c.path, &c.doc) {
                Ok(()) => c.saved = c.doc.clone(),
                Err(e) => self.status = format!("Couldn't save collage: {e:#}"),
            }
        }
    }

    // ------------------------------------------------------------- frame

    pub fn save_current(&mut self, ctx: &egui::Context) {
        if let Some(ed) = &self.editor {
            if matches!(ed.item.r, ItemRef::Filed { .. }) {
                self.export_current(ctx);
            } else {
                self.save_editor_sidecar();
                self.status = "Recipe saved (file into a collection to render the edit)".into();
            }
        }
    }

    pub fn undo(&mut self) {
        match self.view {
            View::Edit => {
                if let Some(ed) = &mut self.editor
                    && let Some(d) = ed.history.undo(|d| d.key())
                {
                    ed.doc = d;
                }
            }
            View::Collage => {
                if let Some(c) = &mut self.collage
                    && let Some(d) = c
                        .history
                        .undo(|d| serde_json::to_string(d).unwrap_or_default())
                {
                    c.doc = d;
                }
            }
            View::Library => self.restore_last(),
            View::Settings => {}
        }
    }

    pub fn redo(&mut self) {
        match self.view {
            View::Edit => {
                if let Some(ed) = &mut self.editor
                    && let Some(d) = ed.history.redo(|d| d.key())
                {
                    ed.doc = d;
                }
            }
            View::Collage => {
                if let Some(c) = &mut self.collage
                    && let Some(d) = c
                        .history
                        .redo(|d| serde_json::to_string(d).unwrap_or_default())
                {
                    c.doc = d;
                }
            }
            View::Library | View::Settings => {}
        }
    }

    /// Commit undo snapshots + autosave.
    fn track_history(&mut self, ctx: &egui::Context) {
        // Undo snapshots only: nothing is written to disk until the user saves.
        let interacting = ctx.input(|i| i.pointer.any_down());
        if let Some(ed) = &mut self.editor {
            let key = ed.doc.key();
            if ed.history.observe(&ed.doc, key, interacting)
                && matches!(ed.item.r, ItemRef::Filed { .. })
            {
                ed.stale = true;
            }
        }
        if let Some(c) = &mut self.collage {
            let key = serde_json::to_string(&c.doc).unwrap_or_default();
            c.history.observe(&c.doc, key, interacting);
        }
    }

    fn import_dropped(&mut self, ctx: &egui::Context) {
        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .collect()
        });
        if dropped.is_empty() {
            return;
        }
        self.import(dropped);
    }

    pub fn import(&mut self, files: Vec<PathBuf>) {
        let Some(lib) = &self.lib else { return };
        match lib.import(&files) {
            Ok(n) => self.status = format!("Imported {n} file(s) into Inbox"),
            Err(e) => self.status = format!("Import failed: {e:#}"),
        }
        self.source = Source::Inbox;
        self.view = View::Library;
        self.refresh();
    }

    pub fn frame(&mut self, ui: &mut Ui) {
        let ctx = ui.ctx().clone();
        if self.ctx.is_none() {
            self.ctx = Some(ctx.clone());
        }
        if !self.themed {
            theme::apply(&ctx);
            self.themed = true;
        }
        self.handle_messages(&ctx);
        self.import_dropped(&ctx);
        let prev_view = self.view;
        self.drop_hover = None; // recomputed by the sidebar rows this frame
        // Closing the window with unsaved edits asks first.
        if ctx.input(|i| i.viewport().close_requested()) && !self.quitting && self.any_dirty() {
            ctx.send_viewport_cmd(egui::ViewportCommand::CancelClose);
            self.leave = Some(Leave::Quit);
        }
        if self.confirm.is_some() || self.leave.is_some() {
            // A dialog is open: Enter = confirm / save, Esc = cancel, N = don't save.
            match modal_key(&ctx) {
                Some(ModalKey::Yes) => {
                    if let Some(c) = self.confirm.take() {
                        self.run_confirm(c);
                    } else {
                        self.leave_choice(&ctx, LeaveChoice::Save);
                    }
                }
                Some(ModalKey::No) if self.leave.is_some() => {
                    self.leave_choice(&ctx, LeaveChoice::Discard);
                }
                Some(ModalKey::No) => {}
                Some(ModalKey::Cancel) => {
                    self.confirm = None;
                    self.leave_choice(&ctx, LeaveChoice::Cancel);
                }
                None => {}
            }
        } else {
            let acts = self.capture_keys(&ctx);
            for a in acts {
                self.act(&ctx, a);
            }
        }
        self.guard_view(prev_view);

        // Watch the Inbox (cheap directory listing every 2 s).
        let now = ctx.input(|i| i.time);
        if now - self.last_refresh > 2.0 && self.view == View::Library {
            self.last_refresh = now;
            self.refresh();
        }
        if !self.worker.is_sync() {
            ctx.request_repaint_after(std::time::Duration::from_secs(2));
        }

        egui::Panel::top("toolbar")
            .exact_size(crate::ui::shell::TOOLBAR_H)
            .frame(
                egui::Frame::new()
                    .fill(theme::TOOLBAR)
                    .inner_margin(egui::Margin::symmetric(8, 0)),
            )
            .show(ui, |ui| self.toolbar(ui));
        self.guard_view(prev_view);
        egui::Panel::bottom("status")
            .exact_size(crate::ui::editor::BAR_H)
            .frame(
                egui::Frame::new()
                    .fill(theme::TOOLBAR)
                    .inner_margin(egui::Margin::symmetric(0, 0)),
            )
            .show(ui, |ui| self.statusline(ui));

        if self.lib.is_none() {
            egui::CentralPanel::default()
                .frame(egui::Frame::new().fill(theme::CANVAS))
                .show(ui, |ui| self.startup_picker(ui));
            self.help_overlay(&ctx);
            return;
        }

        let compact = self.settings.sidebar_compact;
        let mut side = egui::Panel::left(if compact {
            "sidebar_compact"
        } else {
            "sidebar"
        })
        .resizable(!compact)
        .default_size(210.0)
        .show_separator_line(true)
        .frame(
            egui::Frame::new()
                .fill(if compact { theme::RAIL } else { theme::SIDEBAR })
                .inner_margin(egui::Margin::symmetric(
                    if compact { 4 } else { 0 },
                    if compact { 6 } else { 0 },
                )),
        );
        if compact {
            side = side.exact_size(crate::ui::shell::COMPACT_W);
        }
        side.show(ui, |ui| {
            if compact {
                self.sidebar_compact(ui)
            } else {
                self.sidebar(ui)
            }
        });

        match self.view {
            View::Library => {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::new()
                            .fill(theme::CANVAS)
                            .inner_margin(egui::Margin::same(14)),
                    )
                    .show(ui, |ui| self.library_grid(ui));
            }
            View::Edit => self.edit_view(ui),
            View::Collage => self.collage_view(ui),
            View::Settings => {
                egui::CentralPanel::default()
                    .frame(
                        egui::Frame::new()
                            .fill(theme::CANVAS)
                            .inner_margin(egui::Margin::same(16)),
                    )
                    .show(ui, |ui| self.settings_view(ui));
            }
        }
        self.finish_drag(&ctx);
        self.help_overlay(&ctx);
        self.confirm_modal(&ctx);
        self.guard_view(prev_view);
        self.leave_modal(&ctx);
        self.text_focus = ctx.text_edit_focused();

        self.track_history(&ctx);
        if self.worker.has_queued() {
            ctx.request_repaint();
        }
    }
}

impl eframe::App for FoundApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        self.frame(ui);
    }

    fn on_exit(&mut self) {
        // Unsaved edits were either saved or discarded in the quit prompt.
        self.save_settings();
    }
}

pub fn path_key(p: &std::path::Path) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    p.hash(&mut h);
    if let Ok(m) = std::fs::metadata(p) {
        m.len().hash(&mut h);
        m.modified().ok().hash(&mut h);
    }
    h.finish()
}

pub fn gen_next(a: &AtomicU64) -> u64 {
    a.fetch_add(1, Ordering::SeqCst) + 1
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LeaveChoice {
    Save,
    Discard,
    Cancel,
}

enum ModalKey {
    Yes,
    No,
    Cancel,
}

/// Keys for an open dialog (consumed so nothing else reacts).
fn modal_key(ctx: &egui::Context) -> Option<ModalKey> {
    ctx.input_mut(|i| {
        let k = if i.consume_key(egui::Modifiers::NONE, egui::Key::Enter) {
            Some(ModalKey::Yes)
        } else if i.consume_key(egui::Modifiers::NONE, egui::Key::Escape) {
            Some(ModalKey::Cancel)
        } else if i.consume_key(egui::Modifiers::NONE, egui::Key::N)
            || i.consume_key(egui::Modifiers::NONE, egui::Key::D)
        {
            Some(ModalKey::No)
        } else {
            None
        };
        i.events
            .retain(|e| !matches!(e, egui::Event::Text(_) | egui::Event::Key { .. }));
        k
    })
}
