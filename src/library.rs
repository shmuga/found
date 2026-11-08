//! Library on disk: one folder per collection plus an Inbox. The root and the Inbox /
//! collages / presets folders come from user settings ([`crate::settings::Layout`]); the tree
//! below shows the defaults.
//!
//! ```text
//! library/
//!   Inbox/                    scans to process (can be any folder, e.g. the scanner's output)
//!   .found/inbox-edits/       in-progress recipes for Inbox items
//!   Coins/
//!     0001.orig.jpg           original (moved from Inbox, never modified)
//!     0001.edit.jpg           rendered edit, JPEG q95 full-res
//!     0001.json               sidecar: recipe graph + tags
//!   _collages/                collage-0001.json + .jpg
//!   _presets/                 user presets
//!   .found/trash/<stamp>/     local trash: moved files + meta.json (restore puts them back)
//! ```

use crate::graph::{self, Graph};
use anyhow::{Context, Result, bail};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

use crate::settings::Layout;

/// App-private state inside the library root (hidden).
const PRIVATE: &str = ".found";
/// Name of the private folder before the app was called Found (migrated on open).
const OLD_PRIVATE: &str = ".scanner";

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Sidecar {
    #[serde(default = "one")]
    pub version: u32,
    #[serde(default)]
    pub source_name: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default = "graph::default_graph")]
    pub graph: Graph,
    #[serde(default)]
    pub modified: u64,
}

fn one() -> u32 {
    1
}

impl Default for Sidecar {
    fn default() -> Self {
        Self {
            version: 1,
            source_name: String::new(),
            tags: vec![],
            graph: graph::default_graph(),
            modified: 0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub enum ItemRef {
    Inbox(String),
    Filed {
        collection: String,
        id: String,
    },
    /// In the local trash: folder name inside `.found/trash` + the title it had.
    Trashed {
        key: String,
        title: String,
    },
}

/// Where a trashed photo came from (stored in the trash entry's `meta.json`).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TrashFrom {
    Inbox {
        name: String,
    },
    Filed {
        collection: String,
        id: String,
    },
    /// A collage document (`name` = file stem in the collages folder) + its exported JPEG.
    Collage {
        name: String,
    },
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TrashMeta {
    pub from: TrashFrom,
    /// Unix seconds.
    pub deleted: u64,
    /// Original file extension of the scan.
    pub ext: String,
}

const TRASH_ORIG: &str = "orig";
const TRASH_EDIT: &str = "edit.jpg";
const TRASH_SIDECAR: &str = "sidecar.json";

#[derive(Clone, Debug)]
pub struct Item {
    pub r: ItemRef,
    pub orig: PathBuf,
    pub edit: Option<PathBuf>,
    pub sidecar: PathBuf,
    /// File modification time of the original (unix seconds) — the scan date.
    pub modified: i64,
}

fn mtime(p: &Path) -> i64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

impl Item {
    pub fn title(&self) -> String {
        match &self.r {
            ItemRef::Inbox(name) => name.clone(),
            ItemRef::Filed { collection, id } => format!("{collection}/{id}"),
            ItemRef::Trashed { title, .. } => title.clone(),
        }
    }

    pub fn is_trashed(&self) -> bool {
        matches!(self.r, ItemRef::Trashed { .. })
    }

    /// Best image for display / collage: the rendered edit if present, else the original.
    pub fn display_path(&self) -> &Path {
        self.edit.as_deref().unwrap_or(&self.orig)
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[derive(Clone, Debug)]
pub struct Library {
    pub root: PathBuf,
    pub inbox: PathBuf,
    pub collages: PathBuf,
    pub presets: PathBuf,
}

impl Library {
    /// Open with the default layout.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        Self::open_with(root, &Layout::default())
    }

    pub fn open_with(root: impl Into<PathBuf>, layout: &Layout) -> Result<Self> {
        let root: PathBuf = root.into();
        if root.as_os_str().is_empty() {
            bail!("no library folder given");
        }
        let lib = Self {
            inbox: Layout::resolve(&root, &layout.inbox),
            collages: Layout::resolve(&root, &layout.collages),
            presets: Layout::resolve(&root, &layout.presets),
            root,
        };
        // Libraries made before the rename keep their recipes / trash in `.scanner`: move it.
        let (old, new) = (lib.root.join(OLD_PRIVATE), lib.root.join(PRIVATE));
        if old.is_dir() && !new.exists() {
            std::fs::rename(&old, &new)
                .with_context(|| format!("move {} to {}", old.display(), new.display()))?;
        }
        for d in [&lib.root, &lib.inbox, &lib.collages, &lib.presets] {
            std::fs::create_dir_all(d).with_context(|| format!("create {}", d.display()))?;
        }
        Ok(lib)
    }

    pub fn inbox_dir(&self) -> PathBuf {
        self.inbox.clone()
    }
    pub fn collages_dir(&self) -> PathBuf {
        self.collages.clone()
    }
    pub fn presets_dir(&self) -> PathBuf {
        self.presets.clone()
    }

    /// Folders inside the root that are not collections.
    fn is_special(&self, dir: &Path) -> bool {
        let same = |a: &Path, b: &Path| {
            a == b
                || (a.canonicalize().ok().is_some()
                    && a.canonicalize().ok() == b.canonicalize().ok())
        };
        [&self.inbox, &self.collages, &self.presets]
            .iter()
            .any(|s| same(dir, s))
    }

    pub fn collections(&self) -> Vec<String> {
        let mut v: Vec<String> = std::fs::read_dir(&self.root)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().is_dir() && !self.is_special(&e.path()))
            .filter_map(|e| e.file_name().to_str().map(String::from))
            .filter(|n| !n.starts_with('_') && !n.starts_with('.'))
            .collect();
        v.sort_by_key(|a| a.to_lowercase());
        v
    }

    pub fn create_collection(&self, name: &str) -> Result<String> {
        let name = name.trim();
        if name.is_empty()
            || name.starts_with(['_', '.'])
            || name.contains(['/', '\\'])
            || self.is_special(&self.root.join(name))
        {
            bail!("invalid collection name");
        }
        std::fs::create_dir_all(self.root.join(name))?;
        Ok(name.to_string())
    }

    fn check_collection_name(&self, name: &str) -> Result<()> {
        if name.is_empty()
            || name.starts_with(['_', '.'])
            || name.contains(['/', '\\', ':'])
            || self.is_special(&self.root.join(name))
        {
            bail!("invalid collection name");
        }
        Ok(())
    }

    /// Rename a collection folder. Collage layers and trashed photos that point into it are
    /// updated so nothing breaks. Returns the new name.
    pub fn rename_collection(&self, old: &str, new: &str) -> Result<String> {
        let new = new.trim();
        self.check_collection_name(new)?;
        let from = self.root.join(old);
        if old.is_empty() || !from.is_dir() || self.is_special(&from) {
            bail!("no collection {old}");
        }
        if new == old {
            return Ok(new.to_string());
        }
        let to = self.root.join(new);
        // Case-only renames are fine on case-insensitive volumes; anything else must be free.
        if to.exists() && !new.eq_ignore_ascii_case(old) {
            bail!("{new} already exists");
        }
        std::fs::rename(&from, &to)?;
        // Collage documents reference sources as "Collection/0001.edit.jpg".
        let (old_p, new_p) = (format!("{old}/"), format!("{new}/"));
        for path in crate::collage::list(&self.collages) {
            if let Ok(mut c) = crate::collage::load(&path) {
                let mut changed = false;
                for l in &mut c.layers {
                    if let Some(rest) = l.source.strip_prefix(&old_p) {
                        l.source = format!("{new_p}{rest}");
                        changed = true;
                    }
                }
                if changed {
                    crate::collage::save(&path, &c)?;
                }
            }
        }
        // Trashed photos restore into the renamed collection.
        for e in std::fs::read_dir(self.trash_dir())
            .into_iter()
            .flatten()
            .flatten()
        {
            let Some(key) = e.file_name().to_str().map(String::from) else {
                continue;
            };
            if let Some(mut m) = self.trash_meta(&key)
                && let TrashFrom::Filed { collection, .. } = &mut m.from
                && collection == old
            {
                *collection = new.to_string();
                std::fs::write(
                    e.path().join("meta.json"),
                    serde_json::to_string_pretty(&m)?,
                )?;
            }
        }
        Ok(new.to_string())
    }

    pub fn inbox_items(&self) -> Vec<Item> {
        let dir = self.inbox_dir();
        let edits = self.root.join(PRIVATE).join("inbox-edits");
        let mut v: Vec<Item> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && crate::io::is_image(p))
            .filter_map(|p| {
                let name = p.file_name()?.to_str()?.to_string();
                Some(Item {
                    sidecar: edits.join(format!("{name}.json")),
                    r: ItemRef::Inbox(name),
                    modified: mtime(&p),
                    orig: p,
                    edit: None,
                })
            })
            .collect();
        v.sort_by(|a, b| a.orig.cmp(&b.orig));
        v
    }

    pub fn items(&self, collection: &str) -> Vec<Item> {
        let dir = self.root.join(collection);
        let mut v: Vec<Item> = std::fs::read_dir(&dir)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter_map(|p| {
                let name = p.file_name()?.to_str()?;
                let (id, _ext) = name.split_once(".orig.")?;
                let edit = dir.join(format!("{id}.edit.jpg"));
                Some(Item {
                    r: ItemRef::Filed {
                        collection: collection.into(),
                        id: id.into(),
                    },
                    sidecar: dir.join(format!("{id}.json")),
                    edit: edit.exists().then_some(edit),
                    modified: mtime(&p),
                    orig: p.clone(),
                })
            })
            .collect();
        v.sort_by(|a, b| a.orig.cmp(&b.orig));
        v
    }

    pub fn all_items(&self) -> Vec<Item> {
        self.collections()
            .iter()
            .flat_map(|c| self.items(c))
            .collect()
    }

    pub fn item(&self, r: &ItemRef) -> Option<Item> {
        match r {
            ItemRef::Inbox(name) => self
                .inbox_items()
                .into_iter()
                .find(|i| matches!(&i.r, ItemRef::Inbox(n) if n == name)),
            ItemRef::Filed { collection, .. } => {
                self.items(collection).into_iter().find(|i| &i.r == r)
            }
            ItemRef::Trashed { .. } => self.trash_items().into_iter().find(|i| &i.r == r),
        }
    }

    pub fn read_sidecar(&self, item: &Item) -> Sidecar {
        std::fs::read_to_string(&item.sidecar)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_else(|| Sidecar {
                source_name: item
                    .orig
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .into(),
                ..Default::default()
            })
    }

    pub fn write_sidecar(&self, item: &Item, sc: &Sidecar) -> Result<()> {
        if let Some(d) = item.sidecar.parent() {
            std::fs::create_dir_all(d)?;
        }
        let mut sc = sc.clone();
        sc.modified = now();
        let tmp = item.sidecar.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(&sc)?)?;
        std::fs::rename(tmp, &item.sidecar)?;
        Ok(())
    }

    pub fn next_id(&self, collection: &str) -> String {
        let max = self
            .items(collection)
            .iter()
            .filter_map(|i| match &i.r {
                ItemRef::Filed { id, .. } => id.parse::<u32>().ok(),
                _ => None,
            })
            .max()
            .unwrap_or(0);
        format!("{:04}", max + 1)
    }

    /// Move an Inbox scan into a collection with the next sequential id. Returns the new item
    /// (without an edit file; the caller renders and writes it).
    pub fn file_into(&self, inbox_item: &Item, collection: &str, sc: &Sidecar) -> Result<Item> {
        let ItemRef::Inbox(name) = &inbox_item.r else {
            bail!("not an inbox item")
        };
        let dir = self.root.join(collection);
        if !dir.is_dir() {
            bail!("collection {collection} does not exist");
        }
        let id = self.next_id(collection);
        let ext = inbox_item
            .orig
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg")
            .to_ascii_lowercase();
        let orig = dir.join(format!("{id}.orig.{ext}"));
        if orig.exists() {
            bail!("{} already exists", orig.display());
        }
        move_file(&inbox_item.orig, &orig)?;
        let item = Item {
            r: ItemRef::Filed {
                collection: collection.into(),
                id: id.clone(),
            },
            modified: mtime(&orig),
            orig,
            edit: None,
            sidecar: dir.join(format!("{id}.json")),
        };
        let mut sc = sc.clone();
        if sc.source_name.is_empty() {
            sc.source_name = name.clone();
        }
        self.write_sidecar(&item, &sc)?;
        let _ = std::fs::remove_file(&inbox_item.sidecar);
        Ok(item)
    }

    /// Move a filed photo (original, rendered edit, recipe) into another collection with that
    /// collection's next id. Collages using it are updated. Returns the moved item and the
    /// (old → new) library-relative paths that changed.
    pub fn move_to(&self, item: &Item, collection: &str) -> Result<(Item, Vec<(String, String)>)> {
        let ItemRef::Filed {
            collection: from, ..
        } = &item.r
        else {
            bail!("only filed photos can be moved; file Inbox scans instead")
        };
        if from == collection {
            bail!("already in {collection}");
        }
        let dir = self.root.join(collection);
        if !dir.is_dir() {
            bail!("collection {collection} does not exist");
        }
        let id = self.next_id(collection);
        let ext = item
            .orig
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg")
            .to_ascii_lowercase();
        let orig = dir.join(format!("{id}.orig.{ext}"));
        let edit = dir.join(format!("{id}.edit.jpg"));
        let sidecar = dir.join(format!("{id}.json"));
        if orig.exists() || sidecar.exists() {
            bail!("{} already exists", orig.display());
        }
        let rel = |p: &Path| {
            p.strip_prefix(&self.root)
                .unwrap_or(p)
                .to_string_lossy()
                .to_string()
        };
        let mut moved = vec![(rel(&item.orig), rel(&orig))];
        move_file(&item.orig, &orig)?;
        let old_edit = item
            .edit
            .clone()
            .or_else(|| self.edit_path(item))
            .filter(|p| p.exists());
        if let Some(e) = &old_edit {
            moved.push((rel(e), rel(&edit)));
            move_file(e, &edit)?;
        }
        if item.sidecar.exists() {
            move_file(&item.sidecar, &sidecar)?;
        }
        self.retarget_collages(&moved)?;
        let new = Item {
            r: ItemRef::Filed {
                collection: collection.into(),
                id,
            },
            modified: mtime(&orig),
            orig,
            edit: old_edit.map(|_| edit),
            sidecar,
        };
        Ok((new, moved))
    }

    /// Point collage layers at moved files (`map` = library-relative old → new paths).
    pub fn retarget_collages(&self, map: &[(String, String)]) -> Result<()> {
        for path in crate::collage::list(&self.collages) {
            if let Ok(mut c) = crate::collage::load(&path) {
                let mut changed = false;
                for l in &mut c.layers {
                    if let Some((_, new)) = map.iter().find(|(old, _)| *old == l.source) {
                        l.source = new.clone();
                        changed = true;
                    }
                }
                if changed {
                    crate::collage::save(&path, &c)?;
                }
            }
        }
        Ok(())
    }

    /// Copy a photo (Inbox or filed) into `collection` as a new item with the next id. The
    /// source is left untouched; `sc` becomes the copy's recipe (render its edit separately).
    pub fn copy_into(&self, src: &Item, collection: &str, sc: &Sidecar) -> Result<Item> {
        if src.is_trashed() {
            bail!("restore it from the trash first");
        }
        let dir = self.root.join(collection);
        if !dir.is_dir() {
            bail!("collection {collection} does not exist");
        }
        let id = self.next_id(collection);
        let ext = src
            .orig
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg")
            .to_ascii_lowercase();
        let orig = dir.join(format!("{id}.orig.{ext}"));
        if orig.exists() {
            bail!("{} already exists", orig.display());
        }
        std::fs::copy(&src.orig, &orig)?;
        let item = Item {
            r: ItemRef::Filed {
                collection: collection.into(),
                id: id.clone(),
            },
            modified: mtime(&orig),
            orig,
            edit: None,
            sidecar: dir.join(format!("{id}.json")),
        };
        let mut sc = sc.clone();
        if sc.source_name.is_empty() {
            sc.source_name = src
                .orig
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or_default()
                .into();
        }
        self.write_sidecar(&item, &sc)?;
        Ok(item)
    }

    pub fn edit_path(&self, item: &Item) -> Option<PathBuf> {
        match &item.r {
            ItemRef::Filed { collection, id } => {
                Some(self.root.join(collection).join(format!("{id}.edit.jpg")))
            }
            ItemRef::Inbox(_) | ItemRef::Trashed { .. } => None,
        }
    }

    /// PDFs waiting in the Inbox.
    pub fn inbox_pdfs(&self) -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(&self.inbox)
            .into_iter()
            .flatten()
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_file() && crate::pdf::is_pdf(p))
            .collect();
        v.sort();
        v
    }

    /// Extract a PDF's page images into the Inbox and move the PDF to `.found/pdf-originals/`
    /// (kept, never deleted). Each extracted scan gets a sidecar naming its source page.
    pub fn extract_pdf(&self, pdf: &Path) -> Result<Vec<PathBuf>> {
        let pages = crate::pdf::extract(pdf, &self.inbox)?;
        let keep_dir = self.root.join(PRIVATE).join("pdf-originals");
        std::fs::create_dir_all(&keep_dir)?;
        let name = pdf
            .file_name()
            .context("pdf name")?
            .to_string_lossy()
            .to_string();
        let mut dest = keep_dir.join(&name);
        let mut k = 1;
        while dest.exists() {
            dest = keep_dir.join(format!("{k}-{name}"));
            k += 1;
        }
        move_file(pdf, &dest)?;
        let multi = pages.len() > 1;
        let mut out = vec![];
        for p in pages {
            if let Some(item) = self.inbox_items().into_iter().find(|i| i.orig == p.path) {
                let sc = Sidecar {
                    source_name: if multi {
                        format!("{name} · page {}", p.page)
                    } else {
                        name.clone()
                    },
                    ..Default::default()
                };
                self.write_sidecar(&item, &sc)?;
            }
            out.push(p.path);
        }
        Ok(out)
    }

    /// Copy external files into the Inbox (never overwrites; adds a numeric suffix).
    pub fn import(&self, files: &[PathBuf]) -> Result<usize> {
        let mut n = 0;
        for f in files
            .iter()
            .filter(|f| crate::io::is_image(f) || crate::pdf::is_pdf(f))
        {
            let name = f.file_name().context("no file name")?;
            let mut dest = self.inbox_dir().join(name);
            let stem = f
                .file_stem()
                .and_then(|s| s.to_str())
                .unwrap_or("scan")
                .to_string();
            let ext = f
                .extension()
                .and_then(|s| s.to_str())
                .unwrap_or("jpg")
                .to_string();
            let mut k = 1;
            while dest.exists() {
                dest = self.inbox_dir().join(format!("{stem}-{k}.{ext}"));
                k += 1;
            }
            std::fs::copy(f, &dest)?;
            n += 1;
        }
        Ok(n)
    }

    pub fn all_tags(&self) -> BTreeSet<String> {
        self.all_items()
            .iter()
            .flat_map(|i| self.read_sidecar(i).tags)
            .collect()
    }

    pub fn items_with_tag(&self, tag: &str) -> Vec<Item> {
        self.all_items()
            .into_iter()
            .filter(|i| self.read_sidecar(i).tags.iter().any(|t| t == tag))
            .collect()
    }

    // ------------------------------------------------------------- local trash

    pub fn trash_dir(&self) -> PathBuf {
        self.root.join(PRIVATE).join("trash")
    }

    /// Move a photo (original + rendered edit + recipe) into the library's local trash.
    /// Nothing is deleted; [`Library::restore`] puts it back.
    pub fn trash(&self, item: &Item) -> Result<String> {
        let from = match &item.r {
            ItemRef::Inbox(name) => TrashFrom::Inbox { name: name.clone() },
            ItemRef::Filed { collection, id } => TrashFrom::Filed {
                collection: collection.clone(),
                id: id.clone(),
            },
            ItemRef::Trashed { .. } => bail!("already in the trash"),
        };
        let (key, dir, deleted) = self.new_trash_entry()?;
        let ext = item
            .orig
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("jpg")
            .to_ascii_lowercase();
        let meta = TrashMeta {
            from,
            deleted,
            ext: ext.clone(),
        };
        std::fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;
        move_file(&item.orig, &dir.join(format!("{TRASH_ORIG}.{ext}")))?;
        let edit = item
            .edit
            .clone()
            .or_else(|| self.edit_path(item))
            .filter(|p| p.exists());
        if let Some(e) = edit {
            move_file(&e, &dir.join(TRASH_EDIT))?;
        }
        if item.sidecar.exists() {
            move_file(&item.sidecar, &dir.join(TRASH_SIDECAR))?;
        }
        Ok(key)
    }

    fn new_trash_entry(&self) -> Result<(String, PathBuf, u64)> {
        let deleted = now();
        let base = self.trash_dir();
        std::fs::create_dir_all(&base)?;
        let mut k = 0;
        let (key, dir) = loop {
            let key = format!("{deleted}-{k:03}");
            let dir = base.join(&key);
            if !dir.exists() {
                break (key, dir);
            }
            k += 1;
        };
        std::fs::create_dir_all(&dir)?;
        Ok((key, dir, deleted))
    }

    /// Move a collage (its `.json` and exported `.jpg`, if any) into the local trash.
    pub fn trash_collage(&self, path: &Path) -> Result<String> {
        if !path.exists() {
            bail!("{} not found", path.display());
        }
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .context("collage name")?
            .to_string();
        let (key, dir, deleted) = self.new_trash_entry()?;
        let meta = TrashMeta {
            from: TrashFrom::Collage { name },
            deleted,
            ext: "json".into(),
        };
        std::fs::write(dir.join("meta.json"), serde_json::to_string_pretty(&meta)?)?;
        move_file(path, &dir.join(format!("{TRASH_ORIG}.json")))?;
        let jpg = path.with_extension("jpg");
        if jpg.exists() {
            move_file(&jpg, &dir.join(TRASH_EDIT))?;
        }
        Ok(key)
    }

    fn trash_meta(&self, key: &str) -> Option<TrashMeta> {
        serde_json::from_str(
            &std::fs::read_to_string(self.trash_dir().join(key).join("meta.json")).ok()?,
        )
        .ok()
    }

    fn trash_item(&self, key: &str, meta: &TrashMeta) -> Item {
        let dir = self.trash_dir().join(key);
        let title = match &meta.from {
            TrashFrom::Inbox { name } => name.clone(),
            TrashFrom::Filed { collection, id } => format!("{collection}/{id}"),
            TrashFrom::Collage { name } => format!("Collage {name}"),
        };
        let edit = dir.join(TRASH_EDIT);
        Item {
            r: ItemRef::Trashed {
                key: key.to_string(),
                title,
            },
            orig: dir.join(format!("{TRASH_ORIG}.{}", meta.ext)),
            edit: edit.exists().then_some(edit),
            sidecar: dir.join(TRASH_SIDECAR),
            modified: meta.deleted as i64,
        }
    }

    /// Trashed photos, newest first (`modified` = time of deletion).
    pub fn trash_items(&self) -> Vec<Item> {
        let mut v: Vec<Item> = std::fs::read_dir(self.trash_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter_map(|e| {
                let key = e.file_name().to_str()?.to_string();
                let meta = self.trash_meta(&key)?;
                Some(self.trash_item(&key, &meta))
            })
            .filter(|i| i.orig.exists())
            .collect();
        v.sort_by(|a, b| {
            b.modified
                .cmp(&a.modified)
                .then_with(|| b.orig.cmp(&a.orig))
        });
        v
    }

    pub fn trash_count(&self) -> usize {
        std::fs::read_dir(self.trash_dir())
            .into_iter()
            .flatten()
            .flatten()
            .filter(|e| e.path().join("meta.json").exists())
            .count()
    }

    /// Put a trashed photo back where it came from. Inbox names get a suffix if taken; a filed
    /// photo whose id was reused gets the next free id (and its collection is recreated).
    pub fn restore(&self, key: &str) -> Result<Item> {
        let meta = self.trash_meta(key).context("not in the trash")?;
        let t = self.trash_item(key, &meta);
        let dir = self.trash_dir().join(key);
        let item = match &meta.from {
            TrashFrom::Inbox { name } => {
                std::fs::create_dir_all(&self.inbox)?;
                let dest = unique_path(&self.inbox.join(name));
                move_file(&t.orig, &dest)?;
                let name = dest
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or_default()
                    .to_string();
                let sidecar = self
                    .root
                    .join(PRIVATE)
                    .join("inbox-edits")
                    .join(format!("{name}.json"));
                if t.sidecar.exists() {
                    std::fs::create_dir_all(sidecar.parent().unwrap())?;
                    move_file(&t.sidecar, &sidecar)?;
                }
                Item {
                    r: ItemRef::Inbox(name),
                    modified: mtime(&dest),
                    orig: dest,
                    edit: None,
                    sidecar,
                }
            }
            TrashFrom::Filed { collection, id } => {
                let cdir = self.root.join(collection);
                std::fs::create_dir_all(&cdir)?;
                let taken = self
                    .items(collection)
                    .iter()
                    .any(|i| matches!(&i.r, ItemRef::Filed { id: x, .. } if x == id))
                    || cdir.join(format!("{id}.json")).exists();
                let id = if taken {
                    self.next_id(collection)
                } else {
                    id.clone()
                };
                let orig = cdir.join(format!("{id}.orig.{}", meta.ext));
                move_file(&t.orig, &orig)?;
                let edit = cdir.join(format!("{id}.edit.jpg"));
                let edit = match &t.edit {
                    Some(e) => {
                        move_file(e, &edit)?;
                        Some(edit)
                    }
                    None => None,
                };
                let sidecar = cdir.join(format!("{id}.json"));
                if t.sidecar.exists() {
                    move_file(&t.sidecar, &sidecar)?;
                }
                Item {
                    r: ItemRef::Filed {
                        collection: collection.clone(),
                        id,
                    },
                    modified: mtime(&orig),
                    orig,
                    edit,
                    sidecar,
                }
            }
            TrashFrom::Collage { name } => {
                std::fs::create_dir_all(&self.collages)?;
                // Pick a free stem so the .json and .jpg stay paired.
                let mut stem = name.clone();
                let mut k = 1;
                while self.collages.join(format!("{stem}.json")).exists()
                    || self.collages.join(format!("{stem}.jpg")).exists()
                {
                    stem = format!("{name}-{k}");
                    k += 1;
                }
                let json = self.collages.join(format!("{stem}.json"));
                move_file(&t.orig, &json)?;
                let jpg = self.collages.join(format!("{stem}.jpg"));
                let edit = match &t.edit {
                    Some(e) => {
                        move_file(e, &jpg)?;
                        Some(jpg)
                    }
                    None => None,
                };
                Item {
                    r: ItemRef::Trashed {
                        key: String::new(),
                        title: stem,
                    },
                    modified: mtime(&json),
                    sidecar: json.clone(),
                    orig: json,
                    edit,
                }
            }
        };
        std::fs::remove_dir_all(&dir)?;
        Ok(item)
    }

    /// Permanently delete one trash entry.
    pub fn purge(&self, key: &str) -> Result<()> {
        if key.is_empty() || key.contains(['/', '\\']) || key.starts_with('.') {
            bail!("bad trash key");
        }
        std::fs::remove_dir_all(self.trash_dir().join(key))?;
        Ok(())
    }

    /// Permanently delete everything in the trash. Returns how many photos were removed.
    pub fn empty_trash(&self) -> Result<usize> {
        let n = self.trash_count();
        let dir = self.trash_dir();
        if dir.exists() {
            std::fs::remove_dir_all(&dir)?;
        }
        Ok(n)
    }
}

/// `path`, or `stem-1.ext`, `stem-2.ext` … if it exists.
fn unique_path(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path.file_stem().and_then(|s| s.to_str()).unwrap_or("scan");
    let ext = path
        .extension()
        .and_then(|s| s.to_str())
        .map(|e| format!(".{e}"))
        .unwrap_or_default();
    let dir = path.parent().unwrap_or(Path::new("."));
    (1..)
        .map(|k| dir.join(format!("{stem}-{k}{ext}")))
        .find(|p| !p.exists())
        .unwrap()
}

fn move_file(from: &Path, to: &Path) -> Result<()> {
    if std::fs::rename(from, to).is_err() {
        std::fs::copy(from, to)?;
        std::fs::remove_file(from)?;
    }
    Ok(())
}
