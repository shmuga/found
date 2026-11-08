//! `cargo run --release --example bench [path.jpg]` — timings for proxy decode, preview edits
//! and the full-resolution export on a large scan (generates a 50MP JPEG if none given).

use found::graph::{self, Evaluator};
use found::ops::Op;
use std::sync::Arc;
use std::time::Instant;

fn main() {
    let path = std::env::args()
        .nth(1)
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| {
            let p = std::env::temp_dir().join("found-bench-50mp.jpg");
            if !p.exists() {
                let (w, h) = (8660u32, 5773u32);
                let img = image::RgbImage::from_fn(w, h, |x, y| {
                    let v = ((x / 40 + y / 40) % 2) as u8 * 40;
                    image::Rgb([200 - v, 180 - v, 150 - v / 2])
                });
                img.save(&p).unwrap();
            }
            p
        });
    let t = Instant::now();
    let proxy = Arc::new(found::io::load_scaled(&path, found::io::PROXY_EDGE).unwrap());
    println!("proxy decode {}x{}: {:?}", proxy.w, proxy.h, t.elapsed());

    let mut g = graph::default_graph();
    let mut ev = Evaluator::new(true);
    let t = Instant::now();
    ev.eval_output(&g, &proxy, 1);
    println!("preview, neutral graph: {:?}", t.elapsed());
    let clar = graph::find(&g, |o| matches!(o, Op::Clarity { .. })).unwrap();
    for amt in [20.0, 40.0, 60.0] {
        g[clar].op = Op::Clarity { amount: amt };
        let t = Instant::now();
        ev.eval_output(&g, &proxy, 1);
        println!("preview, clarity slider -> {amt}: {:?}", t.elapsed());
    }
    let wb = graph::find(&g, |o| matches!(o, Op::WhiteBalance { .. })).unwrap();
    g[wb].op = Op::WhiteBalance {
        temp: -20.0,
        tint: 4.0,
    };
    let t = Instant::now();
    ev.eval_output(&g, &proxy, 1);
    println!("preview, WB change (re-runs downstream): {:?}", t.elapsed());
    for p in found::presets::builtin() {
        let t = Instant::now();
        ev.eval_output(&p.graph, &proxy, 1);
        println!("preview, preset {:<16} {:?}", p.name, t.elapsed());
    }

    let t = Instant::now();
    let full = found::io::load_full(&path).unwrap();
    println!("full decode {}x{}: {:?}", full.w, full.h, t.elapsed());
    let t = Instant::now();
    let out = graph::render_full(&g, full).unwrap();
    println!("full render: {:?}", t.elapsed());
    let t = Instant::now();
    found::io::save_jpeg(&out, &std::env::temp_dir().join("found-bench-out.jpg"), 95).unwrap();
    println!("jpeg q95 encode: {:?}", t.elapsed());
}
