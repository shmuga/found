//! Float RGB image buffer used by the whole pipeline.
//!
//! Pixels are stored display-encoded (sRGB curve) in 0..1. Ops that need linear light
//! (white balance, exposure) decode/encode locally.

use rayon::prelude::*;

#[derive(Clone, Debug, PartialEq)]
pub struct Img {
    pub w: usize,
    pub h: usize,
    pub px: Vec<[f32; 3]>,
}

impl Img {
    pub fn new(w: usize, h: usize) -> Self {
        Self {
            w,
            h,
            px: vec![[0.0; 3]; w * h],
        }
    }

    pub fn filled(w: usize, h: usize, c: [f32; 3]) -> Self {
        Self {
            w,
            h,
            px: vec![c; w * h],
        }
    }

    pub fn from_rgb8(w: usize, h: usize, data: &[u8]) -> Self {
        let px = data
            .par_chunks_exact(3)
            .map(|c| {
                [
                    c[0] as f32 / 255.0,
                    c[1] as f32 / 255.0,
                    c[2] as f32 / 255.0,
                ]
            })
            .collect();
        Self { w, h, px }
    }

    pub fn to_rgb8(&self) -> Vec<u8> {
        let mut out = vec![0u8; self.w * self.h * 3];
        out.par_chunks_exact_mut(3)
            .zip(self.px.par_iter())
            .for_each(|(o, p)| {
                o[0] = to_u8(p[0]);
                o[1] = to_u8(p[1]);
                o[2] = to_u8(p[2]);
            });
        out
    }

    pub fn to_color_image(&self) -> egui::ColorImage {
        egui::ColorImage::from_rgb([self.w, self.h], &self.to_rgb8())
    }

    pub fn long_edge(&self) -> usize {
        self.w.max(self.h)
    }

    #[inline]
    pub fn get(&self, x: usize, y: usize) -> [f32; 3] {
        self.px[y * self.w + x]
    }

    /// Clamped integer fetch.
    #[inline]
    pub fn get_clamped(&self, x: isize, y: isize) -> [f32; 3] {
        let x = x.clamp(0, self.w as isize - 1) as usize;
        let y = y.clamp(0, self.h as isize - 1) as usize;
        self.px[y * self.w + x]
    }

    /// Bilinear sample at pixel-centre coordinates (x + 0.5 is the centre of pixel x).
    #[inline]
    pub fn sample(&self, fx: f32, fy: f32) -> [f32; 3] {
        let x = fx - 0.5;
        let y = fy - 0.5;
        let x0 = x.floor();
        let y0 = y.floor();
        let tx = x - x0;
        let ty = y - y0;
        let (x0, y0) = (x0 as isize, y0 as isize);
        let a = self.get_clamped(x0, y0);
        let b = self.get_clamped(x0 + 1, y0);
        let c = self.get_clamped(x0, y0 + 1);
        let d = self.get_clamped(x0 + 1, y0 + 1);
        let mut o = [0.0; 3];
        for k in 0..3 {
            let top = a[k] + (b[k] - a[k]) * tx;
            let bot = c[k] + (d[k] - c[k]) * tx;
            o[k] = top + (bot - top) * ty;
        }
        o
    }

    /// Area-average downscale so the long edge is at most `max_edge`. Returns a clone when
    /// already small enough.
    pub fn fit(&self, max_edge: usize) -> Img {
        let long = self.long_edge();
        if long <= max_edge || long == 0 {
            return self.clone();
        }
        let s = max_edge as f32 / long as f32;
        let nw = ((self.w as f32 * s).round() as usize).max(1);
        let nh = ((self.h as f32 * s).round() as usize).max(1);
        self.resize_area(nw, nh)
    }

    /// Box-filter resample (good for downscaling, acceptable for small upscales).
    pub fn resize_area(&self, nw: usize, nh: usize) -> Img {
        let sx = self.w as f32 / nw as f32;
        let sy = self.h as f32 / nh as f32;
        if sx <= 1.0 && sy <= 1.0 {
            return self.resize_bilinear(nw, nh);
        }
        let mut out = Img::new(nw, nh);
        out.px.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
            let y0 = (y as f32 * sy) as usize;
            let y1 = (((y + 1) as f32 * sy).ceil() as usize)
                .min(self.h)
                .max(y0 + 1);
            for (x, o) in row.iter_mut().enumerate() {
                let x0 = (x as f32 * sx) as usize;
                let x1 = (((x + 1) as f32 * sx).ceil() as usize)
                    .min(self.w)
                    .max(x0 + 1);
                let mut acc = [0.0f32; 3];
                for yy in y0..y1 {
                    let base = yy * self.w;
                    for xx in x0..x1 {
                        let p = self.px[base + xx];
                        acc[0] += p[0];
                        acc[1] += p[1];
                        acc[2] += p[2];
                    }
                }
                let n = ((y1 - y0) * (x1 - x0)) as f32;
                *o = [acc[0] / n, acc[1] / n, acc[2] / n];
            }
        });
        out
    }

    pub fn resize_bilinear(&self, nw: usize, nh: usize) -> Img {
        let sx = self.w as f32 / nw as f32;
        let sy = self.h as f32 / nh as f32;
        let mut out = Img::new(nw, nh);
        out.px.par_chunks_mut(nw).enumerate().for_each(|(y, row)| {
            let fy = (y as f32 + 0.5) * sy;
            for (x, o) in row.iter_mut().enumerate() {
                *o = self.sample((x as f32 + 0.5) * sx, fy);
            }
        });
        out
    }

    /// Luminance plane.
    pub fn luma_plane(&self) -> Vec<f32> {
        self.px.par_iter().map(|p| luma(*p)).collect()
    }

    /// Mean colour over a square window (radius in px) around a normalised point.
    pub fn mean_around(&self, u: f32, v: f32, radius: usize) -> [f32; 3] {
        let cx = (u * self.w as f32) as isize;
        let cy = (v * self.h as f32) as isize;
        let r = radius as isize;
        let mut acc = [0.0f32; 3];
        let mut n = 0.0;
        for y in cy - r..=cy + r {
            for x in cx - r..=cx + r {
                let p = self.get_clamped(x, y);
                acc[0] += p[0];
                acc[1] += p[1];
                acc[2] += p[2];
                n += 1.0;
            }
        }
        [acc[0] / n, acc[1] / n, acc[2] / n]
    }
}

#[inline]
pub fn to_u8(v: f32) -> u8 {
    (v.clamp(0.0, 1.0) * 255.0 + 0.5) as u8
}

#[inline]
pub fn luma(c: [f32; 3]) -> f32 {
    0.2126 * c[0] + 0.7152 * c[1] + 0.0722 * c[2]
}

#[inline]
pub fn srgb_to_linear(v: f32) -> f32 {
    if v <= 0.04045 {
        v / 12.92
    } else {
        ((v + 0.055) / 1.055).powf(2.4)
    }
}

#[inline]
pub fn linear_to_srgb(v: f32) -> f32 {
    if v <= 0.0031308 {
        v * 12.92
    } else {
        1.055 * v.powf(1.0 / 2.4) - 0.055
    }
}

/// Deterministic hash → 0..1 (splitmix64). Used by glitch ops so results don't depend on
/// thread scheduling and look the same at proxy and full resolution.
#[inline]
pub fn hash01(seed: u32, a: u32, b: u32) -> f32 {
    let mut z = (seed as u64) << 40 ^ (a as u64) << 20 ^ (b as u64) ^ 0x9E37_79B9_7F4A_7C15;
    z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
    z ^= z >> 31;
    (z >> 40) as f32 / (1u64 << 24) as f32
}

/// Smooth 1D value noise in 0..1.
pub fn noise1(seed: u32, x: f32) -> f32 {
    let i = x.floor();
    let t = x - i;
    let t = t * t * (3.0 - 2.0 * t);
    let a = hash01(seed, i as i32 as u32, 7);
    let b = hash01(seed, (i as i32 + 1) as u32, 7);
    a + (b - a) * t
}
