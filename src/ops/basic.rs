//! Standard adjustments: crop/rotate, white balance, exposure, brightness/contrast, clarity,
//! saturation and blending.
//!
//! Clarity follows the approach in photocraft's Camera Raw develop (MIT/Apache-2.0): a guided
//! filter (He, Sun, Tang — *Guided Image Filtering*, ECCV 2010) on luminance with a radius of
//! 2 % of the long edge gives an edge-preserving base layer; the detail (luma − base) is added
//! back, weighted toward the midtones.

use super::Crop;
use crate::img::{Img, linear_to_srgb, luma, srgb_to_linear};
use rayon::prelude::*;

// ---------------------------------------------------------------- crop / rotate

pub fn rotate90(src: &Img, quarter: u8) -> Img {
    match quarter % 4 {
        0 => src.clone(),
        2 => {
            let mut px = src.px.clone();
            px.reverse();
            Img {
                w: src.w,
                h: src.h,
                px,
            }
        }
        q => {
            let (w, h) = (src.h, src.w);
            let mut out = Img::new(w, h);
            out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
                for (x, o) in row.iter_mut().enumerate() {
                    // q == 1: clockwise. dest (x,y) <- src (y, H-1-x)
                    *o = if q == 1 {
                        src.get(y, src.h - 1 - x)
                    } else {
                        src.get(src.w - 1 - y, x)
                    };
                }
            });
            out
        }
    }
}

pub fn flip_h(mut img: Img) -> Img {
    let w = img.w;
    img.px.par_chunks_mut(w).for_each(|row| row.reverse());
    img
}

/// Rotate by `deg` about the centre, zooming so no empty corners appear.
pub fn straighten(src: &Img, deg: f32) -> Img {
    if deg == 0.0 {
        return src.clone();
    }
    let a = deg.to_radians();
    let (c, s) = (a.cos().abs(), a.sin().abs());
    let (w, h) = (src.w as f32, src.h as f32);
    let zoom = ((w * c + h * s) / w).max((w * s + h * c) / h);
    let (ca, sa) = (a.cos(), a.sin());
    let (cx, cy) = (w * 0.5, h * 0.5);
    let mut out = Img::new(src.w, src.h);
    out.px
        .par_chunks_mut(src.w)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, o) in row.iter_mut().enumerate() {
                let dx = (x as f32 + 0.5 - cx) / zoom;
                let dy = (y as f32 + 0.5 - cy) / zoom;
                let sx = ca * dx + sa * dy + cx;
                let sy = -sa * dx + ca * dy + cy;
                *o = src.sample(sx, sy);
            }
        });
    out
}

/// Geometry part of the crop node (rotation + flip) without cutting the rect.
pub fn crop_geometry(src: &Img, c: &Crop) -> Img {
    let mut img = rotate90(src, c.rot90);
    if c.flip_h {
        img = flip_h(img);
    }
    straighten(&img, c.angle)
}

pub fn crop(src: &Img, c: &Crop) -> Img {
    let img = if c.rot90.is_multiple_of(4) && !c.flip_h && c.angle == 0.0 {
        None
    } else {
        Some(crop_geometry(src, c))
    };
    let img = img.as_ref().unwrap_or(src);
    let [x, y, w, h] = c.rect;
    let x0 = ((x.clamp(0.0, 1.0) * img.w as f32).round() as usize).min(img.w - 1);
    let y0 = ((y.clamp(0.0, 1.0) * img.h as f32).round() as usize).min(img.h - 1);
    let cw = ((w * img.w as f32).round() as usize).clamp(1, img.w - x0);
    let ch = ((h * img.h as f32).round() as usize).clamp(1, img.h - y0);
    if x0 == 0 && y0 == 0 && cw == img.w && ch == img.h {
        return img.clone();
    }
    let mut out = Img::new(cw, ch);
    out.px.par_chunks_mut(cw).enumerate().for_each(|(yy, row)| {
        let s = (y0 + yy) * img.w + x0;
        row.copy_from_slice(&img.px[s..s + cw]);
    });
    out
}

// ---------------------------------------------------------------- white balance

const WB_TEMP_STOPS: f32 = 1.0;
const WB_TINT_STOPS: f32 = 0.75;

/// Linear-light channel gains for temp/tint in −100..100. Normalised to keep luminance.
pub fn wb_gains(temp: f32, tint: f32) -> [f32; 3] {
    let t = temp / 100.0;
    let n = tint / 100.0;
    let g = [
        (WB_TEMP_STOPS * t).exp2(),
        (-WB_TINT_STOPS * n).exp2(),
        (-WB_TEMP_STOPS * t).exp2(),
    ];
    let l = 0.2126 * g[0] + 0.7152 * g[1] + 0.0722 * g[2];
    [g[0] / l, g[1] / l, g[2] / l]
}

/// Temp/tint that would make the given (display-encoded) colour neutral.
pub fn wb_solve(c: [f32; 3]) -> (f32, f32) {
    let lin = c.map(|v| srgb_to_linear(v).max(1e-4).log2());
    let [r, g, b] = lin;
    let t = (b - r) / (2.0 * WB_TEMP_STOPS);
    let n = (g - (r + b) * 0.5) / WB_TINT_STOPS;
    (
        (t * 100.0).clamp(-100.0, 100.0),
        (n * 100.0).clamp(-100.0, 100.0),
    )
}

/// Estimate the scene neutral. `white_patch`: mean of the brightest few percent (good for
/// scans on a white lid); otherwise a robust grey-world mean.
pub fn wb_estimate(img: &Img, white_patch: bool) -> [f32; 3] {
    let step = (img.px.len() / 200_000).max(1);
    let mut samples: Vec<([f32; 3], f32)> = img
        .px
        .iter()
        .step_by(step)
        .map(|p| (*p, luma(*p)))
        .collect();
    samples.sort_by(|a, b| a.1.total_cmp(&b.1));
    let n = samples.len();
    let (lo, hi) = if white_patch {
        // Skip clipped highlights.
        let hi = samples
            .iter()
            .rposition(|s| s.1 < 0.995)
            .map(|i| i + 1)
            .unwrap_or(n);
        ((hi as f32 * 0.95) as usize, hi)
    } else {
        ((n as f32 * 0.02) as usize, (n as f32 * 0.98) as usize)
    };
    let slice = &samples[lo.min(n.saturating_sub(1))..hi.max(lo + 1).min(n)];
    let mut acc = [0.0f32; 3];
    for (p, _) in slice {
        for k in 0..3 {
            acc[k] += srgb_to_linear(p[k]);
        }
    }
    let m = slice.len().max(1) as f32;
    acc.map(|v| linear_to_srgb(v / m))
}

pub fn white_balance(mut img: Img, temp: f32, tint: f32) -> Img {
    if temp == 0.0 && tint == 0.0 {
        return img;
    }
    let g = wb_gains(temp, tint);
    img.px.par_iter_mut().for_each(|p| {
        for k in 0..3 {
            p[k] = linear_to_srgb(srgb_to_linear(p[k]) * g[k]);
        }
    });
    img
}

// ---------------------------------------------------------------- tone

pub fn exposure(mut img: Img, ev: f32) -> Img {
    if ev == 0.0 {
        return img;
    }
    let m = ev.exp2();
    img.px.par_iter_mut().for_each(|p| {
        for k in 0..3 {
            p[k] = linear_to_srgb(srgb_to_linear(p[k]) * m);
        }
    });
    img
}

fn smoothstep(x: f32) -> f32 {
    x * x * (3.0 - 2.0 * x)
}

fn inv_smoothstep(y: f32) -> f32 {
    0.5 - ((1.0 - 2.0 * y).clamp(-1.0, 1.0).asin() / 3.0).sin()
}

pub fn contrast_curve(l: f32, k: f32) -> f32 {
    let l = l.clamp(0.0, 1.0);
    if k >= 0.0 {
        l + (smoothstep(l) - l) * k
    } else {
        l + (inv_smoothstep(l) - l) * -k
    }
}

pub fn brightness_contrast(mut img: Img, brightness: f32, contrast: f32) -> Img {
    if brightness == 0.0 && contrast == 0.0 {
        return img;
    }
    let gamma = (-brightness / 100.0).exp2();
    let k = contrast / 100.0;
    img.px.par_iter_mut().for_each(|p| {
        if brightness != 0.0 {
            for k in 0..3 {
                p[k] = p[k].max(0.0).powf(gamma);
            }
        }
        if k != 0.0 {
            let l = luma(*p);
            let d = contrast_curve(l, k) - l;
            for c in 0..3 {
                p[c] += d;
            }
        }
    });
    img
}

pub fn saturation(mut img: Img, sat: f32, vib: f32) -> Img {
    if sat == 0.0 && vib == 0.0 {
        return img;
    }
    let s = sat / 100.0;
    let v = vib / 100.0;
    img.px.par_iter_mut().for_each(|p| {
        let l = luma(*p);
        let mx = p[0].max(p[1]).max(p[2]);
        let mn = p[0].min(p[1]).min(p[2]);
        let cur = if mx > 0.0 { (mx - mn) / mx } else { 0.0 };
        let f = 1.0 + s + v * (1.0 - cur);
        for k in 0..3 {
            p[k] = l + (p[k] - l) * f.max(0.0);
        }
    });
    img
}

// ---------------------------------------------------------------- clarity

/// Box mean with radius r (window 2r+1), clamped edges. Two separable sliding-window passes.
pub fn box_mean(v: &[f32], w: usize, h: usize, r: usize) -> Vec<f32> {
    let mut tmp = vec![0.0f32; w * h];
    tmp.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let src = &v[y * w..(y + 1) * w];
        let mut acc = 0.0f32;
        let n = (2 * r + 1) as f32;
        let at = |i: isize| src[i.clamp(0, w as isize - 1) as usize];
        for i in -(r as isize)..=(r as isize) {
            acc += at(i);
        }
        for x in 0..w {
            row[x] = acc / n;
            acc += at(x as isize + r as isize + 1) - at(x as isize - r as isize);
        }
    });
    let mut out = vec![0.0f32; w * h];
    // Vertical pass, column blocks in parallel.
    const BLK: usize = 64;
    let cols: Vec<usize> = (0..w).step_by(BLK).collect();
    let n = (2 * r + 1) as f32;
    let results: Vec<(usize, Vec<f32>)> = cols
        .par_iter()
        .map(|&c0| {
            let bw = BLK.min(w - c0);
            let mut col_out = vec![0.0f32; bw * h];
            let at = |y: isize, x: usize| tmp[(y.clamp(0, h as isize - 1) as usize) * w + c0 + x];
            let mut acc = vec![0.0f32; bw];
            for y in -(r as isize)..=(r as isize) {
                for x in 0..bw {
                    acc[x] += at(y, x);
                }
            }
            for y in 0..h {
                for x in 0..bw {
                    col_out[y * bw + x] = acc[x] / n;
                    acc[x] += at(y as isize + r as isize + 1, x) - at(y as isize - r as isize, x);
                }
            }
            (c0, col_out)
        })
        .collect();
    for (c0, col) in results {
        let bw = BLK.min(w - c0);
        for y in 0..h {
            out[y * w + c0..y * w + c0 + bw].copy_from_slice(&col[y * bw..(y + 1) * bw]);
        }
    }
    out
}

/// Self-guided filter (I = p) computed on a subsampled grid for speed.
pub fn guided_self(l: &[f32], w: usize, h: usize, r: usize, eps: f32) -> Vec<f32> {
    let s = (r / 8).clamp(1, 8);
    let (sw, sh) = (w.div_ceil(s), h.div_ceil(s));
    let mut small = vec![0.0f32; sw * sh];
    if s == 1 {
        small.copy_from_slice(l);
    } else {
        small.par_chunks_mut(sw).enumerate().for_each(|(sy, row)| {
            for (sx, o) in row.iter_mut().enumerate() {
                let mut acc = 0.0;
                let mut n = 0.0;
                for y in sy * s..((sy + 1) * s).min(h) {
                    for x in sx * s..((sx + 1) * s).min(w) {
                        acc += l[y * w + x];
                        n += 1.0;
                    }
                }
                *o = acc / n;
            }
        });
    }
    let rs = (r / s).max(1);
    let mi = box_mean(&small, sw, sh, rs);
    let ii: Vec<f32> = small.par_iter().map(|v| v * v).collect();
    let mii = box_mean(&ii, sw, sh, rs);
    let a: Vec<f32> = (0..sw * sh)
        .into_par_iter()
        .map(|k| {
            let var = mii[k] - mi[k] * mi[k];
            var / (var + eps)
        })
        .collect();
    let b: Vec<f32> = (0..sw * sh)
        .into_par_iter()
        .map(|k| mi[k] - a[k] * mi[k])
        .collect();
    let ma = box_mean(&a, sw, sh, rs);
    let mb = box_mean(&b, sw, sh, rs);
    let mut out = vec![0.0f32; w * h];
    let sf = s as f32;
    out.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let fy = ((y as f32 + 0.5) / sf - 0.5).clamp(0.0, (sh - 1) as f32);
        let y0 = fy.floor() as usize;
        let y1 = (y0 + 1).min(sh - 1);
        let ty = fy - y0 as f32;
        for (x, o) in row.iter_mut().enumerate() {
            let fx = ((x as f32 + 0.5) / sf - 0.5).clamp(0.0, (sw - 1) as f32);
            let x0 = fx.floor() as usize;
            let x1 = (x0 + 1).min(sw - 1);
            let tx = fx - x0 as f32;
            let bil = |m: &[f32]| {
                let t = m[y0 * sw + x0] + (m[y0 * sw + x1] - m[y0 * sw + x0]) * tx;
                let b = m[y1 * sw + x0] + (m[y1 * sw + x1] - m[y1 * sw + x0]) * tx;
                t + (b - t) * ty
            };
            *o = bil(&ma) * l[y * w + x] + bil(&mb);
        }
    });
    out
}

pub fn clarity(mut img: Img, amount: f32) -> Img {
    if amount == 0.0 {
        return img;
    }
    let (w, h) = (img.w, img.h);
    let l: Vec<f32> = img
        .px
        .par_iter()
        .map(|p| luma(*p).clamp(0.0, 1.0))
        .collect();
    let r = ((img.long_edge() as f32 * 0.02) as usize).max(3);
    let base = guided_self(&l, w, h, r, 0.005);
    let k = amount / 100.0 * 0.8 * 1.5;
    img.px.par_iter_mut().enumerate().for_each(|(i, p)| {
        let lv = l[i];
        let mw = 1.0 - (2.0 * lv - 1.0).powi(2);
        let d = k * (lv - base[i]) * mw;
        for c in 0..3 {
            p[c] += d;
        }
    });
    img
}

// ---------------------------------------------------------------- blend

pub fn blend(mut a: Img, b: &Img, mode: usize, mix: f32) -> Img {
    let resized;
    let b = if b.w != a.w || b.h != a.h {
        resized = b.resize_bilinear(a.w, a.h);
        &resized
    } else {
        b
    };
    a.px.par_iter_mut().zip(b.px.par_iter()).for_each(|(p, q)| {
        for k in 0..3 {
            let (x, y) = (p[k].clamp(0.0, 1.0), q[k].clamp(0.0, 1.0));
            let r = match mode {
                1 => x * y,
                2 => 1.0 - (1.0 - x) * (1.0 - y),
                3 => {
                    if x < 0.5 {
                        2.0 * x * y
                    } else {
                        1.0 - 2.0 * (1.0 - x) * (1.0 - y)
                    }
                }
                4 => (x - y).abs(),
                5 => (x + y).min(1.0),
                6 => x.max(y),
                7 => x.min(y),
                _ => y,
            };
            p[k] = x + (r - x) * mix;
        }
    });
    a
}

/// Keep `a` where the mask is light and `b` where it's dark. The mask is `m`'s luma, or `b`'s
/// own luma without one. `threshold` ± `softness` is the transition.
pub fn mask(
    mut a: Img,
    b: &Img,
    m: Option<&Img>,
    threshold: f32,
    softness: f32,
    invert: bool,
) -> Img {
    let fit = |x: &Img| {
        if x.w != a.w || x.h != a.h {
            Some(x.resize_bilinear(a.w, a.h))
        } else {
            None
        }
    };
    let b_fit = fit(b);
    let b = b_fit.as_ref().unwrap_or(b);
    let m_fit = m.and_then(fit);
    let m = m_fit.as_ref().or(m).unwrap_or(b);
    let (lo, hi) = (
        threshold - softness.max(1e-3),
        threshold + softness.max(1e-3),
    );
    a.px.par_iter_mut()
        .zip(b.px.par_iter())
        .zip(m.px.par_iter())
        .for_each(|((p, q), mm)| {
            let l = crate::img::luma(*mm).clamp(0.0, 1.0);
            let t = ((l - lo) / (hi - lo)).clamp(0.0, 1.0);
            let mut w = t * t * (3.0 - 2.0 * t);
            if invert {
                w = 1.0 - w;
            }
            for k in 0..3 {
                p[k] = q[k] + (p[k] - q[k]) * w;
            }
        });
    a
}

/// Split into three images that add back up to `src`: R / G / B channels (mode 0) or
/// shadows / mids / highlights by luma (mode 1).
pub fn split(src: &Img, mode: usize) -> Vec<Img> {
    let mut outs = vec![
        Img::new(src.w, src.h),
        Img::new(src.w, src.h),
        Img::new(src.w, src.h),
    ];
    let smooth = |e0: f32, e1: f32, x: f32| {
        let t = ((x - e0) / (e1 - e0)).clamp(0.0, 1.0);
        t * t * (3.0 - 2.0 * t)
    };
    for (k, out) in outs.iter_mut().enumerate() {
        out.px
            .par_iter_mut()
            .zip(src.px.par_iter())
            .for_each(|(o, p)| {
                *o = if mode == 0 {
                    let mut c = [0.0; 3];
                    c[k] = p[k];
                    c
                } else {
                    let l = crate::img::luma(*p).clamp(0.0, 1.0);
                    let ws = 1.0 - smooth(0.2, 0.45, l);
                    let wh = smooth(0.55, 0.8, l);
                    let w = [ws, (1.0 - ws - wh).max(0.0), wh][k];
                    [p[0] * w, p[1] * w, p[2] * w]
                };
            });
    }
    outs
}
