//! Collage model + CPU compositor (shared by preview and full-resolution export).

use crate::img::Img;
use anyhow::Result;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum Template {
    #[default]
    Free,
    TwoH,
    TwoV,
    ThreeH,
    ThreeV,
    Grid2x2,
    OneTwo,
    Grid3x3,
}

impl Template {
    pub const ALL: [Template; 8] = [
        Template::Free,
        Template::TwoH,
        Template::TwoV,
        Template::ThreeH,
        Template::ThreeV,
        Template::Grid2x2,
        Template::OneTwo,
        Template::Grid3x3,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Template::Free => "Free",
            Template::TwoH => "2 side by side",
            Template::TwoV => "2 stacked",
            Template::ThreeH => "3 columns",
            Template::ThreeV => "3 rows",
            Template::Grid2x2 => "2×2 grid",
            Template::OneTwo => "1 big + 2",
            Template::Grid3x3 => "3×3 grid",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct CanvasPreset {
    pub name: &'static str,
    pub w: u32,
    pub h: u32,
}

pub const CANVAS_PRESETS: &[CanvasPreset] = &[
    CanvasPreset {
        name: "1:1 · 4000",
        w: 4000,
        h: 4000,
    },
    CanvasPreset {
        name: "3:2 · 6000×4000",
        w: 6000,
        h: 4000,
    },
    CanvasPreset {
        name: "2:3 · 4000×6000",
        w: 4000,
        h: 6000,
    },
    CanvasPreset {
        name: "A4 portrait 300dpi",
        w: 2480,
        h: 3508,
    },
    CanvasPreset {
        name: "A4 landscape 300dpi",
        w: 3508,
        h: 2480,
    },
    CanvasPreset {
        name: "16:9 · 3840×2160",
        w: 3840,
        h: 2160,
    },
];

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize, Default)]
pub enum Mask {
    #[default]
    None,
    /// Normalised rect in source uv.
    Rect { r: [f32; 4] },
    /// Lasso polygon in source uv.
    Poly { pts: Vec<[f32; 2]> },
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: u32,
    /// Path relative to the library root.
    pub source: String,
    pub name: String,
    pub visible: bool,
    pub locked: bool,
    /// Template cell this layer is clipped to.
    pub cell: Option<usize>,
    /// Centre in canvas px.
    pub cx: f32,
    pub cy: f32,
    /// Displayed width in canvas px (height follows the source aspect).
    pub width: f32,
    pub rotation: f32,
    pub opacity: f32,
    pub mask: Mask,
    /// Source w / h.
    pub aspect: f32,
}

impl Layer {
    pub fn height(&self) -> f32 {
        self.width / self.aspect
    }

    /// Canvas px → source uv (may be outside 0..1).
    pub fn canvas_to_uv(&self, x: f32, y: f32) -> (f32, f32) {
        let (s, c) = (-self.rotation.to_radians()).sin_cos();
        let (dx, dy) = (x - self.cx, y - self.cy);
        let rx = c * dx - s * dy;
        let ry = s * dx + c * dy;
        (rx / self.width + 0.5, ry / self.height() + 0.5)
    }

    pub fn uv_to_canvas(&self, u: f32, v: f32) -> (f32, f32) {
        let (s, c) = self.rotation.to_radians().sin_cos();
        let dx = (u - 0.5) * self.width;
        let dy = (v - 0.5) * self.height();
        (self.cx + c * dx - s * dy, self.cy + s * dx + c * dy)
    }

    pub fn corners(&self) -> [(f32, f32); 4] {
        [
            self.uv_to_canvas(0.0, 0.0),
            self.uv_to_canvas(1.0, 0.0),
            self.uv_to_canvas(1.0, 1.0),
            self.uv_to_canvas(0.0, 1.0),
        ]
    }

    pub fn hit(&self, x: f32, y: f32, alpha: Option<&MaskRaster>) -> bool {
        let (u, v) = self.canvas_to_uv(x, y);
        if !(0.0..=1.0).contains(&u) || !(0.0..=1.0).contains(&v) {
            return false;
        }
        alpha.map(|m| m.at(u, v) > 0.1).unwrap_or(true)
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Collage {
    pub name: String,
    pub width: u32,
    pub height: u32,
    pub background: [u8; 3],
    pub template: Template,
    /// Gutter and margin as fractions of the canvas short edge.
    pub gutter: f32,
    pub margin: f32,
    pub layers: Vec<Layer>,
    pub next_id: u32,
}

impl Collage {
    pub fn new(name: &str, preset: CanvasPreset) -> Self {
        Self {
            name: name.into(),
            width: preset.w,
            height: preset.h,
            background: [245, 245, 242],
            template: Template::Free,
            gutter: 0.02,
            margin: 0.03,
            layers: vec![],
            next_id: 1,
        }
    }

    /// Template cells in canvas px [x, y, w, h].
    pub fn cells(&self) -> Vec<[f32; 4]> {
        let (w, h) = (self.width as f32, self.height as f32);
        let short = w.min(h);
        let m = self.margin * short;
        let g = self.gutter * short;
        let (ix, iy, iw, ih) = (m, m, w - 2.0 * m, h - 2.0 * m);
        let grid = |cols: usize, rows: usize| -> Vec<[f32; 4]> {
            let cw = (iw - g * (cols - 1) as f32) / cols as f32;
            let ch = (ih - g * (rows - 1) as f32) / rows as f32;
            let mut v = vec![];
            for r in 0..rows {
                for c in 0..cols {
                    v.push([ix + c as f32 * (cw + g), iy + r as f32 * (ch + g), cw, ch]);
                }
            }
            v
        };
        match self.template {
            Template::Free => vec![],
            Template::TwoH => grid(2, 1),
            Template::TwoV => grid(1, 2),
            Template::ThreeH => grid(3, 1),
            Template::ThreeV => grid(1, 3),
            Template::Grid2x2 => grid(2, 2),
            Template::Grid3x3 => grid(3, 3),
            Template::OneTwo => {
                let bw = (iw - g) * 0.6;
                let sw = iw - g - bw;
                let sh = (ih - g) / 2.0;
                vec![
                    [ix, iy, bw, ih],
                    [ix + bw + g, iy, sw, sh],
                    [ix + bw + g, iy + sh + g, sw, sh],
                ]
            }
        }
    }

    pub fn first_free_cell(&self) -> Option<usize> {
        (0..self.cells().len()).find(|c| !self.layers.iter().any(|l| l.cell == Some(*c)))
    }

    /// Scale/centre a layer to cover its cell (or fit in the canvas when free).
    pub fn fit_layer(&self, layer: &mut Layer) {
        let cells = self.cells();
        let cell = layer.cell.and_then(|c| cells.get(c)).copied();
        layer.rotation = 0.0;
        match cell {
            Some([x, y, w, h]) => {
                layer.cx = x + w * 0.5;
                layer.cy = y + h * 0.5;
                layer.width = w.max(h * layer.aspect);
            }
            None => {
                let (cw, ch) = (self.width as f32, self.height as f32);
                layer.cx = cw * 0.5;
                layer.cy = ch * 0.5;
                layer.width = (cw * 0.6).min(ch * 0.6 * layer.aspect);
            }
        }
    }

    pub fn add_layer(&mut self, source: String, name: String, aspect: f32) -> u32 {
        let id = self.next_id;
        self.next_id += 1;
        let mut l = Layer {
            id,
            source,
            name,
            visible: true,
            locked: false,
            cell: self.first_free_cell(),
            cx: 0.0,
            cy: 0.0,
            width: 1.0,
            rotation: 0.0,
            opacity: 1.0,
            mask: Mask::None,
            aspect: aspect.max(0.01),
        };
        // Free layers cascade a little so they don't stack exactly.
        self.fit_layer(&mut l);
        if l.cell.is_none() {
            let k = self.layers.len() as f32;
            l.cx += k * self.width as f32 * 0.03;
            l.cy += k * self.height as f32 * 0.03;
        }
        self.layers.push(l);
        id
    }

    /// Re-assign cells after a template change: layers keep their order.
    pub fn retemplate(&mut self, t: Template) {
        self.template = t;
        let n = self.cells().len();
        let mut layers = std::mem::take(&mut self.layers);
        for (i, l) in layers.iter_mut().enumerate() {
            l.cell = (i < n).then_some(i);
            self.fit_layer(l);
        }
        self.layers = layers;
    }

    pub fn refit_all(&mut self) {
        let mut layers = std::mem::take(&mut self.layers);
        for l in layers.iter_mut().filter(|l| l.cell.is_some()) {
            self.fit_layer(l);
        }
        self.layers = layers;
    }
}

/// Rasterised mask alpha in source uv space.
pub struct MaskRaster {
    pub w: usize,
    pub h: usize,
    pub a: Vec<f32>,
}

impl MaskRaster {
    pub fn build(mask: &Mask, aspect: f32, long: usize) -> Option<MaskRaster> {
        let (w, h) = if aspect >= 1.0 {
            (long, ((long as f32 / aspect) as usize).max(1))
        } else {
            (((long as f32 * aspect) as usize).max(1), long)
        };
        match mask {
            Mask::None => None,
            Mask::Rect { r } => {
                let mut a = vec![0.0; w * h];
                let (x0, y0) = ((r[0] * w as f32) as usize, (r[1] * h as f32) as usize);
                let (x1, y1) = (
                    (((r[0] + r[2]) * w as f32) as usize).min(w),
                    (((r[1] + r[3]) * h as f32) as usize).min(h),
                );
                for y in y0..y1 {
                    for x in x0..x1 {
                        a[y * w + x] = 1.0;
                    }
                }
                Some(MaskRaster { w, h, a })
            }
            Mask::Poly { pts } => {
                if pts.len() < 3 {
                    return None;
                }
                let pts: Vec<(f32, f32)> = pts
                    .iter()
                    .map(|p| (p[0] * w as f32, p[1] * h as f32))
                    .collect();
                const SS: usize = 4; // vertical supersamples for smooth edges
                let mut a = vec![0.0f32; w * h];
                a.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                    for s in 0..SS {
                        let fy = y as f32 + (s as f32 + 0.5) / SS as f32;
                        let mut xs: Vec<f32> = vec![];
                        for i in 0..pts.len() {
                            let (ax, ay) = pts[i];
                            let (bx, by) = pts[(i + 1) % pts.len()];
                            if (ay <= fy && by > fy) || (by <= fy && ay > fy) {
                                xs.push(ax + (fy - ay) / (by - ay) * (bx - ax));
                            }
                        }
                        xs.sort_by(|a, b| a.total_cmp(b));
                        for pair in xs.chunks_exact(2) {
                            let (l, r) = (pair[0].max(0.0), pair[1].min(w as f32));
                            if r <= l {
                                continue;
                            }
                            let (li, ri) = (l.floor() as usize, (r.ceil() as usize).min(w));
                            for x in li..ri {
                                let cov = ((x + 1) as f32).min(r) - (x as f32).max(l);
                                row[x] += cov.max(0.0) / SS as f32;
                            }
                        }
                    }
                });
                Some(MaskRaster { w, h, a })
            }
        }
    }

    #[inline]
    pub fn at(&self, u: f32, v: f32) -> f32 {
        let x = (u * self.w as f32 - 0.5).clamp(0.0, (self.w - 1) as f32);
        let y = (v * self.h as f32 - 0.5).clamp(0.0, (self.h - 1) as f32);
        let (x0, y0) = (x as usize, y as usize);
        let (x1, y1) = ((x0 + 1).min(self.w - 1), (y0 + 1).min(self.h - 1));
        let (tx, ty) = (x - x0 as f32, y - y0 as f32);
        let a =
            self.a[y0 * self.w + x0] + (self.a[y0 * self.w + x1] - self.a[y0 * self.w + x0]) * tx;
        let b =
            self.a[y1 * self.w + x0] + (self.a[y1 * self.w + x1] - self.a[y1 * self.w + x0]) * tx;
        a + (b - a) * ty
    }
}

/// Composite at `scale` output px per canvas px. `source` loads a layer's image on demand
/// (export loads full-res one layer at a time, so memory stays bounded).
pub fn composite(
    c: &Collage,
    scale: f32,
    mask_res: usize,
    mut source: impl FnMut(&Layer) -> Option<std::sync::Arc<Img>>,
) -> Img {
    let ow = ((c.width as f32 * scale).round() as usize).max(1);
    let oh = ((c.height as f32 * scale).round() as usize).max(1);
    let bg = c.background.map(|v| v as f32 / 255.0);
    let mut out = Img::filled(ow, oh, bg);
    let cells = c.cells();
    for l in c.layers.iter().filter(|l| l.visible && l.opacity > 0.0) {
        let Some(src) = source(l) else { continue };
        let mask = MaskRaster::build(&l.mask, l.aspect, mask_res);
        let clip = l.cell.and_then(|i| cells.get(i)).copied();
        // Bounding box in output px.
        let cs = l.corners();
        let (mut x0, mut y0, mut x1, mut y1) = (f32::MAX, f32::MAX, f32::MIN, f32::MIN);
        for (x, y) in cs {
            x0 = x0.min(x);
            y0 = y0.min(y);
            x1 = x1.max(x);
            y1 = y1.max(y);
        }
        if let Some([cx, cy, cw, ch]) = clip {
            x0 = x0.max(cx);
            y0 = y0.max(cy);
            x1 = x1.min(cx + cw);
            y1 = y1.min(cy + ch);
        }
        let bx0 = ((x0 * scale).floor().max(0.0)) as usize;
        let by0 = ((y0 * scale).floor().max(0.0)) as usize;
        let bx1 = ((x1 * scale).ceil().max(0.0) as usize).min(ow);
        let by1 = ((y1 * scale).ceil().max(0.0) as usize).min(oh);
        if bx1 <= bx0 || by1 <= by0 {
            continue;
        }
        let (sw, sh) = (src.w as f32, src.h as f32);
        out.px
            .par_chunks_mut(ow)
            .enumerate()
            .skip(by0)
            .take(by1 - by0)
            .for_each(|(y, row)| {
                let cyp = (y as f32 + 0.5) / scale;
                for x in bx0..bx1 {
                    let cxp = (x as f32 + 0.5) / scale;
                    if let Some([cx, cy, cw, ch]) = clip
                        && (cxp < cx || cxp >= cx + cw || cyp < cy || cyp >= cy + ch)
                    {
                        continue;
                    }
                    let (u, v) = l.canvas_to_uv(cxp, cyp);
                    if !(0.0..1.0).contains(&u) || !(0.0..1.0).contains(&v) {
                        continue;
                    }
                    let mut a = l.opacity;
                    if let Some(m) = &mask {
                        a *= m.at(u, v);
                    }
                    if a <= 0.0 {
                        continue;
                    }
                    let p = src.sample(u * sw, v * sh);
                    let o = &mut row[x];
                    for k in 0..3 {
                        o[k] += (p[k] - o[k]) * a.min(1.0);
                    }
                }
            });
    }
    out
}

pub fn list(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .collect();
    v.sort();
    v
}

pub fn load(path: &Path) -> Result<Collage> {
    Ok(serde_json::from_str(&std::fs::read_to_string(path)?)?)
}

pub fn save(path: &Path, c: &Collage) -> Result<()> {
    std::fs::write(path, serde_json::to_string_pretty(c)?)?;
    Ok(())
}

pub fn next_path(dir: &Path) -> PathBuf {
    let max = list(dir)
        .iter()
        .filter_map(|p| {
            p.file_stem()?
                .to_str()?
                .strip_prefix("collage-")?
                .parse::<u32>()
                .ok()
        })
        .max()
        .unwrap_or(0);
    dir.join(format!("collage-{:04}.json", max + 1))
}

/// A file-name-safe version of a collage name ("" if nothing usable is left).
pub fn file_stem_for(name: &str) -> String {
    let s: String = name
        .trim()
        .chars()
        .map(|c| {
            if matches!(c, '/' | '\\' | ':' | '\0') || c.is_control() {
                '-'
            } else {
                c
            }
        })
        .collect();
    s.trim_start_matches('.').trim().to_string()
}

/// A free `<name>.json` path in `dir` (adds `-1`, `-2` … if taken; also avoids a stray `.jpg`).
pub fn path_for_name(dir: &Path, name: &str) -> PathBuf {
    let stem = file_stem_for(name);
    let stem = if stem.is_empty() {
        "collage".to_string()
    } else {
        stem
    };
    let free =
        |s: &str| !dir.join(format!("{s}.json")).exists() && !dir.join(format!("{s}.jpg")).exists();
    if free(&stem) {
        return dir.join(format!("{stem}.json"));
    }
    (1..)
        .map(|k| format!("{stem}-{k}"))
        .find(|s| free(s))
        .map(|s| dir.join(format!("{s}.json")))
        .unwrap()
}

/// Rename a collage: its `.json` (and exported `.jpg`) get the new name, and the document's
/// name is updated. Returns the new path.
pub fn rename(path: &Path, new_name: &str) -> Result<PathBuf> {
    let stem = file_stem_for(new_name);
    if stem.is_empty() {
        anyhow::bail!("invalid collage name");
    }
    let dir = path.parent().unwrap_or(Path::new("."));
    let dest = dir.join(format!("{stem}.json"));
    let same = dest == path;
    if !same && (dest.exists() || dest.with_extension("jpg").exists()) {
        anyhow::bail!("a collage named {stem} already exists");
    }
    let mut c = load(path)?;
    c.name = new_name.trim().to_string();
    save(path, &c)?;
    if !same {
        std::fs::rename(path, &dest)?;
        let jpg = path.with_extension("jpg");
        if jpg.exists() {
            std::fs::rename(&jpg, dest.with_extension("jpg"))?;
        }
    }
    Ok(dest)
}

/// Stacking moves for a layer (the last layer is drawn on top).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stack {
    Front,
    Forward,
    Backward,
    Back,
}

impl Collage {
    /// Move layer `id` to index `to` (0 = bottom). Returns false if it isn't there.
    pub fn move_layer(&mut self, id: u32, to: usize) -> bool {
        let Some(i) = self.layers.iter().position(|l| l.id == id) else {
            return false;
        };
        let l = self.layers.remove(i);
        let to = to.min(self.layers.len());
        self.layers.insert(to, l);
        true
    }

    pub fn restack(&mut self, id: u32, s: Stack) -> bool {
        let Some(i) = self.layers.iter().position(|l| l.id == id) else {
            return false;
        };
        let last = self.layers.len() - 1;
        let to = match s {
            Stack::Front => last,
            Stack::Forward => (i + 1).min(last),
            Stack::Backward => i.saturating_sub(1),
            Stack::Back => 0,
        };
        self.move_layer(id, to)
    }
}
