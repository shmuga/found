//! Renders the app icon (1024×1024 PNG). Three original designs, all drawn from code (no
//! third-party artwork):
//!
//! - `a` "Scan bed": a grey app window with a scanned copper coin above a patch object.
//! - `b` "Patch cord": a coin on a dark slate tile, a glowing scan beam across it and a yellow
//!   patch cord into an object box.
//! - `c` "Glitch": an RGB-split, slice-shifted coin on an indigo → magenta tile.
//!
//! `cargo run --release --example icon -- <a|b|c> out.png`
//! `cargo run --release --example icon -- from <picture> out.png` — fit your own square artwork
//! into the macOS icon shape (824 px rounded square on a 1024 canvas, soft shadow).

use image::{Rgba, RgbaImage};
use rayon::prelude::*;

type C = [f32; 4]; // straight RGBA, 0..1

/// Signed distance to a rounded rectangle (negative inside).
fn rrect(x: f32, y: f32, x0: f32, y0: f32, x1: f32, y1: f32, r: f32) -> f32 {
    let (cx, cy) = ((x0 + x1) / 2.0, (y0 + y1) / 2.0);
    let (hx, hy) = ((x1 - x0) / 2.0 - r, (y1 - y0) / 2.0 - r);
    let (dx, dy) = ((x - cx).abs() - hx, (y - cy).abs() - hy);
    (dx.max(0.0).powi(2) + dy.max(0.0).powi(2)).sqrt() + dx.max(dy).min(0.0) - r
}

fn circle(x: f32, y: f32, cx: f32, cy: f32, r: f32) -> f32 {
    ((x - cx).powi(2) + (y - cy).powi(2)).sqrt() - r
}

/// Distance to a quadratic Bézier, approximated by sampling (fine for an icon).
fn bezier(x: f32, y: f32, p0: (f32, f32), p1: (f32, f32), p2: (f32, f32)) -> f32 {
    let mut d = f32::MAX;
    for i in 0..=64 {
        let t = i as f32 / 64.0;
        let u = 1.0 - t;
        let bx = u * u * p0.0 + 2.0 * u * t * p1.0 + t * t * p2.0;
        let by = u * u * p0.1 + 2.0 * u * t * p1.1 + t * t * p2.1;
        d = d.min(((x - bx).powi(2) + (y - by).powi(2)).sqrt());
    }
    d
}

fn rgb(r: u8, g: u8, b: u8) -> C {
    [r as f32 / 255.0, g as f32 / 255.0, b as f32 / 255.0, 1.0]
}

fn mix(a: C, b: C, t: f32) -> C {
    let t = t.clamp(0.0, 1.0);
    [
        a[0] + (b[0] - a[0]) * t,
        a[1] + (b[1] - a[1]) * t,
        a[2] + (b[2] - a[2]) * t,
        a[3] + (b[3] - a[3]) * t,
    ]
}

/// Paint `c` over `dst` with coverage `a`.
fn over(dst: &mut C, c: C, a: f32) {
    let a = (a * c[3]).clamp(0.0, 1.0);
    let out_a = a + dst[3] * (1.0 - a);
    if out_a <= 0.0 {
        return;
    }
    for i in 0..3 {
        dst[i] = (c[i] * a + dst[i] * dst[3] * (1.0 - a)) / out_a;
    }
    dst[3] = out_a;
}

/// Additive light (for glows), only inside the tile.
fn add(dst: &mut C, c: C, a: f32) {
    for i in 0..3 {
        dst[i] = (dst[i] + c[i] * a).min(1.0);
    }
}

fn inside(d: f32) -> f32 {
    if d < 0.0 { 1.0 } else { 0.0 }
}

/// macOS icon grid: 824 px body with a ~185 px corner radius.
const B0: f32 = 100.0;
const B1: f32 = 924.0;
const BR: f32 = 185.0;

fn coin(x: f32, y: f32, cx: f32, cy: f32, r: f32, warm: f32) -> Option<C> {
    let d = ((x - cx).powi(2) + (y - cy).powi(2)).sqrt();
    if d > r {
        return None;
    }
    let ring = (d / (r / 8.3) * std::f32::consts::PI).sin() * 0.5 + 0.5;
    let light =
        1.05 - 0.35 * ((x - cx + y - cy) / (2.0 * r)).clamp(-1.0, 1.0) - 0.12 * (d / r).powi(3);
    let k = (0.76 + 0.24 * ring) * light;
    Some([
        (0.86 + 0.06 * warm) * k,
        (0.45 + 0.04 * warm) * k,
        0.25 * k,
        1.0,
    ])
}

fn shade(v: char, x: f32, y: f32) -> C {
    let mut p: C = [0.0; 4];
    let body = rrect(x, y, B0, B0, B1, B1, BR);
    // Soft drop shadow under the tile.
    let sh = rrect(x, y - 14.0, B0 + 6.0, B0 + 6.0, B1 - 6.0, B1 - 6.0, BR);
    over(
        &mut p,
        [0.0, 0.0, 0.0, 1.0],
        (1.0 - (sh / 28.0).clamp(0.0, 1.0)) * 0.35 * inside(body - 0.0).max(1.0 - inside(body)),
    );
    if body > 0.0 {
        return p;
    }
    let t = (y - B0) / (B1 - B0);
    match v {
        'a' => {
            over(&mut p, mix(rgb(210, 213, 218), rgb(176, 179, 184), t), 1.0);
            // Title bar with traffic lights.
            if y < 230.0 {
                over(&mut p, rgb(168, 171, 176), 1.0);
            }
            for (i, c) in [rgb(230, 92, 80), rgb(232, 186, 64), rgb(96, 186, 88)]
                .iter()
                .enumerate()
            {
                over(
                    &mut p,
                    *c,
                    inside(circle(x, y, 190.0 + i as f32 * 62.0, 165.0, 20.0)),
                );
            }
            // Scan: cream paper in a dark frame, coin on it.
            over(
                &mut p,
                rgb(58, 60, 64),
                inside(rrect(x, y, 224.0, 284.0, 800.0, 696.0, 10.0)),
            );
            over(
                &mut p,
                rgb(246, 232, 204),
                inside(rrect(x, y, 230.0, 290.0, 794.0, 690.0, 6.0)),
            );
            if let Some(c) = coin(x, y, 512.0, 490.0, 150.0, 0.0) {
                over(&mut p, c, 1.0);
            }
            // Patch cord + object box with a yellow inlet bar.
            over(
                &mut p,
                rgb(70, 72, 76),
                inside(rrect(x, y, 296.0, 690.0, 306.0, 760.0, 2.0)),
            );
            let obj = rrect(x, y, 230.0, 760.0, 640.0, 840.0, 12.0);
            over(&mut p, rgb(86, 88, 92), inside(obj));
            over(
                &mut p,
                rgb(232, 196, 52),
                inside(rrect(x, y, 230.0, 760.0, 640.0, 774.0, 6.0).max(obj)),
            );
            for (k, w) in [(0.0, 150.0), (170.0, 90.0)] {
                over(
                    &mut p,
                    rgb(236, 236, 236),
                    0.9 * inside(rrect(x, y, 262.0 + k, 794.0, 262.0 + k + w, 812.0, 9.0)),
                );
            }
        }
        'b' => {
            // Dark slate tile with a faint dot grid (a patcher canvas).
            over(&mut p, mix(rgb(54, 57, 64), rgb(26, 27, 31), t), 1.0);
            let (gx, gy) = ((x - B0) % 48.0 - 24.0, (y - B0) % 48.0 - 24.0);
            over(
                &mut p,
                [1.0, 1.0, 1.0, 1.0],
                0.06 * inside((gx * gx + gy * gy).sqrt() - 2.2),
            );
            // Coin with a soft shadow.
            over(
                &mut p,
                [0.0, 0.0, 0.0, 1.0],
                0.45 * (1.0 - (circle(x, y, 520.0, 438.0, 205.0) / 30.0).clamp(0.0, 1.0)),
            );
            if let Some(c) = coin(x, y, 512.0, 420.0, 205.0, 1.0) {
                over(&mut p, c, 1.0);
            }
            // Scan beam: bright core + cyan glow, lighter where it crosses the coin.
            let beam_y = 470.0;
            let dy = (y - beam_y).abs();
            let on_coin = circle(x, y, 512.0, 420.0, 205.0) < 0.0;
            add(
                &mut p,
                [0.35, 0.9, 1.0, 1.0],
                (-dy / 26.0).exp() * if on_coin { 0.55 } else { 0.35 },
            );
            add(
                &mut p,
                [1.0, 1.0, 1.0, 1.0],
                if dy < 3.0 { 0.85 } else { 0.0 },
            );
            // Patch cord from the coin's outlet down into the object box.
            let cord = bezier(x, y, (380.0, 600.0), (330.0, 720.0), (420.0, 776.0));
            over(&mut p, rgb(240, 200, 60), inside(cord - 9.0));
            over(
                &mut p,
                rgb(240, 200, 60),
                inside(circle(x, y, 380.0, 600.0, 15.0)),
            );
            let obj = rrect(x, y, 300.0, 770.0, 724.0, 850.0, 12.0);
            over(&mut p, rgb(92, 94, 99), inside(obj));
            over(
                &mut p,
                rgb(240, 200, 60),
                inside(rrect(x, y, 300.0, 770.0, 724.0, 784.0, 6.0).max(obj)),
            );
            for (k, w) in [(0.0, 170.0), (190.0, 110.0)] {
                over(
                    &mut p,
                    rgb(232, 232, 232),
                    0.9 * inside(rrect(x, y, 334.0 + k, 803.0, 334.0 + k + w, 821.0, 9.0)),
                );
            }
        }
        _ => {
            // Indigo → magenta diagonal gradient with scanlines.
            let d = ((x - B0) + (y - B0)) / (2.0 * (B1 - B0));
            over(&mut p, mix(rgb(30, 27, 84), rgb(186, 44, 112), d), 1.0);
            if (y as i32 / 6) % 2 == 0 {
                over(&mut p, [0.0, 0.0, 0.0, 1.0], 0.10);
            }
            // Slice shift: horizontal bands displace the disc sideways.
            let band = ((y - 260.0) / 46.0).floor() as i32;
            let shift = match band.rem_euclid(7) {
                1 => 34.0,
                3 => -48.0,
                4 => 18.0,
                6 => -22.0,
                _ => 0.0,
            };
            let (cx, cy, r) = (512.0 + shift, 500.0, 245.0);
            // RGB split: three offset discs, additive.
            for (off, col) in [
                (-22.0, [0.0, 0.85, 1.0, 1.0]),
                (22.0, [1.0, 0.15, 0.6, 1.0]),
                (0.0, [1.0, 0.85, 0.2, 1.0]),
            ] {
                let dd = circle(x, y, cx + off, cy, r);
                add(&mut p, col, 0.55 * inside(dd));
            }
            // Coin rings on top, with a white core.
            if let Some(c) = coin(x, y, cx, cy, r - 30.0, 0.5) {
                over(&mut p, c, 0.92);
            }
            // Pixel blocks breaking off at the edge.
            for (bx, by, s) in [
                (770.0, 340.0, 34.0),
                (812.0, 392.0, 22.0),
                (742.0, 610.0, 26.0),
                (220.0, 470.0, 30.0),
                (196.0, 520.0, 18.0),
            ] {
                over(
                    &mut p,
                    rgb(255, 214, 90),
                    0.9 * inside(rrect(x, y, bx, by, bx + s, by + s, 2.0)),
                );
            }
        }
    }
    // Inner highlight along the top edge of the tile.
    let edge = -body;
    if edge < 2.5 && y < 512.0 {
        over(&mut p, [1.0, 1.0, 1.0, 1.0], 0.18 * (1.0 - edge / 2.5));
    }
    p
}

/// Your own artwork → macOS icon: scale to the 824 px body, round the corners, add the shadow.
fn from_picture(src: &str, out: &str) {
    let art = image::open(src).expect("read picture").to_rgba8();
    let body = (B1 - B0) as u32;
    let art = image::imageops::resize(&art, body, body, image::imageops::FilterType::Lanczos3);
    let n = 1024u32;
    let mut img = RgbaImage::new(n, n);
    for (x, y, px) in img.enumerate_pixels_mut() {
        // 3×3 supersampled coverage of the rounded square and its shadow.
        let (mut cov, mut sh) = (0.0f32, 0.0f32);
        for sy in 0..3 {
            for sx in 0..3 {
                let (fx, fy) = (
                    x as f32 + (sx as f32 + 0.5) / 3.0,
                    y as f32 + (sy as f32 + 0.5) / 3.0,
                );
                cov += inside(rrect(fx, fy, B0, B0, B1, B1, BR));
                let d = rrect(fx, fy - 14.0, B0 + 6.0, B0 + 6.0, B1 - 6.0, B1 - 6.0, BR);
                sh += (1.0 - (d / 28.0).clamp(0.0, 1.0)) * 0.35;
            }
        }
        let (cov, sh) = (cov / 9.0, sh / 9.0);
        let mut c: C = [0.0, 0.0, 0.0, sh];
        if cov > 0.0 {
            let (ax, ay) = (
                (x as f32 - B0).clamp(0.0, body as f32 - 1.0) as u32,
                (y as f32 - B0).clamp(0.0, body as f32 - 1.0) as u32,
            );
            let a = art.get_pixel(ax, ay);
            over(
                &mut c,
                [
                    a[0] as f32 / 255.0,
                    a[1] as f32 / 255.0,
                    a[2] as f32 / 255.0,
                    a[3] as f32 / 255.0,
                ],
                cov,
            );
        }
        let q = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
        *px = Rgba([q(c[0]), q(c[1]), q(c[2]), q(c[3])]);
    }
    img.save(out).expect("write icon");
    println!("{out}");
}

fn main() {
    let mut args = std::env::args().skip(1);
    if std::env::args().nth(1).as_deref() == Some("from") {
        let src = std::env::args().nth(2).expect("picture path");
        let out = std::env::args().nth(3).unwrap_or_else(|| "icon.png".into());
        from_picture(&src, &out);
        return;
    }
    let variant = args.next().and_then(|s| s.chars().next()).unwrap_or('b');
    let out = args.next().unwrap_or_else(|| format!("icon-{variant}.png"));
    let n = 1024u32;
    // 3×3 supersampling for clean edges.
    let rows: Vec<Vec<[u8; 4]>> = (0..n)
        .into_par_iter()
        .map(|py| {
            (0..n)
                .map(|px| {
                    let mut acc = [0.0f32; 4];
                    for sy in 0..3 {
                        for sx in 0..3 {
                            let c = shade(
                                variant,
                                px as f32 + (sx as f32 + 0.5) / 3.0,
                                py as f32 + (sy as f32 + 0.5) / 3.0,
                            );
                            // premultiplied accumulation
                            for i in 0..3 {
                                acc[i] += c[i] * c[3];
                            }
                            acc[3] += c[3];
                        }
                    }
                    let a = acc[3] / 9.0;
                    let px = |v: f32| (v.clamp(0.0, 1.0) * 255.0).round() as u8;
                    if a <= 0.0 {
                        [0, 0, 0, 0]
                    } else {
                        [
                            px(acc[0] / acc[3]),
                            px(acc[1] / acc[3]),
                            px(acc[2] / acc[3]),
                            px(a),
                        ]
                    }
                })
                .collect()
        })
        .collect();
    let mut img = RgbaImage::new(n, n);
    for (y, row) in rows.iter().enumerate() {
        for (x, c) in row.iter().enumerate() {
            img.put_pixel(x as u32, y as u32, Rgba(*c));
        }
    }
    img.save(&out).expect("write icon");
    println!("{out}");
}
