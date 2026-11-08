//! Unit-level tests for ops, graph, presets, library and collage compositing.

use found::collage::{self, Collage, Mask, Template};
use found::graph::{self, Evaluator};
use found::img::Img;
use found::library::{ItemRef, Library, Sidecar};
use found::ops::{Aspect, Crop, Op, basic, glitch};
use found::presets;
use std::sync::Arc;

fn gradient(w: usize, h: usize) -> Img {
    let mut img = Img::new(w, h);
    for y in 0..h {
        for x in 0..w {
            img.px[y * w + x] = [x as f32 / w as f32, y as f32 / h as f32, 0.5];
        }
    }
    img
}

#[test]
fn wb_solve_neutralises_a_cast() {
    let cast = [0.92, 0.84, 0.66];
    let (t, n) = basic::wb_solve(cast);
    assert!(t < 0.0, "warm cast needs a cooler temp, got {t}");
    let out = basic::white_balance(Img::filled(1, 1, cast), t, n);
    let p = out.px[0];
    assert!(
        (p[0] - p[1]).abs() < 0.01 && (p[1] - p[2]).abs() < 0.01,
        "not neutral: {p:?}"
    );
}

#[test]
fn crop_aspect_and_rotation() {
    let src = gradient(600, 400);
    let mut c = Crop {
        aspect: Aspect::Square,
        ..Default::default()
    };
    c.apply_aspect(600, 400);
    let out = basic::crop(&src, &c);
    assert_eq!((out.w, out.h), (400, 400));

    let c = Crop {
        rot90: 1,
        ..Default::default()
    };
    let out = basic::crop(&src, &c);
    assert_eq!((out.w, out.h), (400, 600));
    // Clockwise: the top-left of the output is the bottom-left of the input.
    assert_eq!(out.get(0, 0), src.get(0, 399));

    let mut r = src.clone();
    for _ in 0..4 {
        r = basic::rotate90(&r, 1);
    }
    assert_eq!(r, src);

    let mut c = Crop {
        aspect: Aspect::R2x3,
        ..Default::default()
    };
    c.apply_aspect(600, 400);
    let out = basic::crop(&src, &c);
    let ratio = out.w as f32 / out.h as f32;
    assert!((ratio - 2.0 / 3.0).abs() < 0.01, "{ratio}");
}

#[test]
fn clarity_is_identity_at_zero_and_adds_local_contrast() {
    let src = gradient(300, 200);
    assert_eq!(basic::clarity(src.clone(), 0.0), src);
    // Low-contrast texture on mid grey.
    let mut tex = Img::new(200, 200);
    for (i, p) in tex.px.iter_mut().enumerate() {
        let v = 0.5 + 0.05 * ((i % 200 / 10 + i / 2000) % 2) as f32;
        *p = [v; 3];
    }
    let std = |img: &Img| {
        let m: f32 = img.px.iter().map(|p| p[0]).sum::<f32>() / img.px.len() as f32;
        (img.px.iter().map(|p| (p[0] - m).powi(2)).sum::<f32>() / img.px.len() as f32).sqrt()
    };
    let out = basic::clarity(tex.clone(), 80.0);
    assert!(
        std(&out) > std(&tex) * 1.05,
        "{} vs {}",
        std(&out),
        std(&tex)
    );
}

#[test]
fn pixel_sort_permutes_pixels_within_rows() {
    let src = gradient(64, 16);
    let mut noisy = src.clone();
    for (i, p) in noisy.px.iter_mut().enumerate() {
        p[0] = found::img::hash01(3, i as u32, 0);
    }
    let out = glitch::pixel_sort(noisy.clone(), 0.0, 1.0, false, false, 0);
    for y in 0..16 {
        let mut a: Vec<_> = noisy.px[y * 64..(y + 1) * 64]
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect();
        let mut b: Vec<_> = out.px[y * 64..(y + 1) * 64]
            .iter()
            .map(|p| p.map(f32::to_bits))
            .collect();
        a.sort();
        b.sort();
        assert_eq!(a, b);
    }
    assert_ne!(out, noisy);
}

#[test]
fn glitch_ops_are_deterministic() {
    let src = gradient(200, 150);
    for op in Op::catalog() {
        if op.inputs() != 1 {
            continue;
        }
        let a = op.apply(vec![Some(Arc::new(src.clone()))]).unwrap();
        let b = op.apply(vec![Some(Arc::new(src.clone()))]).unwrap();
        assert_eq!(a, b, "{} not deterministic", op.name());
        assert!(
            a.px.iter().all(|p| p.iter().all(|v| v.is_finite())),
            "{} produced NaN",
            op.name()
        );
    }
}

#[test]
fn full_render_matches_cached_preview_eval() {
    let src = gradient(160, 120);
    let p = presets::builtin()
        .into_iter()
        .find(|p| p.name == "Double Exposure")
        .unwrap();
    let mut ev = Evaluator::new(true);
    let a = ev.eval_output(&p.graph, &Arc::new(src.clone()), 1).unwrap();
    let b = graph::render_full(&p.graph, src).unwrap();
    assert_eq!(*a, b);
}

#[test]
fn chain_editing_keeps_graph_valid() {
    let mut g = graph::default_graph();
    let names = |g: &graph::Graph| {
        graph::chain(g)
            .iter()
            .map(|id| g[*id].op.name())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        names(&g),
        [
            "Scan",
            "Crop",
            "White Balance",
            "Exposure",
            "Brightness/Contrast",
            "Clarity",
            "Output"
        ]
    );
    let exp = graph::find(&g, |o| matches!(o, Op::Exposure { .. })).unwrap();
    graph::move_in_chain(&mut g, exp, -1);
    assert_eq!(names(&g)[2], "Exposure");
    let ps = graph::insert_after(&mut g, exp, Op::Invert { mix: 1.0 });
    assert_eq!(names(&g)[3], "Invert");
    graph::remove_bridging(&mut g, ps);
    assert_eq!(names(&g).len(), 7);
    let scan = graph::scan_node(&g).unwrap();
    let clar = graph::find(&g, |o| matches!(o, Op::Clarity { .. })).unwrap();
    assert!(
        graph::would_cycle(&g, clar, scan),
        "clarity → scan must be a cycle"
    );
    // Scan/Output never move.
    graph::move_in_chain(&mut g, scan, 1);
    assert_eq!(names(&g)[0], "Scan");
}

#[test]
fn presets_keep_the_photo_crop() {
    let mut g = graph::default_graph();
    let c = graph::crop_node(&g).unwrap();
    g[c].op = Op::Crop(Crop {
        aspect: Aspect::R3x2,
        rect: [0.1, 0.1, 0.6, 0.5],
        ..Default::default()
    });
    let w = graph::find(&g, |o| matches!(o, Op::WhiteBalance { .. })).unwrap();
    g[w].op = Op::WhiteBalance {
        temp: -20.0,
        tint: 5.0,
    };
    for p in presets::builtin() {
        let out = presets::apply(&g, &p.graph);
        let ow = graph::find(&out, |o| matches!(o, Op::WhiteBalance { .. })).expect("wb kept");
        assert_eq!(
            out[ow].op,
            Op::WhiteBalance {
                temp: -20.0,
                tint: 5.0
            },
            "{}",
            p.name
        );
        let oc = graph::crop_node(&out).expect("crop kept");
        match &out[oc].op {
            Op::Crop(cc) if p.name != "Kaleido" => {
                assert_eq!(cc.rect, [0.1, 0.1, 0.6, 0.5], "{}", p.name)
            }
            Op::Crop(cc) => assert_eq!(cc.aspect, Aspect::Square),
            _ => unreachable!(),
        }
        assert!(graph::chain(&out).len() >= 3, "{} broken", p.name);
    }
}

#[test]
fn library_filing_moves_and_numbers() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path()).unwrap();
    lib.create_collection("Coins").unwrap();
    for n in ["a.jpg", "b.jpg"] {
        found::io::save_jpeg(&gradient(40, 30), &lib.inbox_dir().join(n), 90).unwrap();
    }
    let items = lib.inbox_items();
    assert_eq!(items.len(), 2);
    let sc = Sidecar {
        tags: vec!["x".into()],
        ..Default::default()
    };
    let a = lib.file_into(&items[0], "Coins", &sc).unwrap();
    let b = lib.file_into(&items[1], "Coins", &sc).unwrap();
    assert_eq!(
        a.r,
        ItemRef::Filed {
            collection: "Coins".into(),
            id: "0001".into()
        }
    );
    assert_eq!(
        b.r,
        ItemRef::Filed {
            collection: "Coins".into(),
            id: "0002".into()
        }
    );
    assert!(lib.inbox_items().is_empty());
    assert_eq!(lib.items_with_tag("x").len(), 2);
    assert!(lib.create_collection("_bad").is_err());
    assert!(lib.create_collection("Inbox").is_err());
}

#[test]
fn collage_cells_and_lasso_mask() {
    let mut c = Collage::new("t", collage::CANVAS_PRESETS[1]);
    c.retemplate(Template::TwoH);
    assert_eq!(c.cells().len(), 2);
    c.add_layer("a.jpg".into(), "a".into(), 1.0);
    c.add_layer("b.jpg".into(), "b".into(), 1.0);
    assert_eq!(c.layers[0].cell, Some(0));
    assert_eq!(c.layers[1].cell, Some(1));
    // Layer covers its cell.
    let [_, _, cw, ch] = c.cells()[0];
    assert!(c.layers[0].width >= cw && c.layers[0].height() >= ch);

    // Lasso triangle on a free white layer over black background.
    let mut f = Collage::new("f", collage::CANVAS_PRESETS[0]);
    f.background = [0, 0, 0];
    f.add_layer("w".into(), "w".into(), 1.0);
    f.layers[0].width = 4000.0;
    f.layers[0].cx = 2000.0;
    f.layers[0].cy = 2000.0;
    f.layers[0].mask = Mask::Poly {
        pts: vec![[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]],
    };
    let white = Arc::new(Img::filled(10, 10, [1.0; 3]));
    let out = collage::composite(&f, 0.05, 256, |_| Some(white.clone()));
    assert_eq!((out.w, out.h), (200, 200));
    assert!(out.get(20, 20)[0] > 0.9, "inside the lasso is the layer");
    assert!(
        out.get(180, 180)[0] < 0.1,
        "outside the lasso is background"
    );
}

#[test]
fn settings_roundtrip_and_corrupt_file() {
    use found::settings::{Layout, Settings};
    let dir = tempfile::tempdir().unwrap();
    let f = dir.path().join("s/settings.json");
    assert_eq!(
        Settings::load(&f),
        Settings::default(),
        "missing file → defaults"
    );
    let mut s = Settings::default();
    for i in 0..15 {
        s.touch_recent(&dir.path().join(format!("lib{i}")));
    }
    s.touch_recent(&dir.path().join("lib3"));
    assert_eq!(s.recent_libraries.len(), found::settings::MAX_RECENT);
    assert_eq!(s.last_library().unwrap(), &dir.path().join("lib3"));
    s.set_layout(
        dir.path(),
        Layout {
            inbox: "/abs/in".into(),
            ..Default::default()
        },
    );
    s.save(&f).unwrap();
    assert_eq!(Settings::load(&f), s);
    assert_eq!(
        Layout::resolve(dir.path(), "/abs/in"),
        std::path::PathBuf::from("/abs/in")
    );
    assert_eq!(
        Layout::resolve(dir.path(), "Inbox"),
        dir.path().join("Inbox")
    );

    std::fs::write(&f, "{ not json").unwrap();
    assert_eq!(Settings::load(&f), Settings::default());
    assert!(
        f.with_extension("json.bad").exists(),
        "corrupt file kept aside"
    );
}

/// Build a PDF whose pages each show one image XObject: (filter, bytes, w, h, colorspace).
fn make_pdf(path: &std::path::Path, pages: Vec<(Option<&str>, Vec<u8>, i64, i64, &str)>) {
    use lopdf::{Document, Object, Stream, dictionary};
    let mut doc = Document::with_version("1.5");
    let pages_id = doc.new_object_id();
    let mut kids = vec![];
    for (filter, bytes, w, h, cs) in pages {
        let mut d = dictionary! {
            "Type" => "XObject", "Subtype" => "Image", "Width" => w, "Height" => h,
            "ColorSpace" => cs, "BitsPerComponent" => 8,
        };
        if let Some(f) = filter {
            d.set("Filter", Object::Name(f.as_bytes().to_vec()));
        }
        let img_id = doc.add_object(Stream::new(d, bytes));
        let content = doc.add_object(Stream::new(
            dictionary! {},
            format!("q {w} 0 0 {h} 0 0 cm /Im0 Do Q").into_bytes(),
        ));
        let page = doc.add_object(dictionary! {
            "Type" => "Page", "Parent" => pages_id, "MediaBox" => vec![0.into(), 0.into(), w.into(), h.into()],
            "Contents" => content,
            "Resources" => dictionary! { "XObject" => dictionary! { "Im0" => img_id } },
        });
        kids.push(page.into());
    }
    let n = kids.len() as i64;
    doc.objects.insert(
        pages_id,
        Object::Dictionary(dictionary! { "Type" => "Pages", "Kids" => kids, "Count" => n }),
    );
    let catalog = doc.add_object(dictionary! { "Type" => "Catalog", "Pages" => pages_id });
    doc.trailer.set("Root", catalog);
    doc.save(path).unwrap();
}

#[test]
fn pdf_scans_are_extracted_into_the_inbox() {
    use std::io::Write;
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path().join("lib")).unwrap();
    // Page 1: a real JPEG (DCTDecode) — must come out byte-identical.
    let jpg_path = dir.path().join("p1.jpg");
    found::io::save_jpeg(&gradient(64, 48), &jpg_path, 90).unwrap();
    let jpg = std::fs::read(&jpg_path).unwrap();
    // Page 2: raw RGB, Flate-compressed.
    let raw: Vec<u8> = (0..32 * 16)
        .flat_map(|i| [(i % 256) as u8, 128, 30])
        .collect();
    let mut z = flate2::write::ZlibEncoder::new(vec![], flate2::Compression::default());
    z.write_all(&raw).unwrap();
    let flate = z.finish().unwrap();
    let pdf = lib.inbox_dir().join("scan.pdf");
    make_pdf(
        &pdf,
        vec![
            (Some("DCTDecode"), jpg.clone(), 64, 48, "DeviceRGB"),
            (Some("FlateDecode"), flate, 32, 16, "DeviceRGB"),
        ],
    );

    assert_eq!(lib.inbox_pdfs(), [pdf.clone()]);
    let out = lib.extract_pdf(&pdf).unwrap();
    assert_eq!(out.len(), 2);
    assert_eq!(
        std::fs::read(lib.inbox_dir().join("scan-p01.jpg")).unwrap(),
        jpg,
        "JPEG copied losslessly"
    );
    let png = image::open(lib.inbox_dir().join("scan-p02.png"))
        .unwrap()
        .to_rgb8();
    assert_eq!(png.dimensions(), (32, 16));
    assert_eq!(png.get_pixel(5, 0).0, [5, 128, 30]);
    assert!(!pdf.exists(), "PDF moved out of the Inbox");
    assert!(
        lib.root.join(".found/pdf-originals/scan.pdf").exists(),
        "…and kept"
    );
    let items = lib.inbox_items();
    assert_eq!(items.len(), 2);
    assert_eq!(lib.read_sidecar(&items[0]).source_name, "scan.pdf · page 1");
    assert!(lib.inbox_pdfs().is_empty());
}

#[test]
fn trash_restore_and_purge() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path()).unwrap();
    lib.create_collection("Coins").unwrap();
    let px = image::RgbImage::from_pixel(8, 8, image::Rgb([200, 100, 50]));
    for n in ["a.jpg", "b.jpg"] {
        px.save(lib.inbox_dir().join(n)).unwrap();
    }
    // File b with a sidecar and a fake rendered edit.
    let b = lib
        .inbox_items()
        .into_iter()
        .find(|i| i.title() == "b.jpg")
        .unwrap();
    let sc = Sidecar {
        tags: vec!["x".into()],
        ..Default::default()
    };
    let filed = lib.file_into(&b, "Coins", &sc).unwrap();
    std::fs::write(lib.edit_path(&filed).unwrap(), b"edit").unwrap();
    let filed = lib.item(&filed.r).unwrap();
    let a = lib.inbox_items().remove(0);
    lib.write_sidecar(
        &a,
        &Sidecar {
            tags: vec!["keep".into()],
            ..Default::default()
        },
    )
    .unwrap();
    let a_bytes = std::fs::read(&a.orig).unwrap();

    let ka = lib.trash(&a).unwrap();
    let kb = lib.trash(&filed).unwrap();
    assert!(
        lib.inbox_items().is_empty() && lib.items("Coins").is_empty(),
        "moved out"
    );
    assert!(
        !dir.path().join("Coins/0001.json").exists()
            && !dir.path().join("Coins/0001.edit.jpg").exists()
    );
    let t = lib.trash_items();
    assert_eq!(t.len(), 2);
    assert_eq!(lib.trash_count(), 2);
    assert!(t.iter().all(|i| i.is_trashed()));
    assert!(
        t.iter()
            .any(|i| i.title() == "Coins/0001" && i.edit.is_some())
    );
    assert!(lib.trash(&t[0]).is_err(), "can't trash twice");

    // Restore the inbox scan: same name, same bytes, recipe back.
    let back = lib.restore(&ka).unwrap();
    assert_eq!(back.r, ItemRef::Inbox("a.jpg".into()));
    assert_eq!(std::fs::read(&back.orig).unwrap(), a_bytes);
    assert_eq!(lib.read_sidecar(&back).tags, ["keep"]);

    // Restoring a filed photo whose id was reused meanwhile picks the next id.
    lib.file_into(&back, "Coins", &Sidecar::default()).unwrap(); // takes 0001
    let restored = lib.restore(&kb).unwrap();
    assert_eq!(
        restored.r,
        ItemRef::Filed {
            collection: "Coins".into(),
            id: "0002".into()
        }
    );
    assert!(restored.edit.as_ref().is_some_and(|e| e.exists()));
    assert_eq!(lib.read_sidecar(&restored).tags, ["x"]);
    assert_eq!(lib.trash_count(), 0);

    // Purge / empty really delete.
    let k = lib.trash(&restored).unwrap();
    lib.purge(&k).unwrap();
    assert_eq!(lib.trash_count(), 0);
    assert!(lib.purge("../Coins").is_err());
    let one = lib.items("Coins").remove(0);
    lib.trash(&one).unwrap();
    assert_eq!(lib.empty_trash().unwrap(), 1);
    assert!(lib.items("Coins").is_empty() && lib.trash_items().is_empty());
}

#[test]
fn collages_go_to_the_trash_and_come_back() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path()).unwrap();
    let json = collage::next_path(&lib.collages_dir());
    collage::save(&json, &Collage::new("c", collage::CANVAS_PRESETS[0])).unwrap();
    std::fs::write(json.with_extension("jpg"), b"jpg").unwrap();
    let key = lib.trash_collage(&json).unwrap();
    assert!(collage::list(&lib.collages_dir()).is_empty());
    assert!(!json.with_extension("jpg").exists());
    let t = lib.trash_items();
    assert_eq!(t.len(), 1);
    assert!(t[0].title().starts_with("Collage "));
    lib.restore(&key).unwrap();
    assert_eq!(collage::list(&lib.collages_dir()), [json.clone()]);
    assert_eq!(std::fs::read(json.with_extension("jpg")).unwrap(), b"jpg");
    assert!(collage::load(&json).is_ok());
    assert_eq!(lib.trash_count(), 0);
}

#[test]
fn renaming_a_collection_keeps_collages_and_trash_working() {
    let dir = tempfile::tempdir().unwrap();
    let lib = Library::open(dir.path()).unwrap();
    lib.create_collection("Coins").unwrap();
    lib.create_collection("Stamps").unwrap();
    let px = image::RgbImage::from_pixel(8, 8, image::Rgb([10, 20, 30]));
    for n in ["a.jpg", "b.jpg"] {
        px.save(lib.inbox_dir().join(n)).unwrap();
    }
    for _ in 0..2 {
        let it = lib.inbox_items().remove(0);
        lib.file_into(&it, "Coins", &Sidecar::default()).unwrap();
    }
    // A collage using Coins/0001 and a trashed Coins/0002.
    let cpath = collage::next_path(&lib.collages_dir());
    let mut c = Collage::new("c", collage::CANVAS_PRESETS[0]);
    c.add_layer("Coins/0001.orig.jpg".into(), "Coins/0001".into(), 1.0);
    collage::save(&cpath, &c).unwrap();
    let second = lib.items("Coins").remove(1);
    let key = lib.trash(&second).unwrap();

    assert!(
        lib.rename_collection("Coins", "Stamps").is_err(),
        "name taken"
    );
    assert!(lib.rename_collection("Coins", "_x").is_err(), "reserved");
    assert!(lib.rename_collection("Nope", "X").is_err());
    assert_eq!(
        lib.rename_collection("Coins", " Old coins ").unwrap(),
        "Old coins"
    );
    assert_eq!(lib.collections(), ["Old coins", "Stamps"]);
    assert_eq!(lib.items("Old coins").len(), 1);
    assert_eq!(
        collage::load(&cpath).unwrap().layers[0].source,
        "Old coins/0001.orig.jpg"
    );
    let back = lib.restore(&key).unwrap();
    assert_eq!(
        back.r,
        ItemRef::Filed {
            collection: "Old coins".into(),
            id: "0002".into()
        }
    );
}

#[test]
fn justified_rows_fill_the_width_and_keep_aspect() {
    use found::settings::GridLayout;
    use found::ui::library_ui::layout_rows;
    let aspects = [1.5, 0.75, 1.0, 2.0, 1.5, 1.5, 0.66];
    let rows = layout_rows(GridLayout::Rows, &aspects, 1000.0, 200.0, 10.0);
    assert_eq!(rows.iter().map(|r| r.len()).sum::<usize>(), aspects.len());
    for row in &rows[..rows.len() - 1] {
        let w: f32 = row.iter().map(|(_, s)| s.x).sum::<f32>() + 10.0 * (row.len() - 1) as f32;
        assert!(
            (w - 1000.0).abs() <= row.len() as f32,
            "full rows fill the width: {w}"
        );
        let h = row[0].1.y;
        assert!(
            row.iter().all(|(_, s)| (s.y - h).abs() < 0.5),
            "equal height"
        );
        for (k, s) in row {
            let img_h = s.y - 26.0;
            assert!((s.x / img_h - aspects[*k]).abs() < 0.05, "aspect kept");
        }
    }
    let grid = layout_rows(GridLayout::Grid, &aspects, 1000.0, 200.0, 10.0);
    assert_eq!(grid[0].len(), 4, "(1000+10)/(200+10) → 4 per row");
    let w: f32 = grid[0].iter().map(|(_, s)| s.x).sum::<f32>() + 30.0;
    assert!((w - 1000.0).abs() <= 4.0, "grid rows fill the width: {w}");
    let list = layout_rows(GridLayout::List, &aspects, 1000.0, 200.0, 0.0);
    assert!(list.iter().all(|r| r.len() == 1 && r[0].1.x == 1000.0));
}

#[test]
fn libraries_from_before_the_rename_keep_their_recipes_and_trash() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    std::fs::create_dir_all(root.join("Inbox")).unwrap();
    std::fs::create_dir_all(root.join(".scanner/inbox-edits")).unwrap();
    std::fs::create_dir_all(root.join(".scanner/trash")).unwrap();
    image::RgbImage::from_pixel(4, 4, image::Rgb([9, 9, 9]))
        .save(root.join("Inbox/a.jpg"))
        .unwrap();
    let sc = Sidecar {
        tags: vec!["old".into()],
        ..Default::default()
    };
    std::fs::write(
        root.join(".scanner/inbox-edits/a.jpg.json"),
        serde_json::to_string(&sc).unwrap(),
    )
    .unwrap();
    let lib = Library::open(&root).unwrap();
    assert!(!root.join(".scanner").exists() && root.join(".found/trash").is_dir());
    let it = lib.inbox_items().remove(0);
    assert_eq!(lib.read_sidecar(&it).tags, ["old"]);
}

#[test]
fn split_adds_back_up_and_mask_picks_the_bright_parts() {
    let src = gradient(64, 48);
    for mode in [0, 1] {
        let parts = basic::split(&src, mode);
        assert_eq!(parts.len(), 3);
        for (i, p) in src.px.iter().enumerate() {
            for k in 0..3 {
                let sum: f32 = parts.iter().map(|q| q.px[i][k]).sum();
                assert!(
                    (sum - p[k]).abs() < 1e-4,
                    "mode {mode}: parts sum to the original"
                );
            }
        }
    }
    // RGB mode isolates channels.
    let rgb = basic::split(&src, 0);
    assert!(rgb[0].px.iter().all(|p| p[1] == 0.0 && p[2] == 0.0));

    // Mask: A (white) where B is bright, B where it's dark; no mask input = B's luma.
    let mut b = Img::new(2, 1);
    b.px = vec![[0.05; 3], [0.95; 3]];
    let mut a = Img::new(2, 1);
    a.px = vec![[1.0, 0.0, 0.0]; 2];
    let out = basic::mask(a.clone(), &b, None, 0.5, 0.1, false);
    assert_eq!(out.px[0], b.px[0], "dark → B");
    assert_eq!(out.px[1], [1.0, 0.0, 0.0], "bright → A");
    let inv = basic::mask(a, &b, None, 0.5, 0.1, true);
    assert_eq!(inv.px[0], [1.0, 0.0, 0.0]);
}

#[test]
fn multi_output_and_multi_input_presets_render_the_same_full_res() {
    let src = gradient(160, 120);
    for name in ["Channel Drift", "Glitch Highlights"] {
        let p = presets::builtin()
            .into_iter()
            .find(|p| p.name == name)
            .unwrap();
        // Every Split output that's wired is used through its own index.
        let mut ev = Evaluator::new(true);
        let a = ev
            .eval_output(&p.graph, &Arc::new(src.clone()), 1)
            .expect(name);
        let b = graph::render_full(&p.graph, src.clone()).unwrap();
        assert_eq!(*a, b, "{name}: preview == full render");
        assert_ne!(*a, src, "{name}: does something");
    }
    // Channel Drift with neutral branches is the identity (R+G+B add back up).
    let mut g = presets::builtin()
        .into_iter()
        .find(|p| p.name == "Channel Drift")
        .unwrap()
        .graph;
    let ids: Vec<_> = g.node_ids().map(|(i, _)| i).collect();
    for id in ids {
        if matches!(g[id].op, Op::SliceShift { .. } | Op::Wave { .. }) {
            g[id].bypass = true;
        }
    }
    let out = graph::render_full(&g, src.clone()).unwrap();
    let max_err = out
        .px
        .iter()
        .zip(&src.px)
        .flat_map(|(a, b)| (0..3).map(move |k| (a[k] - b[k]).abs()))
        .fold(0.0, f32::max);
    assert!(max_err < 1e-4, "split + add back = original ({max_err})");
}
