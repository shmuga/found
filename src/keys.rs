//! Vim-style modal keyboard layer. Pure state machine: keys in, [`Act`]s out, so it is unit
//! testable and independent of egui.
//!
//! Modes: NORMAL (motions / operators, counts like `5l`), VISUAL (`v`, extend a selection in
//! the library), INSERT (`i` in the editor: type a value), COMMAND (`:`), SEARCH (`/`).
//! Multi-key sequences (`gg`, `dd`, `yy`, `gt`, `zz`, …) wait for the next key, like vim.

use crate::ops::Aspect;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    Normal,
    Visual,
    Insert,
    Command,
    Search,
}

impl Mode {
    pub fn label(self) -> &'static str {
        match self {
            Mode::Normal => "NORMAL",
            Mode::Visual => "VISUAL",
            Mode::Insert => "INSERT",
            Mode::Command => "COMMAND",
            Mode::Search => "SEARCH",
        }
    }
}

/// Where keys go. Crop is the editor with the crop tool active.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Ctx {
    Library,
    Edit,
    Crop,
    Collage,
    Other,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum K {
    Ch(char),
    Ctrl(char),
    Esc,
    Enter,
    Tab,
    BackTab,
    Backspace,
    Delete,
    Up,
    Down,
    Left,
    Right,
}

impl K {
    fn token(self) -> String {
        match self {
            K::Ch(c) => c.to_string(),
            K::Ctrl(c) => format!("<C-{c}>"),
            K::Esc => "<Esc>".into(),
            K::Enter => "<CR>".into(),
            K::Tab => "<Tab>".into(),
            K::BackTab => "<S-Tab>".into(),
            K::Backspace => "<BS>".into(),
            K::Delete => "<Del>".into(),
            K::Up => "<Up>".into(),
            K::Down => "<Down>".into(),
            K::Left => "<Left>".into(),
            K::Right => "<Right>".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Go {
    Inbox,
    Library,
    Edit,
    Collage,
    Settings,
    Trash,
}

#[derive(Clone, Debug, PartialEq)]
pub enum Act {
    // ---- global
    Undo,
    Redo,
    Help,
    ToggleSidebar,
    Back,
    Escape,
    Go(Go),
    CollectionStep(i32),
    ZoomIn,
    ZoomOut,
    ZoomFit,
    Zoom100,
    RunCommand(String),
    Search(String),
    SearchNext(i32),
    Complete(String),
    // ---- lists (library)
    Move {
        dx: i32,
        dy: i32,
    },
    RowStart,
    RowEnd,
    First,
    Last,
    Page(i32),
    Open,
    ToggleMark,
    VisualStart,
    VisualEnd,
    /// Yank: recipe (library / `Y` in editor), node (editor), layer (collage).
    Yank,
    YankRecipe,
    Paste {
        before: bool,
    },
    Delete,
    /// Restore the selection from the trash.
    Restore,
    ItemStep(i32),
    // ---- editor
    ParamStep(i32),
    NodeStep(i32),
    Adjust {
        steps: i32,
        coarse: bool,
    },
    ResetParam,
    ResetNode,
    SetValue(String),
    NodeMove(i32),
    Bypass,
    ToggleCrop,
    ToggleCompare,
    TogglePatch,
    AutoWb(bool),
    Eyedropper,
    Randomize,
    Mutate,
    Repeat,
    // ---- crop tool
    CropMove {
        dx: i32,
        dy: i32,
    },
    CropResize(i32),
    CropAspect(Aspect),
    CropRotate(i32),
    CropFlip,
    CropDone,
    // ---- collage
    LayerStep(i32),
    LayerNudge {
        dx: i32,
        dy: i32,
    },
    LayerScale(i32),
    LayerRotate(i32),
    LayerOrder(i32),
    ToolLasso,
    ToolRect,
    ToolMove,
    ClearMask,
    FitLayer,
}

impl Act {
    /// Apply a count prefix. Repeatable actions multiply; others ignore the count.
    fn counted(self, n: i32) -> Vec<Act> {
        use Act::*;
        match self {
            Move { dx, dy } => vec![Move {
                dx: dx * n,
                dy: dy * n,
            }],
            Page(p) => vec![Page(p * n)],
            ItemStep(s) => vec![ItemStep(s * n)],
            ParamStep(s) => vec![ParamStep(s * n)],
            NodeStep(s) => vec![NodeStep(s * n)],
            Adjust { steps, coarse } => vec![Adjust {
                steps: steps * n,
                coarse,
            }],
            NodeMove(s) => vec![NodeMove(s * n)],
            CropMove { dx, dy } => vec![CropMove {
                dx: dx * n,
                dy: dy * n,
            }],
            CropResize(s) => vec![CropResize(s * n)],
            CropRotate(s) => vec![CropRotate(s * n)],
            LayerStep(s) => vec![LayerStep(s * n)],
            LayerNudge { dx, dy } => vec![LayerNudge {
                dx: dx * n,
                dy: dy * n,
            }],
            LayerScale(s) => vec![LayerScale(s * n)],
            LayerRotate(s) => vec![LayerRotate(s * n)],
            LayerOrder(s) => vec![LayerOrder(s * n)],
            CollectionStep(s) => vec![CollectionStep(s * n)],
            SearchNext(s) => vec![SearchNext(s * n)],
            Undo | Redo | ZoomIn | ZoomOut | Paste { .. } | Repeat => {
                vec![self; n.clamp(1, 100) as usize]
            }
            other => vec![other],
        }
    }
}

/// What a binding does: an action, or switching into a text-entry mode.
#[derive(Clone, Debug)]
enum B {
    A(Act),
    Cmd(&'static str),
    Search,
    Insert,
    Visual,
}

fn bindings(ctx: Ctx, visual: bool) -> Vec<(&'static str, B)> {
    use Act::*;
    use B::*;
    let mut v: Vec<(&'static str, B)> = vec![
        (":", Cmd("")),
        ("u", A(Undo)),
        ("<C-r>", A(Redo)),
        ("?", A(Help)),
        ("gb", A(ToggleSidebar)),
        ("gi", A(Go(self::Go::Inbox))),
        ("gl", A(Go(self::Go::Library))),
        ("ge", A(Go(self::Go::Edit))),
        ("gc", A(Go(self::Go::Collage))),
        ("gs", A(Go(self::Go::Settings))),
        ("gx", A(Go(self::Go::Trash))),
        ("gt", A(CollectionStep(1))),
        ("gT", A(CollectionStep(-1))),
        ("q", A(Back)),
        ("+", A(ZoomIn)),
        ("-", A(ZoomOut)),
        ("=", A(ZoomFit)),
        ("zi", A(ZoomIn)),
        ("zo", A(ZoomOut)),
        ("zz", A(ZoomFit)),
        ("z1", A(Zoom100)),
    ];
    match ctx {
        Ctx::Library => {
            v.extend([
                ("h", A(Move { dx: -1, dy: 0 })),
                ("<Left>", A(Move { dx: -1, dy: 0 })),
                ("l", A(Move { dx: 1, dy: 0 })),
                ("<Right>", A(Move { dx: 1, dy: 0 })),
                ("j", A(Move { dx: 0, dy: 1 })),
                ("<Down>", A(Move { dx: 0, dy: 1 })),
                ("k", A(Move { dx: 0, dy: -1 })),
                ("<Up>", A(Move { dx: 0, dy: -1 })),
                ("0", A(RowStart)),
                ("^", A(RowStart)),
                ("$", A(RowEnd)),
                ("gg", A(First)),
                ("G", A(Last)),
                ("<C-d>", A(Page(1))),
                ("<C-u>", A(Page(-1))),
                ("<C-f>", A(Page(2))),
                ("<C-b>", A(Page(-2))),
                ("/", B::Search),
                ("n", A(SearchNext(1))),
                ("N", A(SearchNext(-1))),
                ("y", A(YankRecipe)),
                ("yy", A(YankRecipe)),
                ("p", A(Paste { before: false })),
                ("t", Cmd("tag ")),
                ("m", Cmd("file ")),
                (" ", A(ToggleMark)),
                // Move to the local trash (restorable: `u`, `r` in the trash, :restore).
                ("dd", A(Delete)),
                ("x", A(Delete)),
                ("<BS>", A(Delete)),
                ("<Del>", A(Delete)),
                ("r", A(Restore)),
            ]);
            if visual {
                v.extend([
                    ("v", A(VisualEnd)),
                    ("V", A(VisualEnd)),
                    ("o", A(Open)),
                    ("<CR>", A(Open)),
                    ("d", A(Delete)),
                ]);
                // In visual mode a single `y` / `d` acts; drop the doubled sequences so they don't wait.
                v.retain(|(s, _)| *s != "yy" && *s != "dd");
            } else {
                v.extend([
                    ("v", Visual),
                    ("V", Visual),
                    ("o", A(Open)),
                    ("<CR>", A(Open)),
                ]);
                v.retain(|(s, _)| *s != "y");
            }
        }
        Ctx::Edit => v.extend([
            ("j", A(ParamStep(1))),
            ("<Down>", A(ParamStep(1))),
            ("k", A(ParamStep(-1))),
            ("<Up>", A(ParamStep(-1))),
            (
                "h",
                A(Adjust {
                    steps: -1,
                    coarse: false,
                }),
            ),
            (
                "<Left>",
                A(Adjust {
                    steps: -1,
                    coarse: false,
                }),
            ),
            (
                "l",
                A(Adjust {
                    steps: 1,
                    coarse: false,
                }),
            ),
            (
                "<Right>",
                A(Adjust {
                    steps: 1,
                    coarse: false,
                }),
            ),
            (
                "H",
                A(Adjust {
                    steps: -1,
                    coarse: true,
                }),
            ),
            (
                "L",
                A(Adjust {
                    steps: 1,
                    coarse: true,
                }),
            ),
            ("w", A(NodeStep(1))),
            ("b", A(NodeStep(-1))),
            ("gg", A(First)),
            ("G", A(Last)),
            ("x", A(ResetParam)),
            ("X", A(ResetNode)),
            ("i", Insert),
            ("dd", A(Delete)),
            ("yy", A(Yank)),
            ("Y", A(YankRecipe)),
            ("p", A(Paste { before: false })),
            ("P", A(Paste { before: true })),
            ("J", A(NodeMove(1))),
            ("K", A(NodeMove(-1))),
            ("o", Cmd("add ")),
            ("O", Cmd("addb ")),
            (" ", A(Bypass)),
            ("c", A(ToggleCrop)),
            ("\\", A(ToggleCompare)),
            ("<Tab>", A(TogglePatch)),
            ("aw", A(AutoWb(true))),
            ("ag", A(AutoWb(false))),
            ("e", A(Eyedropper)),
            ("R", A(Randomize)),
            ("M", A(Mutate)),
            (".", A(Repeat)),
            ("]", A(ItemStep(1))),
            ("[", A(ItemStep(-1))),
            ("t", Cmd("tag ")),
            ("m", Cmd("file ")),
            ("A", A(RunCommand("last".into()))),
        ]),
        Ctx::Crop => v.extend([
            ("h", A(CropMove { dx: -1, dy: 0 })),
            ("<Left>", A(CropMove { dx: -1, dy: 0 })),
            ("l", A(CropMove { dx: 1, dy: 0 })),
            ("<Right>", A(CropMove { dx: 1, dy: 0 })),
            ("j", A(CropMove { dx: 0, dy: 1 })),
            ("<Down>", A(CropMove { dx: 0, dy: 1 })),
            ("k", A(CropMove { dx: 0, dy: -1 })),
            ("<Up>", A(CropMove { dx: 0, dy: -1 })),
            ("H", A(CropMove { dx: -10, dy: 0 })),
            ("L", A(CropMove { dx: 10, dy: 0 })),
            ("J", A(CropMove { dx: 0, dy: 10 })),
            ("K", A(CropMove { dx: 0, dy: -10 })),
            (">", A(CropResize(1))),
            ("<", A(CropResize(-1))),
            ("f", A(CropAspect(Aspect::Free))),
            ("s", A(CropAspect(Aspect::Square))),
            ("w", A(CropAspect(Aspect::R3x2))),
            ("t", A(CropAspect(Aspect::R2x3))),
            ("O", A(CropAspect(Aspect::Original))),
            ("r", A(CropRotate(1))),
            ("R", A(CropRotate(-1))),
            ("F", A(CropFlip)),
            ("c", A(CropDone)),
            ("<CR>", A(CropDone)),
            ("\\", A(ToggleCompare)),
        ]),
        Ctx::Collage => v.extend([
            ("h", A(LayerNudge { dx: -1, dy: 0 })),
            ("<Left>", A(LayerNudge { dx: -1, dy: 0 })),
            ("l", A(LayerNudge { dx: 1, dy: 0 })),
            ("<Right>", A(LayerNudge { dx: 1, dy: 0 })),
            ("j", A(LayerNudge { dx: 0, dy: 1 })),
            ("<Down>", A(LayerNudge { dx: 0, dy: 1 })),
            ("k", A(LayerNudge { dx: 0, dy: -1 })),
            ("<Up>", A(LayerNudge { dx: 0, dy: -1 })),
            ("H", A(LayerNudge { dx: -10, dy: 0 })),
            ("L", A(LayerNudge { dx: 10, dy: 0 })),
            ("J", A(LayerNudge { dx: 0, dy: 10 })),
            ("K", A(LayerNudge { dx: 0, dy: -10 })),
            ("<Tab>", A(LayerStep(1))),
            ("<S-Tab>", A(LayerStep(-1))),
            (">", A(LayerScale(1))),
            ("<", A(LayerScale(-1))),
            ("r", A(LayerRotate(1))),
            ("R", A(LayerRotate(-1))),
            ("]", A(LayerOrder(1))),
            ("[", A(LayerOrder(-1))),
            ("dd", A(Delete)),
            ("yy", A(Yank)),
            ("p", A(Paste { before: false })),
            ("m", A(ToolLasso)),
            ("M", A(ToolRect)),
            ("s", A(ToolMove)),
            ("x", A(ClearMask)),
            ("f", A(FitLayer)),
        ]),
        Ctx::Other => {}
    }
    v
}

#[derive(Debug)]
pub struct Keys {
    pub mode: Mode,
    /// Keys typed so far in a multi-key sequence (vim's showcmd).
    pub pending: String,
    pub count: Option<u32>,
    /// Text being typed in COMMAND / SEARCH / INSERT.
    pub line: String,
    /// Mode to return to after a command line (VISUAL keeps its selection for `:tag` etc.).
    return_to: Mode,
}

impl Default for Keys {
    fn default() -> Self {
        Self {
            mode: Mode::Normal,
            pending: String::new(),
            count: None,
            line: String::new(),
            return_to: Mode::Normal,
        }
    }
}

impl Keys {
    pub fn showcmd(&self) -> String {
        format!(
            "{}{}",
            self.count.map(|c| c.to_string()).unwrap_or_default(),
            self.pending
        )
    }

    pub fn reset(&mut self) {
        self.pending.clear();
        self.count = None;
    }

    /// Enter COMMAND mode with a prefilled line (used by `:`, `o`, `t`, `m`, Cmd-K …).
    pub fn prompt(&mut self, prefill: &str) {
        if self.mode != Mode::Command && self.mode != Mode::Search {
            self.return_to = if self.mode == Mode::Visual {
                Mode::Visual
            } else {
                Mode::Normal
            };
        }
        self.mode = Mode::Command;
        self.line = prefill.to_string();
        self.reset();
    }

    pub fn feed(&mut self, k: K, ctx: Ctx) -> Vec<Act> {
        match self.mode {
            Mode::Command | Mode::Search | Mode::Insert => self.feed_line(k),
            Mode::Normal | Mode::Visual => self.feed_normal(k, ctx),
        }
    }

    fn feed_line(&mut self, k: K) -> Vec<Act> {
        let mode = self.mode;
        let finish = |s: &mut Self| {
            s.mode = if mode == Mode::Insert {
                Mode::Normal
            } else {
                s.return_to
            };
            s.return_to = Mode::Normal;
            std::mem::take(&mut s.line)
        };
        match k {
            K::Esc | K::Ctrl('c') | K::Ctrl('[') => {
                finish(self);
                vec![]
            }
            K::Enter => {
                let line = finish(self);
                let line = line.trim().to_string();
                match mode {
                    Mode::Command if !line.is_empty() => vec![Act::RunCommand(line)],
                    Mode::Search => vec![Act::Search(line)],
                    Mode::Insert if !line.is_empty() => vec![Act::SetValue(line)],
                    _ => vec![],
                }
            }
            K::Backspace => {
                if self.line.pop().is_none() {
                    finish(self);
                }
                vec![]
            }
            K::Ctrl('u') => {
                self.line.clear();
                vec![]
            }
            K::Ctrl('w') => {
                let t = self.line.trim_end().len();
                let cut = self.line[..t].rfind(' ').map(|i| i + 1).unwrap_or(0);
                self.line.truncate(cut);
                vec![]
            }
            K::Tab if mode == Mode::Command => vec![Act::Complete(self.line.clone())],
            K::Ch(c) => {
                self.line.push(c);
                vec![]
            }
            _ => vec![],
        }
    }

    fn feed_normal(&mut self, k: K, ctx: Ctx) -> Vec<Act> {
        if matches!(k, K::Esc | K::Ctrl('c') | K::Ctrl('[')) {
            let had_pending = !self.pending.is_empty() || self.count.is_some();
            self.reset();
            if self.mode == Mode::Visual {
                self.mode = Mode::Normal;
                return vec![Act::VisualEnd];
            }
            return if had_pending {
                vec![]
            } else {
                vec![Act::Escape]
            };
        }
        // Count prefix (a leading 0 is a motion, as in vim).
        if self.pending.is_empty()
            && let K::Ch(c) = k
            && let Some(d) = c.to_digit(10)
            && (d != 0 || self.count.is_some())
        {
            self.count = Some(
                self.count
                    .unwrap_or(0)
                    .saturating_mul(10)
                    .saturating_add(d)
                    .min(9999),
            );
            return vec![];
        }
        let seq = format!("{}{}", self.pending, k.token());
        let table = bindings(ctx, self.mode == Mode::Visual);
        if let Some((_, b)) = table.iter().find(|(s, _)| *s == seq) {
            let n = self.count.unwrap_or(1) as i32;
            self.reset();
            return match b.clone() {
                B::A(a) => {
                    if a == Act::VisualEnd {
                        self.mode = Mode::Normal;
                    }
                    a.counted(n)
                }
                B::Cmd(pre) => {
                    self.prompt(pre);
                    vec![]
                }
                B::Search => {
                    self.return_to = Mode::Normal;
                    self.mode = Mode::Search;
                    self.line.clear();
                    vec![]
                }
                B::Insert => {
                    self.mode = Mode::Insert;
                    self.line.clear();
                    vec![]
                }
                B::Visual => {
                    self.mode = Mode::Visual;
                    vec![Act::VisualStart]
                }
            };
        }
        if table.iter().any(|(s, _)| s.starts_with(&seq)) {
            self.pending = seq;
        } else {
            self.reset(); // unknown sequence: drop it, like vim
        }
        vec![]
    }
}

/// Short reference shown by `?`.
pub fn help(ctx: Ctx) -> Vec<(&'static str, &'static str)> {
    let mut v = vec![
        (":", "command line (Tab completes)"),
        ("u / Ctrl-r", "undo / redo"),
        ("q", "back to library"),
        (
            "gi gl ge gc gs gx",
            "go: inbox, library, edit, collage, settings, trash",
        ),
        ("gt / gT", "next / previous collection"),
        ("gb", "toggle sidebar"),
        ("+ - = z1", "zoom in, out, fit, 100%"),
        ("<count>", "prefix most motions, e.g. 5l"),
    ];
    v.extend(match ctx {
        Ctx::Library => vec![
            ("h j k l", "move"),
            ("0 $ gg G", "row start / end, first / last"),
            ("Ctrl-d / Ctrl-u", "half page down / up"),
            ("Enter / o", "open"),
            ("v", "visual select (move to extend)"),
            ("Space", "mark / unmark"),
            ("/ n N", "search name or tag, next, previous"),
            ("yy / p", "yank recipe, paste onto selection (keeps crop)"),
            ("t / m", ":tag … / :file … the selection"),
            ("dd x Backspace / d", "move selection to trash (u undoes)"),
            ("r", "restore (in the trash)"),
        ],
        Ctx::Edit => vec![
            ("j k", "next / previous slider"),
            ("h l / H L", "adjust (fine / coarse)"),
            ("w b", "next / previous node"),
            ("i", "type a value"),
            ("x / X", "reset slider / node"),
            ("dd yy p P", "delete, yank, paste node after / before"),
            ("J K", "move node down / up"),
            ("o O", "add node after / before"),
            ("Space", "bypass node"),
            ("c", "crop tool"),
            ("aw ag e", "auto WB white / grey, eyedropper"),
            ("R M", "randomize node, mutate all"),
            (".", "repeat last adjustment"),
            ("] [", "next / previous photo"),
            ("Tab", "presentation / patch"),
            ("\\", "before / after"),
            ("Y", "yank recipe"),
            ("A", "apply last preset / last saved edits"),
            (":w :q :wq :q!", "save & render, back, back without saving"),
            (":saveas Coll", "save a copy into a collection"),
        ],
        Ctx::Crop => vec![
            ("h j k l / H J K L", "move crop (1% / 10%)"),
            ("> <", "grow / shrink"),
            ("f s w t O", "free, 1:1, 3:2, 2:3, original"),
            ("r R F", "rotate right / left, flip"),
            ("c / Enter / Esc", "done"),
        ],
        Ctx::Collage => vec![
            ("Tab / S-Tab", "next / previous layer"),
            ("h j k l / H J K L", "nudge layer"),
            ("> <", "scale layer"),
            ("r R", "rotate layer"),
            ("] [", "raise / lower"),
            ("dd yy p", "delete, yank, paste (duplicate)"),
            ("m M s", "lasso, rect mask, move tool"),
            ("x f", "clear mask, fit"),
            (":w / :export", "save collage / render JPEG"),
        ],
        Ctx::Other => vec![],
    });
    v
}

/// Command names for completion and `:help`.
pub const COMMANDS: &[&str] = &[
    "w",
    "q",
    "wq",
    "tag",
    "untag",
    "file",
    "new",
    "add",
    "addb",
    "preset",
    "savepreset",
    "crop",
    "sort",
    "group",
    "collage",
    "export",
    "inbox",
    "settings",
    "library",
    "nohlsearch",
    "help",
    "yank",
    "paste",
    "set",
    "trash",
    "restore",
    "emptytrash",
    "layout",
    "collections",
    "last",
];

#[cfg(test)]
mod tests {
    use super::*;

    fn feed(k: &mut Keys, s: &str, ctx: Ctx) -> Vec<Act> {
        s.chars().flat_map(|c| k.feed(K::Ch(c), ctx)).collect()
    }

    #[test]
    fn counts_and_sequences() {
        let mut k = Keys::default();
        assert_eq!(
            feed(&mut k, "5l", Ctx::Library),
            [Act::Move { dx: 5, dy: 0 }]
        );
        assert_eq!(feed(&mut k, "g", Ctx::Library), []);
        assert_eq!(k.showcmd(), "g");
        assert_eq!(feed(&mut k, "g", Ctx::Library), [Act::First]);
        assert_eq!(feed(&mut k, "0", Ctx::Library), [Act::RowStart]);
        assert_eq!(
            feed(&mut k, "10j", Ctx::Library),
            [Act::Move { dx: 0, dy: 10 }]
        );
        assert_eq!(feed(&mut k, "dd", Ctx::Edit), [Act::Delete]);
        assert_eq!(
            feed(&mut k, "3H", Ctx::Edit),
            [Act::Adjust {
                steps: -3,
                coarse: true
            }]
        );
        assert_eq!(feed(&mut k, "2u", Ctx::Edit), [Act::Undo, Act::Undo]);
        assert_eq!(
            feed(&mut k, "zq", Ctx::Edit),
            [],
            "unknown sequence dropped"
        );
        assert_eq!(feed(&mut k, "z1", Ctx::Edit), [Act::Zoom100]);
    }

    #[test]
    fn command_line_and_insert() {
        let mut k = Keys::default();
        feed(&mut k, ":file Coins", Ctx::Library);
        assert_eq!(k.mode, Mode::Command);
        assert_eq!(
            k.feed(K::Enter, Ctx::Library),
            [Act::RunCommand("file Coins".into())]
        );
        assert_eq!(k.mode, Mode::Normal);
        feed(&mut k, "i-0.5", Ctx::Edit);
        assert_eq!(k.mode, Mode::Insert);
        assert_eq!(k.feed(K::Enter, Ctx::Edit), [Act::SetValue("-0.5".into())]);
        feed(&mut k, "o", Ctx::Edit);
        assert_eq!(k.line, "add ");
        assert_eq!(k.feed(K::Esc, Ctx::Edit), []);
        assert_eq!(k.mode, Mode::Normal);
        // Backspace on an empty line leaves the mode, like vim.
        feed(&mut k, ":", Ctx::Edit);
        k.feed(K::Backspace, Ctx::Edit);
        assert_eq!(k.mode, Mode::Normal);
    }

    #[test]
    fn visual_mode_keeps_selection_through_commands() {
        let mut k = Keys::default();
        assert_eq!(feed(&mut k, "v", Ctx::Library), [Act::VisualStart]);
        assert_eq!(
            feed(&mut k, "3l", Ctx::Library),
            [Act::Move { dx: 3, dy: 0 }]
        );
        feed(&mut k, "t", Ctx::Library);
        assert_eq!(k.line, "tag ");
        feed(&mut k, "coin", Ctx::Library);
        assert_eq!(
            k.feed(K::Enter, Ctx::Library),
            [Act::RunCommand("tag coin".into())]
        );
        assert_eq!(
            k.mode,
            Mode::Visual,
            "back to visual so the selection is still shown"
        );
        assert_eq!(k.feed(K::Esc, Ctx::Library), [Act::VisualEnd]);
        assert_eq!(k.mode, Mode::Normal);
        assert_eq!(k.feed(K::Esc, Ctx::Library), [Act::Escape]);
    }

    #[test]
    fn crop_and_collage_contexts() {
        let mut k = Keys::default();
        assert_eq!(
            feed(&mut k, "4K", Ctx::Crop),
            [Act::CropMove { dx: 0, dy: -40 }]
        );
        assert_eq!(
            feed(&mut k, "s", Ctx::Crop),
            [Act::CropAspect(Aspect::Square)]
        );
        assert_eq!(k.feed(K::Tab, Ctx::Collage), [Act::LayerStep(1)]);
        assert_eq!(
            feed(&mut k, "yyp", Ctx::Collage),
            [Act::Yank, Act::Paste { before: false }]
        );
    }
}
