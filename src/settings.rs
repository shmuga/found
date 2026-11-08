//! User settings (JSON). Nothing about folders is fixed in code: every folder is either chosen
//! by the user or is a default the user can override here.
//!
//! The settings file lives in the OS config dir (`~/Library/Application Support/found/
//! settings.json` on macOS) unless `FOUND_SETTINGS` points elsewhere.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Folder layout of one library. Each entry is relative to the library root or absolute
/// (e.g. the Inbox can be your scanner's output folder anywhere on disk).
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Layout {
    pub inbox: String,
    pub collages: String,
    pub presets: String,
}

impl Default for Layout {
    fn default() -> Self {
        Self {
            inbox: "Inbox".into(),
            collages: "_collages".into(),
            presets: "_presets".into(),
        }
    }
}

impl Layout {
    pub fn resolve(root: &Path, entry: &str) -> PathBuf {
        let p = Path::new(entry.trim());
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            root.join(p)
        }
    }

    /// Store a picked folder relative to the root when it's inside it.
    pub fn entry_for(root: &Path, picked: &Path) -> String {
        picked
            .strip_prefix(root)
            .map(|r| r.to_string_lossy().to_string())
            .unwrap_or_else(|_| picked.to_string_lossy().to_string())
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct LibraryProfile {
    pub root: PathBuf,
    #[serde(default)]
    pub layout: Layout,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// Most recent first.
    pub recent_libraries: Vec<PathBuf>,
    /// Skip the startup picker and reopen the last library.
    pub open_last_on_startup: bool,
    pub libraries: Vec<LibraryProfile>,
    /// Thumbnail cache folder; `None` = the OS cache folder.
    pub thumb_cache: Option<PathBuf>,
    pub jpeg_quality: u8,
    /// Long edge of the editing proxy in px.
    pub preview_edge: usize,
    /// Vim-style modal keys (NORMAL / VISUAL / INSERT / COMMAND).
    pub vim_keys: bool,
    pub sidebar_compact: bool,
    pub sort: SortKey,
    pub sort_desc: bool,
    /// Grouping in the Inbox / a collection / a tag.
    pub group_by: GroupBy,
    /// Grouping in "All collections" (by collection unless changed).
    pub group_all: GroupBy,
    pub grid_layout: GridLayout,
    /// Thumbnail size in points (tile width in Grid, row height in Rows).
    pub tile_size: f32,
    /// What the editor's "Apply last" button applies: the last preset applied or the recipe
    /// of the last photo saved, whichever happened last.
    pub last_recipe: Option<LastRecipe>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct LastRecipe {
    /// Preset name, or "edits of <photo>".
    pub name: String,
    pub graph: crate::graph::Graph,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GroupBy {
    None,
    #[default]
    Day,
    Month,
    Collection,
}

impl GroupBy {
    pub const ALL: [GroupBy; 4] = [
        GroupBy::None,
        GroupBy::Day,
        GroupBy::Month,
        GroupBy::Collection,
    ];
    pub fn label(self) -> &'static str {
        match self {
            GroupBy::None => "No groups",
            GroupBy::Day => "By day",
            GroupBy::Month => "By month",
            GroupBy::Collection => "By collection",
        }
    }
}

/// How the library grid is laid out.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum GridLayout {
    /// Equal square tiles.
    #[default]
    Grid,
    /// Justified rows: equal height, each photo at its own aspect ratio.
    Rows,
    /// One line per photo with details.
    List,
}

impl GridLayout {
    pub const ALL: [GridLayout; 3] = [GridLayout::Grid, GridLayout::Rows, GridLayout::List];
    pub fn label(self) -> &'static str {
        match self {
            GridLayout::Grid => "Grid",
            GridLayout::Rows => "Rows",
            GridLayout::List => "List",
        }
    }
}

impl PartialEq for LastRecipe {
    fn eq(&self, o: &Self) -> bool {
        self.name == o.name
            && crate::graph::graph_json(&self.graph) == crate::graph::graph_json(&o.graph)
    }
}

pub const TILE_MIN: f32 = 72.0;
pub const TILE_MAX: f32 = 520.0;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum SortKey {
    #[default]
    Date,
    Name,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            recent_libraries: vec![],
            open_last_on_startup: false,
            libraries: vec![],
            thumb_cache: None,
            jpeg_quality: 95,
            preview_edge: 2400,
            vim_keys: true,
            sidebar_compact: false,
            sort: SortKey::Date,
            sort_desc: true,
            group_by: GroupBy::Day,
            group_all: GroupBy::Collection,
            grid_layout: GridLayout::Grid,
            tile_size: 168.0,
            last_recipe: None,
        }
    }
}

pub const MAX_RECENT: usize = 10;

impl Settings {
    pub fn default_file() -> Option<PathBuf> {
        if let Some(p) =
            std::env::var_os("FOUND_SETTINGS").or_else(|| std::env::var_os("SCANNER_SETTINGS"))
        {
            return Some(PathBuf::from(p));
        }
        let file = dirs::config_dir()?.join("found").join("settings.json");
        // Carry settings over from before the app was renamed.
        let old = dirs::config_dir()?.join("scanner").join("settings.json");
        if !file.exists() && old.exists() {
            let _ = std::fs::create_dir_all(file.parent()?);
            let _ = std::fs::copy(&old, &file);
        }
        Some(file)
    }

    /// Missing or unreadable file → defaults (a corrupt file is kept aside, not overwritten).
    pub fn load(path: &Path) -> Settings {
        match std::fs::read_to_string(path) {
            Ok(s) => serde_json::from_str(&s).unwrap_or_else(|e| {
                log::warn!("settings unreadable ({e}); using defaults");
                let _ = std::fs::rename(path, path.with_extension("json.bad"));
                Settings::default()
            }),
            Err(_) => Settings::default(),
        }
    }

    pub fn save(&self, path: &Path) -> anyhow::Result<()> {
        if let Some(d) = path.parent() {
            std::fs::create_dir_all(d)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_string_pretty(self)?)?;
        std::fs::rename(tmp, path)?;
        Ok(())
    }

    pub fn last_library(&self) -> Option<&PathBuf> {
        self.recent_libraries.first()
    }

    pub fn touch_recent(&mut self, root: &Path) {
        self.recent_libraries.retain(|p| p != root);
        self.recent_libraries.insert(0, root.to_path_buf());
        self.recent_libraries.truncate(MAX_RECENT);
    }

    pub fn forget(&mut self, root: &Path) {
        self.recent_libraries.retain(|p| p != root);
    }

    pub fn layout_for(&self, root: &Path) -> Layout {
        self.libraries
            .iter()
            .find(|l| l.root == root)
            .map(|l| l.layout.clone())
            .unwrap_or_default()
    }

    pub fn set_layout(&mut self, root: &Path, layout: Layout) {
        match self.libraries.iter_mut().find(|l| l.root == root) {
            Some(l) => l.layout = layout,
            None => self.libraries.push(LibraryProfile {
                root: root.to_path_buf(),
                layout,
            }),
        }
    }

    pub fn thumb_dir(&self) -> Option<PathBuf> {
        self.thumb_cache
            .clone()
            .or_else(|| Some(dirs::cache_dir()?.join("found").join("thumbs")))
    }
}
