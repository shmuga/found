//! Experimental / glitch ops. All deterministic (seeded hashes of normalised coordinates).

use crate::img::{Img, hash01, luma, noise1};
use rayon::prelude::*;

pub fn rgb_split(src: &Img, amount_pct: f32, angle: f32) -> Img {
    let d = amount_pct / 100.0 * src.w as f32;
    let (dx, dy) = (angle.to_radians().cos() * d, angle.to_radians().sin() * d);
    let mut out = Img::new(src.w, src.h);
    out.px
        .par_chunks_mut(src.w)
        .enumerate()
        .for_each(|(y, row)| {
            let fy = y as f32 + 0.5;
            for (x, o) in row.iter_mut().enumerate() {
                let fx = x as f32 + 0.5;
                o[0] = src.sample(fx + dx, fy + dy)[0];
                o[1] = src.get(x, y)[1];
                o[2] = src.sample(fx - dx, fy - dy)[2];
            }
        });
    out
}

fn sort_key(p: [f32; 3], key: usize) -> f32 {
    match key {
        1 => {
            let (r, g, b) = (p[0], p[1], p[2]);
            let mx = r.max(g).max(b);
            let mn = r.min(g).min(b);
            let d = mx - mn;
            if d <= 1e-6 {
                0.0
            } else if mx == r {
                ((g - b) / d).rem_euclid(6.0)
            } else if mx == g {
                (b - r) / d + 2.0
            } else {
                (r - g) / d + 4.0
            }
        }
        2 => p[0],
        3 => {
            let mx = p[0].max(p[1]).max(p[2]);
            if mx > 0.0 {
                (mx - p[0].min(p[1]).min(p[2])) / mx
            } else {
                0.0
            }
        }
        _ => luma(p),
    }
}

fn sort_line(line: &mut [[f32; 3]], lo: f32, hi: f32, reverse: bool, key: usize) {
    let n = line.len();
    let mut i = 0;
    while i < n {
        let l = luma(line[i]);
        if l >= lo && l <= hi {
            let start = i;
            while i < n && {
                let l = luma(line[i]);
                l >= lo && l <= hi
            } {
                i += 1;
            }
            let run = &mut line[start..i];
            run.sort_by(|a, b| sort_key(*a, key).total_cmp(&sort_key(*b, key)));
            if reverse {
                run.reverse();
            }
        } else {
            i += 1;
        }
    }
}

pub fn pixel_sort(
    mut img: Img,
    lo: f32,
    hi: f32,
    vertical: bool,
    reverse: bool,
    key: usize,
) -> Img {
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    if !vertical {
        let w = img.w;
        img.px
            .par_chunks_mut(w)
            .for_each(|row| sort_line(row, lo, hi, reverse, key));
        return img;
    }
    let (w, h) = (img.w, img.h);
    let cols: Vec<Vec<[f32; 3]>> = (0..w)
        .into_par_iter()
        .map(|x| {
            let mut col: Vec<[f32; 3]> = (0..h).map(|y| img.px[y * w + x]).collect();
            sort_line(&mut col, lo, hi, reverse, key);
            col
        })
        .collect();
    img.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            *o = cols[x][y];
        }
    });
    img
}

pub fn slice_shift(src: &Img, bands: i32, shift_pct: f32, chance: f32, seed: u32) -> Img {
    let bands = bands.max(1) as u32;
    let mut out = Img::new(src.w, src.h);
    let w = src.w as isize;
    out.px
        .par_chunks_mut(src.w)
        .enumerate()
        .for_each(|(y, row)| {
            let v = (y as f32 + 0.5) / src.h as f32;
            // Jittered band index so bands have uneven heights.
            let jitter = noise1(seed ^ 0xA5, v * bands as f32 * 0.5) * 0.8;
            let band = ((v + jitter / bands as f32) * bands as f32) as u32;
            let hit = hash01(seed, band, 1) < chance;
            let off = if hit {
                ((hash01(seed, band, 2) * 2.0 - 1.0) * shift_pct / 100.0 * src.w as f32) as isize
            } else {
                0
            };
            let chroma = if hit && hash01(seed, band, 3) < 0.3 {
                (off / 3).max(1)
            } else {
                0
            };
            for (x, o) in row.iter_mut().enumerate() {
                let sx = (x as isize - off).rem_euclid(w) as usize;
                let mut p = src.get(sx, y);
                if chroma != 0 {
                    p[0] = src.get((sx as isize + chroma).rem_euclid(w) as usize, y)[0];
                }
                *o = p;
            }
        });
    out
}

pub fn block_glitch(src: &Img, block_pct: f32, chance: f32, mode: usize, seed: u32) -> Img {
    let bs = ((block_pct / 100.0 * src.long_edge() as f32) as usize).max(2);
    let (bw, bh) = (src.w.div_ceil(bs), src.h.div_ceil(bs));
    let mut out = src.clone();
    let w = src.w;
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let by = y / bs;
        for (x, o) in row.iter_mut().enumerate() {
            let bx = x / bs;
            let id = (by * bw + bx) as u32;
            if hash01(seed, id, 11) >= chance {
                continue;
            }
            let m = if mode == 3 {
                (hash01(seed, id, 12) * 3.0) as usize
            } else {
                mode
            };
            match m {
                0 => {
                    // Displace: copy from another block.
                    let tx = (hash01(seed, id, 13) * bw as f32) as usize;
                    let ty = (hash01(seed, id, 14) * bh as f32) as usize;
                    let sx = (tx * bs + x % bs).min(src.w - 1);
                    let sy = (ty * bs + y % bs).min(src.h - 1);
                    *o = src.get(sx, sy);
                }
                1 => {
                    // Smear: repeat the block's top row downward.
                    *o = src.get(x, by * bs);
                }
                _ => {
                    let p = src.get(x, y);
                    *o = match (hash01(seed, id, 15) * 3.0) as u32 {
                        0 => [p[1], p[2], p[0]],
                        1 => [p[2], p[0], p[1]],
                        _ => [1.0 - p[0], p[1], 1.0 - p[2]],
                    };
                }
            }
        }
    });
    out
}

pub fn bit_crush(mut img: Img, levels: i32) -> Img {
    let n = (levels.max(2) - 1) as f32;
    img.px.par_iter_mut().for_each(|p| {
        for k in 0..3 {
            p[k] = (p[k].clamp(0.0, 1.0) * n).round() / n;
        }
    });
    img
}

pub fn vhs(src: &Img, jitter: f32, bleed: f32, lines: f32, noise: f32, seed: u32) -> Img {
    let (w, h) = (src.w, src.h);
    let mut out = Img::new(w, h);
    let max_shift = jitter * 0.02 * w as f32;
    let bleed_r = (bleed * 0.012 * w as f32) as isize;
    let line_period = (h as f32 / 240.0).max(2.0);
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let v = y as f32 / h as f32;
        // Slow wobble + occasional tracking tear.
        let wob = (noise1(seed, v * 12.0) - 0.5) * 2.0 * max_shift;
        let tear = if hash01(seed, (v * 40.0) as u32, 5) < 0.06 * jitter {
            max_shift * 3.0
        } else {
            0.0
        };
        let off = wob + tear;
        // Shifted row in YIQ-ish (luma + chroma); chroma box-blurred horizontally.
        let shifted: Vec<[f32; 3]> = (0..w)
            .map(|x| src.sample(x as f32 + 0.5 - off, y as f32 + 0.5))
            .collect();
        let yv: Vec<f32> = shifted.iter().map(|p| luma(*p)).collect();
        let mut prefix = vec![[0.0f32; 3]; w + 1];
        for x in 0..w {
            let p = shifted[x];
            let c = [p[0] - yv[x], p[1] - yv[x], p[2] - yv[x]];
            for k in 0..3 {
                prefix[x + 1][k] = prefix[x][k] + c[k];
            }
        }
        let dark = if (y as f32 / line_period).fract() < 0.5 {
            1.0 - lines * 0.35
        } else {
            1.0
        };
        for (x, o) in row.iter_mut().enumerate() {
            let a = (x as isize - bleed_r).max(0) as usize;
            let b = ((x as isize + bleed_r * 2 + 1) as usize).min(w);
            let n = (b - a) as f32;
            let nz = (hash01(seed, (x * 31 + y * 7919) as u32, 9) - 0.5) * noise * 0.4;
            for k in 0..3 {
                let c = (prefix[b][k] - prefix[a][k]) / n;
                o[k] = ((yv[x] + nz) + c * 1.15) * dark;
            }
        }
    });
    out
}

const BAYER8: [[u8; 8]; 8] = [
    [0, 32, 8, 40, 2, 34, 10, 42],
    [48, 16, 56, 24, 50, 18, 58, 26],
    [12, 44, 4, 36, 14, 46, 6, 38],
    [60, 28, 52, 20, 62, 30, 54, 22],
    [3, 35, 11, 43, 1, 33, 9, 41],
    [51, 19, 59, 27, 49, 17, 57, 25],
    [15, 47, 7, 39, 13, 45, 5, 37],
    [63, 31, 55, 23, 61, 29, 53, 21],
];

pub fn dither(mut img: Img, levels: i32, scale: i32, mono: bool) -> Img {
    let n = (levels.max(2) - 1) as f32;
    // Dot scale relative to a 1000px-wide reference.
    let s = ((scale.max(1) as f32 * img.long_edge() as f32 / 1000.0).round() as usize).max(1);
    let w = img.w;
    img.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, p) in row.iter_mut().enumerate() {
            let t = (BAYER8[(y / s) % 8][(x / s) % 8] as f32 + 0.5) / 64.0 - 0.5;
            if mono {
                let l = luma(*p);
                let v = ((l * n + t).round() / n).clamp(0.0, 1.0);
                *p = [v; 3];
            } else {
                for k in 0..3 {
                    p[k] = ((p[k] * n + t).round() / n).clamp(0.0, 1.0);
                }
            }
        }
    });
    img
}

pub fn pixelate(src: &Img, cell_pct: f32) -> Img {
    let cs = ((cell_pct / 100.0 * src.long_edge() as f32) as usize).max(1);
    if cs <= 1 {
        return src.clone();
    }
    let (cw, ch) = (src.w.div_ceil(cs), src.h.div_ceil(cs));
    let small = src.resize_area(cw, ch);
    let mut out = Img::new(src.w, src.h);
    out.px
        .par_chunks_mut(src.w)
        .enumerate()
        .for_each(|(y, row)| {
            for (x, o) in row.iter_mut().enumerate() {
                *o = small.get((x / cs).min(cw - 1), (y / cs).min(ch - 1));
            }
        });
    out
}

pub fn grain(mut img: Img, amount: f32, size: f32, seed: u32) -> Img {
    let s = (size * img.long_edge() as f32 / 2000.0).max(1.0);
    let w = img.w;
    img.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        let gy = (y as f32 / s) as u32;
        for (x, p) in row.iter_mut().enumerate() {
            let gx = (x as f32 / s) as u32;
            let n = (hash01(seed, gx, gy) - 0.5) * amount * 0.5;
            let l = luma(*p);
            let wgt = 1.0 - (2.0 * l - 1.0).powi(2) * 0.6;
            for k in 0..3 {
                p[k] += n * wgt;
            }
        }
    });
    img
}

pub fn mirror(src: &Img, mode: usize) -> Img {
    let (w, h) = (src.w, src.h);
    let mut out = Img::new(w, h);
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let (mx, my) = (w - 1 - x, h - 1 - y);
            *o = match mode {
                0 => src.get(if x < w / 2 { x } else { mx }, y),
                1 => src.get(if x >= w / 2 { x } else { mx }, y),
                2 => src.get(x, if y < h / 2 { y } else { my }),
                3 => src.get(
                    if x < w / 2 { x } else { mx },
                    if y < h / 2 { y } else { my },
                ),
                _ => {
                    // Kaleido: 6-fold polar mirror around the centre.
                    let (cx, cy) = (w as f32 * 0.5, h as f32 * 0.5);
                    let (dx, dy) = (x as f32 + 0.5 - cx, y as f32 + 0.5 - cy);
                    let r = (dx * dx + dy * dy).sqrt();
                    let seg = std::f32::consts::PI / 3.0;
                    let mut a = dy.atan2(dx).rem_euclid(seg * 2.0);
                    if a > seg {
                        a = seg * 2.0 - a;
                    }
                    src.sample(cx + r * a.cos(), cy + r * a.sin())
                }
            };
        }
    });
    out
}

pub fn channel_swap(mut img: Img, mode: usize) -> Img {
    img.px.par_iter_mut().for_each(|p| {
        let [r, g, b] = *p;
        *p = match mode {
            0 => [b, r, g],
            1 => [g, b, r],
            2 => [b, g, r],
            3 => [r, b, g],
            _ => [g, r, b],
        };
    });
    img
}

pub fn invert(mut img: Img, mix: f32) -> Img {
    img.px.par_iter_mut().for_each(|p| {
        for k in 0..3 {
            p[k] += (1.0 - 2.0 * p[k]) * mix;
        }
    });
    img
}

pub fn wave(src: &Img, amp_pct: f32, freq: f32, vertical: bool, phase: f32) -> Img {
    let (w, h) = (src.w, src.h);
    let a = amp_pct / 100.0 * w.max(h) as f32;
    let tau = std::f32::consts::TAU;
    let mut out = Img::new(w, h);
    out.px.par_chunks_mut(w).enumerate().for_each(|(y, row)| {
        for (x, o) in row.iter_mut().enumerate() {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            *o = if vertical {
                let d = (tau * (freq * fx / w as f32 + phase)).sin() * a;
                src.sample(fx, fy + d)
            } else {
                let d = (tau * (freq * fy / h as f32 + phase)).sin() * a;
                src.sample(fx + d, fy)
            };
        }
    });
    out
}
