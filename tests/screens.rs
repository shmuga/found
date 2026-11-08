//! Renders screenshots of each view (wgpu, headless) to target/screens/. Run with
//! `cargo test --test screens -- --ignored`.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use found::library::{Library, Sidecar};
use found::{AppConfig, FoundApp};
use std::path::Path;

/// A large synthetic "scan": a bronze coin with relief rings on a warm lid.
fn write_coin(path: &Path, w: u32, h: u32, hue: f32) {
    let (cx, cy, r) = (w as f32 * 0.5, h as f32 * 0.5, h as f32 * 0.36);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        let (dx, dy) = (x as f32 - cx, y as f32 - cy);
        let d = (dx * dx + dy * dy).sqrt();
        if d < r {
            let rings = ((d / r * 40.0).sin() * 0.5 + 0.5) * 0.25;
            let light = 0.55 + 0.35 * (-dx - dy) / (2.0 * r) + rings;
            let l = light.clamp(0.0, 1.0);
            image::Rgb([(190.0 * l + hue) as u8, (130.0 * l) as u8, (70.0 * l) as u8])
        } else {
            let n = ((x * 7 + y * 13) % 17) as u8;
            image::Rgb([246 - n / 4, 232 - n / 4, 204 - n / 4])
        }
    });
    img.save(path).unwrap();
}

fn shot(h: &mut Harness<'static, FoundApp>, name: &str) {
    h.run();
    let img = h.render().expect("render");
    let dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("target/screens");
    std::fs::create_dir_all(&dir).unwrap();
    img.save(dir.join(format!("{name}.png"))).unwrap();
}

fn click(h: &mut Harness<'static, FoundApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    h.get_by_label(label).click();
    h.run();
}

#[test]
#[ignore]
fn screens() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("library");
    let lib = Library::open(&root).unwrap();
    lib.create_collection("Coins").unwrap();
    lib.create_collection("Stamps").unwrap();
    for (i, n) in ["coin-a.jpg", "coin-b.jpg", "coin-c.jpg"]
        .iter()
        .enumerate()
    {
        write_coin(&lib.inbox_dir().join(n), 3000, 2000, i as f32 * 25.0);
    }
    let t = std::time::Instant::now();
    let first = lib.inbox_items().remove(0);
    lib.file_into(
        &first,
        "Coins",
        &Sidecar {
            tags: vec!["bronze".into()],
            ..Default::default()
        },
    )
    .unwrap();
    let second = lib.inbox_items().remove(0);
    lib.file_into(&second, "Coins", &Sidecar::default())
        .unwrap();

    for (i, n) in ["coin-d.jpg", "coin-e.jpg", "coin-f.jpg", "coin-g.jpg"]
        .iter()
        .enumerate()
    {
        write_coin(&lib.inbox_dir().join(n), 1200, 800, 60.0 + i as f32 * 40.0);
    }

    let r2 = root.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1480.0, 940.0))
        .with_pixels_per_point(1.0)
        .with_os(egui::os::OperatingSystem::Mac)
        .wgpu()
        .build_eframe(move |_| {
            FoundApp::new(AppConfig {
                library: Some(r2),
                sync: true,
                ..Default::default()
            })
        });
    shot(&mut h, "1-inbox");
    click(&mut h, "coin-e.jpg");
    h.get_by_label("coin-f.jpg")
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    h.get_by_label("coin-g.jpg").hover();
    h.run();
    shot(&mut h, "1b-select");
    h.key_press(egui::Key::Backspace);
    h.run();
    for c in "gx".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    shot(&mut h, "1c-trash");
    for c in "gi".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    {
        // Mid-drag onto a collection (then dropped back on the grid: nothing moves).
        let from = h.get_by_label("coin-d.jpg").rect().center();
        let to = h.get_by_label("Stamps").rect().center();
        h.hover_at(from);
        h.run();
        h.drag_at(from);
        h.run();
        for k in 1..=6 {
            h.hover_at(from + (to - from) * (k as f32 / 6.0));
            h.run();
        }
        shot(&mut h, "1d-drag");
        h.hover_at(from);
        h.run();
        h.drop_at(from);
        h.run();
    }
    click(&mut h, "coin-c.jpg");
    h.key_press(egui::Key::Enter);
    h.run();
    println!("open+render: {:?}", t.elapsed());
    click(&mut h, "Auto WB (white)");
    shot(&mut h, "2-edit");
    click(&mut h, "3:2");
    shot(&mut h, "3-crop");
    click(&mut h, "Done");
    let idx = h
        .state()
        .presets
        .iter()
        .position(|p| p.name == "Channel Drift")
        .unwrap();
    h.state_mut().editor.as_mut().unwrap().preset_pick = idx;
    click(&mut h, "Apply preset");
    shot(&mut h, "4-glitch");
    click(&mut h, "Patch");
    shot(&mut h, "5-patch");
    click(&mut h, "Presentation");
    for c in "4j++".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    shot(&mut h, "5b-zoom-cursor");
    for c in ":add pix".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    shot(&mut h, "5c-command");
    h.key_press(egui::Key::Escape);
    h.run();
    click(&mut h, "Coins");
    shot(&mut h, "5d-unsaved-prompt");
    h.key_press(egui::Key::N); // don't save
    h.run();
    shot(&mut h, "6-collection");
    for cmd in [":collections", ":layout rows"] {
        for c in cmd.chars() {
            h.event(egui::Event::Text(c.to_string()));
            h.run();
        }
        h.key_press(egui::Key::Enter);
        h.run();
    }
    shot(&mut h, "6c-all-rows");
    for c in ":layout list".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    h.key_press(egui::Key::Enter);
    h.run();
    shot(&mut h, "6d-all-list");
    for c in ":layout grid".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    h.key_press(egui::Key::Enter);
    h.run();
    // "Coins" is also a group heading in this view; the sidebar row comes first.
    h.get_all_by_label("Coins").next().unwrap().click();
    h.run();
    for c in "vl?".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    shot(&mut h, "6b-visual-help");
    for c in "?".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    h.key_press(egui::Key::Escape);
    h.run();

    h.state_mut().new_collage_name = "Coin spread".into();
    h.run();
    click(&mut h, "Create collage");
    click(&mut h, "Template");
    click(&mut h, "2 side by side");
    click(&mut h, "Add Coins/0001");
    click(&mut h, "Add Coins/0002");
    shot(&mut h, "7-collage");
    for c in "gb".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    shot(&mut h, "8-compact-collage");
}

#[test]
#[ignore]
fn screens_settings() {
    let dir = tempfile::tempdir().unwrap();
    let sf = dir.path().join("settings.json");
    let mut s = found::settings::Settings::default();
    for n in ["Coins", "Stamps archive", "Old"] {
        let p = dir.path().join(n);
        std::fs::create_dir_all(&p).unwrap();
        s.touch_recent(&p);
    }
    s.save(&sf).unwrap();
    let sf2 = sf.clone();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1480.0, 940.0))
        .with_pixels_per_point(1.0)
        .wgpu()
        .build_eframe(move |_| {
            FoundApp::new(AppConfig {
                library: None,
                settings_file: Some(sf2),
                sync: true,
            })
        });
    shot(&mut h, "9a-startup");
    let first = dir.path().join("Old").display().to_string();
    click(&mut h, &first);
    h.key_press(egui::Key::Enter);
    h.run();
    click(&mut h, "Settings");
    shot(&mut h, "9b-settings");
}
