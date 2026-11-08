//! End-to-end tests: drive the real app UI headlessly with egui_kittest (AccessKit queries,
//! simulated clicks/keys) against a temporary library folder, then check the files on disk.
//!
//! Text fields are filled through `state_mut()`; everything else goes through the UI.

use egui_kittest::Harness;
use egui_kittest::kittest::Queryable;
use found::library::{ItemRef, Library, Sidecar};
use found::ops::Op;
use found::{AppConfig, FoundApp, graph};
use std::path::{Path, PathBuf};

/// 600×400 "scan": warm off-white lid with a dark grey object in the middle.
fn write_scan(path: &Path) {
    let (w, h) = (600u32, 400u32);
    let img = image::RgbImage::from_fn(w, h, |x, y| {
        if (200..400).contains(&x) && (120..280).contains(&y) {
            image::Rgb([70, 64, 52])
        } else {
            image::Rgb([250, 232, 200])
        }
    });
    img.save(path).unwrap();
}

fn library_with_scans(names: &[&str]) -> (tempfile::TempDir, PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("lib");
    let lib = Library::open(&root).unwrap();
    for n in names {
        write_scan(&lib.inbox_dir().join(n));
    }
    (dir, root)
}

fn harness(root: &Path) -> Harness<'static, FoundApp> {
    let root = root.to_path_buf();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 1000.0))
        .with_max_steps(64)
        .build_eframe(move |_cc| {
            FoundApp::new(AppConfig {
                library: Some(root),
                sync: true,
                ..Default::default()
            })
        });
    h.run();
    h
}

fn click(h: &mut Harness<'static, FoundApp>, label: &str) {
    h.get_by_label(label).scroll_to_me();
    h.run();
    h.get_by_label(label).click();
    h.run();
}

/// Library tiles: a click selects (Finder-style; double-click opens), Enter opens.
fn open(h: &mut Harness<'static, FoundApp>, label: &str) {
    click(h, label);
    h.key_press(egui::Key::Enter);
    h.run();
}

fn wb(app: &FoundApp) -> (f32, f32) {
    let g = &app.editor.as_ref().unwrap().doc.graph;
    let id = graph::find(g, |o| matches!(o, Op::WhiteBalance { .. })).unwrap();
    match g[id].op {
        Op::WhiteBalance { temp, tint } => (temp, tint),
        _ => unreachable!(),
    }
}

/// `:collage` → a new collage with the default `collage-NNNN` name.
fn new_collage(h: &mut Harness<'static, FoundApp>) {
    for c in ":collage".chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
    h.key_press(egui::Key::Enter);
    h.run();
}

fn create_collection(h: &mut Harness<'static, FoundApp>, name: &str) {
    h.state_mut().new_collection = name.into();
    h.run(); // the Create button appears once there's text
    click(h, "Create");
}

#[test]
fn inbox_crop_wb_glitch_tag_and_file_into_collection() {
    let (_tmp, root) = library_with_scans(&["scan1.jpg"]);
    let original = std::fs::read(root.join("Inbox/scan1.jpg")).unwrap();
    let mut h = harness(&root);
    assert_eq!(h.state().inbox_count, 1);

    create_collection(&mut h, "Coins");
    assert!(root.join("Coins").is_dir());

    // Open the scan from the Inbox grid.
    open(&mut h, "scan1.jpg");
    assert!(h.state().editor.is_some());
    assert!(
        h.state().editor.as_ref().unwrap().preview.is_some(),
        "preview rendered"
    );

    // Crop 1:1 (enters crop mode), then leave it.
    click(&mut h, "1:1");
    click(&mut h, "Done");

    // White balance from the white scanner lid → should cool the warm cast.
    click(&mut h, "Auto WB (white)");
    let (temp, _) = wb(h.state());
    assert!(temp < -5.0, "expected cooler temp, got {temp}");

    // Customise the node sequence: + Add node › Experimental › Pixel Sort.
    click(&mut h, "+ Add node");
    h.get_by_label_contains("Experimental").click();
    h.run();
    // Menu items are clicked in place (scrolling the panel underneath would close the menu).
    h.get_by_label("Pixel Sort").click();
    h.run();
    let g = &h.state().editor.as_ref().unwrap().doc.graph;
    let names: Vec<_> = graph::chain(g).iter().map(|id| g[*id].op.name()).collect();
    assert_eq!(
        names[names.len() - 2],
        "Pixel Sort",
        "added before Output: {names:?}"
    );

    // Tag it.
    h.state_mut().editor.as_mut().unwrap().tag_input = "coin, Bronze".into();
    click(&mut h, "Add tag");
    assert_eq!(
        h.state().editor.as_ref().unwrap().doc.tags,
        ["coin", "bronze"]
    );

    // File into the collection.
    click(&mut h, "File into Coins");

    let orig = root.join("Coins/0001.orig.jpg");
    let edit = root.join("Coins/0001.edit.jpg");
    let side = root.join("Coins/0001.json");
    assert!(
        !root.join("Inbox/scan1.jpg").exists(),
        "moved out of the Inbox"
    );
    assert_eq!(
        std::fs::read(&orig).unwrap(),
        original,
        "original kept byte-for-byte"
    );
    assert_eq!(
        image::image_dimensions(&edit).unwrap(),
        (400, 400),
        "1:1 crop at full res"
    );
    let sc: Sidecar = serde_json::from_str(&std::fs::read_to_string(&side).unwrap()).unwrap();
    assert_eq!(sc.tags, ["coin", "bronze"]);
    assert_eq!(sc.source_name, "scan1.jpg");
    assert!(graph::find(&sc.graph, |o| matches!(o, Op::PixelSort { .. })).is_some());
    assert!(
        h.state().tags.contains(&"bronze".to_string()),
        "tag shows up in the sidebar"
    );
}

#[test]
fn reedit_is_non_destructive() {
    let (_tmp, root) = library_with_scans(&["a.jpg"]);
    let lib = Library::open(&root).unwrap();
    lib.create_collection("Stamps").unwrap();
    let item = lib
        .file_into(&lib.inbox_items()[0], "Stamps", &Sidecar::default())
        .unwrap();
    let orig_bytes = std::fs::read(&item.orig).unwrap();

    let mut h = harness(&root);
    click(&mut h, "Stamps");
    open(&mut h, "Stamps/0001");
    assert!(h.state().editor.as_ref().unwrap().stale, "no edit.jpg yet");

    // Apply the built-in Mono Archive look and render.
    let idx = h
        .state()
        .presets
        .iter()
        .position(|p| p.name == "Mono Archive")
        .unwrap();
    h.state_mut().editor.as_mut().unwrap().preset_pick = idx;
    click(&mut h, "Apply preset");
    click(&mut h, "Save & render");

    let edit = root.join("Stamps/0001.edit.jpg");
    let img = image::open(&edit).unwrap().to_rgb8();
    let p = img.get_pixel(10, 10);
    assert!(
        (p[0] as i32 - p[2] as i32).abs() <= 3,
        "mono output, got {p:?}"
    );
    assert_eq!(
        std::fs::read(&item.orig).unwrap(),
        orig_bytes,
        "original untouched"
    );
    assert!(!h.state().editor.as_ref().unwrap().stale);

    // Recipe persisted: reopen in a fresh app and the graph is still mono.
    drop(h);
    let mut h = harness(&root);
    click(&mut h, "Stamps");
    open(&mut h, "Stamps/0001");
    let g = &h.state().editor.as_ref().unwrap().doc.graph;
    assert!(
        graph::find(
            g,
            |o| matches!(o, Op::Saturation { saturation, .. } if *saturation == -100.0)
        )
        .is_some()
    );
}

#[test]
fn glitch_presets_and_user_presets() {
    let (_tmp, root) = library_with_scans(&["g.jpg"]);
    let mut h = harness(&root);
    open(&mut h, "g.jpg");

    // Frame it 3:2 first; presets must keep the photo's crop.
    click(&mut h, "3:2");
    click(&mut h, "Done");
    let crop_before = {
        let g = &h.state().editor.as_ref().unwrap().doc.graph;
        g[graph::crop_node(g).unwrap()].op.clone()
    };

    let idx = h
        .state()
        .presets
        .iter()
        .position(|p| p.name == "VHS Tape")
        .unwrap();
    h.state_mut().editor.as_mut().unwrap().preset_pick = idx;
    click(&mut h, "Apply preset");
    {
        let g = &h.state().editor.as_ref().unwrap().doc.graph;
        assert!(graph::find(g, |o| matches!(o, Op::Vhs { .. })).is_some());
        assert_eq!(g[graph::crop_node(g).unwrap()].op, crop_before);
    }

    // Experimental: dice-roll every glitch node.
    let before = graph::graph_json(&h.state().editor.as_ref().unwrap().doc.graph);
    click(&mut h, "🎲 Mutate");
    assert_ne!(
        graph::graph_json(&h.state().editor.as_ref().unwrap().doc.graph),
        before
    );

    // Save as a user preset; it lands in _presets/ and in the list.
    h.state_mut().editor.as_mut().unwrap().preset_name = "My Tape".into();
    click(&mut h, "Save preset");
    assert!(root.join("_presets/My Tape.json").exists());
    assert!(h.state().presets.iter().any(|p| p.name == "My Tape"));

    // Reset to Neutral, then re-apply the saved preset.
    let neutral = h
        .state()
        .presets
        .iter()
        .position(|p| p.name == "Neutral")
        .unwrap();
    h.state_mut().editor.as_mut().unwrap().preset_pick = neutral;
    click(&mut h, "Apply preset");
    assert!(!graph::has_experimental(
        &h.state().editor.as_ref().unwrap().doc.graph
    ));
    let mine = h
        .state()
        .presets
        .iter()
        .position(|p| p.name == "My Tape")
        .unwrap();
    h.state_mut().editor.as_mut().unwrap().preset_pick = mine;
    click(&mut h, "Apply preset");
    assert!(
        graph::find(&h.state().editor.as_ref().unwrap().doc.graph, |o| matches!(
            o,
            Op::Vhs { .. }
        ))
        .is_some()
    );
}

#[test]
fn undo_redo_and_patch_toggle() {
    let (_tmp, root) = library_with_scans(&["u.jpg"]);
    let mut h = harness(&root);
    open(&mut h, "u.jpg");
    assert_eq!(wb(h.state()), (0.0, 0.0));

    click(&mut h, "Auto WB (white)");
    let after = wb(h.state());
    assert_ne!(after, (0.0, 0.0));

    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Z);
    h.run();
    assert_eq!(wb(h.state()), (0.0, 0.0), "undo");
    h.key_press_modifiers(
        egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
        egui::Key::Z,
    );
    h.run();
    assert_eq!(wb(h.state()), after, "redo");

    // Patch view shows the graph; P toggles back.
    click(&mut h, "Patch");
    assert!(h.state().editor.as_ref().unwrap().patch);
    h.key_press(egui::Key::Tab);
    h.run();
    assert!(
        !h.state().editor.as_ref().unwrap().patch,
        "Tab toggles patch"
    );
}

#[test]
fn collage_split_frame_export() {
    let (_tmp, root) = library_with_scans(&["c1.jpg", "c2.jpg"]);
    let lib = Library::open(&root).unwrap();
    lib.create_collection("Coins").unwrap();
    for it in lib.inbox_items() {
        lib.file_into(&it, "Coins", &Sidecar::default()).unwrap();
    }

    let mut h = harness(&root);
    new_collage(&mut h);
    assert!(h.state().collage.is_some());

    click(&mut h, "Template");
    click(&mut h, "2 side by side");
    click(&mut h, "Add Coins/0001");
    click(&mut h, "Add Coins/0002");
    {
        let c = &h.state().collage.as_ref().unwrap().doc;
        assert_eq!(c.layers.len(), 2);
        assert_eq!(c.layers[0].cell, Some(0));
        assert_eq!(c.layers[1].cell, Some(1));
    }
    click(&mut h, "Export JPEG");

    let jpg = root.join("_collages/collage-0001.jpg");
    assert_eq!(image::image_dimensions(&jpg).unwrap(), (6000, 4000));
    let saved: found::collage::Collage = serde_json::from_str(
        &std::fs::read_to_string(root.join("_collages/collage-0001.json")).unwrap(),
    )
    .unwrap();
    assert_eq!(saved.layers.len(), 2);
    assert!(saved.layers.iter().all(|l| l.source.starts_with("Coins/")));
    let _ = ItemRef::Inbox(String::new());
}

// ---------------------------------------------------------------- settings / startup

fn app_with_settings(settings_file: &Path) -> Harness<'static, FoundApp> {
    let sf = settings_file.to_path_buf();
    let mut h = Harness::builder()
        .with_size(egui::vec2(1500.0, 1000.0))
        .with_max_steps(64)
        .build_eframe(move |_cc| {
            FoundApp::new(AppConfig {
                library: None,
                settings_file: Some(sf),
                sync: true,
            })
        });
    h.run();
    h
}

#[test]
fn startup_picker_recent_libraries_and_auto_open() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = tmp.path().join("cfg/settings.json");
    let root = tmp.path().join("My Scans");

    // First launch: no library, picker shown.
    let mut h = app_with_settings(&settings);
    assert!(h.state().lib.is_none());
    h.state_mut().library_path_input = root.display().to_string();
    click(&mut h, "Open");
    assert!(h.state().lib.is_some());
    assert!(root.join("Inbox").is_dir(), "default layout created");
    let saved = found::settings::Settings::load(&settings);
    assert_eq!(saved.recent_libraries, [root.clone()]);
    drop(h);

    // Second launch: picker again (auto-open is off), library listed under recent.
    let mut h = app_with_settings(&settings);
    assert!(h.state().lib.is_none());
    click(&mut h, &root.display().to_string());
    assert_eq!(h.state().lib.as_ref().unwrap().root, root);

    // Switch library → back to the picker; enable auto-open there.
    click(&mut h, "Switch…");
    assert!(h.state().lib.is_none());
    click(&mut h, "Open the last library automatically on startup");
    assert!(found::settings::Settings::load(&settings).open_last_on_startup);
    drop(h);

    let h = app_with_settings(&settings);
    assert_eq!(
        h.state().lib.as_ref().unwrap().root,
        root,
        "auto-opened last library"
    );
}

#[test]
fn custom_inbox_and_collage_folders_from_settings() {
    let tmp = tempfile::tempdir().unwrap();
    let settings = tmp.path().join("settings.json");
    let root = tmp.path().join("lib");
    let scanner_out = tmp.path().join("ScannerOutput"); // outside the library
    std::fs::create_dir_all(&scanner_out).unwrap();

    let mut h = app_with_settings(&settings);
    h.state_mut().library_path_input = root.display().to_string();
    click(&mut h, "Open");
    create_collection(&mut h, "Coins");

    click(&mut h, "Settings");
    {
        let app = h.state_mut();
        let d = app.layout_draft.as_mut().expect("draft shown");
        d.inbox = scanner_out.display().to_string();
        d.collages = "Collages Out".into();
    }
    click(&mut h, "Apply folders");
    let lib = h.state().lib.clone().unwrap();
    assert_eq!(lib.inbox, scanner_out);
    assert_eq!(lib.collages, root.join("Collages Out"));
    assert!(
        !h.state().collections.contains(&"Collages Out".to_string()),
        "special folders aren't collections"
    );
    let saved = found::settings::Settings::load(&settings);
    assert_eq!(
        saved.layout_for(&root).inbox,
        scanner_out.display().to_string()
    );

    // A scan appearing in the scanner's output folder shows up in the Inbox and files from there.
    write_scan(&scanner_out.join("fresh.jpg"));
    click(&mut h, "Library");
    h.state_mut().refresh();
    h.run();
    assert_eq!(h.state().inbox_count, 1);
    open(&mut h, "fresh.jpg");
    click(&mut h, "File into Coins");
    assert!(!scanner_out.join("fresh.jpg").exists());
    assert!(root.join("Coins/0001.orig.jpg").exists());

    // Collages go to the configured folder.
    new_collage(&mut h);
    click(&mut h, "Add Coins/0001");
    click(&mut h, "Export JPEG");
    assert!(root.join("Collages Out/collage-0001.jpg").exists());

    // Layout persists: a fresh app (auto-open off) → pick from recent → same folders.
    drop(h);
    let mut h = app_with_settings(&settings);
    click(&mut h, &root.display().to_string());
    assert_eq!(h.state().lib.as_ref().unwrap().inbox, scanner_out);
}

// ---------------------------------------------------------------- vim keys

fn typ(h: &mut Harness<'static, FoundApp>, s: &str) {
    for c in s.chars() {
        h.event(egui::Event::Text(c.to_string()));
        h.run();
    }
}

fn key(h: &mut Harness<'static, FoundApp>, k: egui::Key) {
    h.key_press(k);
    h.run();
}

fn set_mtime(p: &Path, secs_ago: u64) {
    let t = std::time::SystemTime::now() - std::time::Duration::from_secs(secs_ago);
    std::fs::File::options()
        .write(true)
        .open(p)
        .unwrap()
        .set_modified(t)
        .unwrap();
}

fn names(app: &FoundApp) -> Vec<String> {
    app.items.iter().map(|i| i.title()).collect()
}

#[test]
fn vim_library_motions_visual_tag_search_and_file() {
    let (_tmp, root) = library_with_scans(&["a.jpg", "b.jpg", "c.jpg", "d.jpg"]);
    // Scan dates: d newest … a oldest; a and b two days ago (different day group).
    for (n, ago) in [
        ("a.jpg", 3 * 86400),
        ("b.jpg", 2 * 86400 + 60),
        ("c.jpg", 120),
        ("d.jpg", 60),
    ] {
        set_mtime(&root.join("Inbox").join(n), ago);
    }
    let mut h = harness(&root);
    assert_eq!(
        names(h.state()),
        ["d.jpg", "c.jpg", "b.jpg", "a.jpg"],
        "newest first"
    );
    assert!(
        h.state().groups.len() >= 2,
        "grouped by day: {:?}",
        h.state().groups
    );

    typ(&mut h, "G");
    assert_eq!(h.state().cursor, 3);
    typ(&mut h, "gg");
    assert_eq!(h.state().cursor, 0);
    typ(&mut h, "l");
    assert_eq!(h.state().cursor, 1);
    typ(&mut h, "0");
    assert_eq!(h.state().cursor, 0);

    // Visual select the first two, tag them, back to normal.
    typ(&mut h, "vl");
    assert_eq!(h.state().keys.mode, found::keys::Mode::Visual);
    assert_eq!(h.state().selection(), [0, 1]);
    typ(&mut h, ":tag fresh");
    key(&mut h, egui::Key::Enter);
    assert_eq!(h.state().keys.mode, found::keys::Mode::Normal);
    let lib = Library::open(&root).unwrap();
    let tagged: Vec<String> = lib
        .inbox_items()
        .iter()
        .filter(|i| lib.read_sidecar(i).tags == ["fresh"])
        .map(|i| i.title())
        .collect();
    assert_eq!(tagged.len(), 2);
    assert!(tagged.contains(&"d.jpg".to_string()) && tagged.contains(&"c.jpg".to_string()));

    // Search jumps the cursor; Esc clears.
    typ(&mut h, "/b");
    key(&mut h, egui::Key::Enter);
    assert_eq!(h.state().items[h.state().cursor].title(), "b.jpg");
    key(&mut h, egui::Key::Escape);
    assert!(h.state().search.is_none());

    // :new + visual :file renders the edits.
    typ(&mut h, ":new Coins");
    key(&mut h, egui::Key::Enter);
    assert!(root.join("Coins").is_dir());
    typ(&mut h, "ggvj");
    let sel = h.state().selection().len();
    assert!(sel >= 2, "j extends the visual range down a row");
    typ(&mut h, ":file co");
    key(&mut h, egui::Key::Tab); // completes to Coins
    assert_eq!(h.state().keys.line, "file Coins");
    key(&mut h, egui::Key::Enter);
    assert!(root.join("Coins/0001.edit.jpg").exists());
    assert!(root.join(format!("Coins/{:04}.edit.jpg", sel)).exists());
    assert_eq!(h.state().inbox_count, 4 - sel);

    // Unknown command → vim-style error, nothing crashes.
    typ(&mut h, ":frobnicate");
    key(&mut h, egui::Key::Enter);
    assert!(h.state().status.starts_with("E492"));
}

#[test]
fn vim_editor_sliders_nodes_crop_zoom() {
    let (_tmp, root) = library_with_scans(&["e.jpg"]);
    let mut h = harness(&root);
    key(&mut h, egui::Key::Enter); // open cursor item
    assert_eq!(h.state().view, found::app::View::Edit);
    let ev = |app: &FoundApp| {
        let g = &app.editor.as_ref().unwrap().doc.graph;
        match g[graph::find(g, |o| matches!(o, Op::Exposure { .. })).unwrap()].op {
            Op::Exposure { ev } => ev,
            _ => unreachable!(),
        }
    };
    // Rows: Crop, Temp, Tint, EV, …
    typ(&mut h, "3j");
    assert_eq!(h.state().cursor_param_label().unwrap(), "Exposure › EV");
    typ(&mut h, "5l");
    assert!((ev(h.state()) - 0.15).abs() < 1e-4, "{}", ev(h.state()));
    typ(&mut h, ".");
    assert!((ev(h.state()) - 0.30).abs() < 1e-4, "dot repeats");
    typ(&mut h, "x");
    assert_eq!(ev(h.state()), 0.0, "x resets");
    typ(&mut h, "i-0.7");
    key(&mut h, egui::Key::Enter);
    assert!((ev(h.state()) + 0.7).abs() < 1e-4, "insert a value");
    typ(&mut h, "u");
    assert_eq!(ev(h.state()), 0.0, "undo");

    // o → :add with completion, then dd / p / J.
    typ(&mut h, "o");
    typ(&mut h, "pixel");
    key(&mut h, egui::Key::Tab);
    assert_eq!(
        h.state().keys.line,
        "add pixel",
        "ambiguous (Pixel Sort / Pixelate): no change, like vim"
    );
    assert_eq!(h.state().completions.len(), 2);
    typ(&mut h, " s");
    key(&mut h, egui::Key::Tab);
    assert_eq!(h.state().keys.line, "add Pixel Sort");
    key(&mut h, egui::Key::Enter);
    let chain = |app: &FoundApp| {
        let g = &app.editor.as_ref().unwrap().doc.graph;
        graph::chain(g)
            .iter()
            .map(|id| g[*id].op.name())
            .collect::<Vec<_>>()
    };
    assert_eq!(
        chain(h.state())[4],
        "Pixel Sort",
        "added after Exposure: {:?}",
        chain(h.state())
    );
    typ(&mut h, "J");
    assert_eq!(chain(h.state())[5], "Pixel Sort", "J moves it down");
    typ(&mut h, "dd");
    assert!(!chain(h.state()).contains(&"Pixel Sort"));
    typ(&mut h, "P");
    assert!(
        chain(h.state()).contains(&"Pixel Sort"),
        "dd yanked it, P pastes"
    );

    // Crop tool: c, s (square), c done.
    typ(&mut h, "c");
    assert_eq!(h.state().key_ctx(), found::keys::Ctx::Crop);
    typ(&mut h, "s");
    typ(&mut h, "c");
    let g = &h.state().editor.as_ref().unwrap().doc.graph;
    assert!(
        matches!(&g[graph::crop_node(g).unwrap()].op, Op::Crop(c) if c.aspect == found::ops::Aspect::Square)
    );

    // Zoom keys.
    typ(&mut h, "+");
    assert!(h.state().editor.as_ref().unwrap().zoom.zoom > 1.0);
    typ(&mut h, "=");
    assert_eq!(h.state().editor.as_ref().unwrap().zoom.zoom, 1.0);

    // :w saves the recipe for an Inbox scan; q goes back.
    typ(&mut h, ":w");
    key(&mut h, egui::Key::Enter);
    assert!(root.join(".found/inbox-edits/e.jpg.json").exists());
    typ(&mut h, "q");
    assert_eq!(h.state().view, found::app::View::Library);
}

#[test]
fn yank_recipe_and_paste_onto_other_photos() {
    let (_tmp, root) = library_with_scans(&["one.jpg", "two.jpg", "three.jpg"]);
    let mut h = harness(&root);
    typ(&mut h, ":sort name asc");
    key(&mut h, egui::Key::Enter);
    assert_eq!(names(h.state()), ["one.jpg", "three.jpg", "two.jpg"]);
    typ(&mut h, "gg"); // the cursor stays on the same photo across a re-sort
    key(&mut h, egui::Key::Enter); // open one.jpg
    typ(&mut h, ":preset mono");
    key(&mut h, egui::Key::Enter);
    typ(&mut h, "Y");
    assert!(h.state().yank_recipe.is_some());
    typ(&mut h, ":wq"); // edits aren't autosaved: write, then back to the library
    key(&mut h, egui::Key::Enter);
    typ(&mut h, "gglvl"); // select the other two
    assert_eq!(h.state().selection(), [1, 2]);
    typ(&mut h, "p");
    let lib = Library::open(&root).unwrap();
    for it in lib.inbox_items() {
        let mono = graph::find(
            &lib.read_sidecar(&it).graph,
            |o| matches!(o, Op::Saturation { saturation, .. } if *saturation == -100.0),
        )
        .is_some();
        assert!(mono, "{} got the recipe", it.title());
    }
}

#[test]
fn sidebar_compact_and_ui_sort_controls() {
    let (_tmp, root) = library_with_scans(&["x.jpg"]);
    let mut h = harness(&root);
    typ(&mut h, "gb");
    assert!(h.state().settings.sidebar_compact);
    // Compact rail still navigates (tile labelled with the full name).
    h.get_all_by_label("Inbox").next().unwrap().click();
    h.run();
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Backslash);
    h.run();
    assert!(!h.state().settings.sidebar_compact);
    typ(&mut h, ":group none");
    key(&mut h, egui::Key::Enter);
    assert_eq!(h.state().settings.group_by, found::settings::GroupBy::None);
}

#[test]
fn pdf_in_inbox_becomes_scans() {
    let (_tmp, root) = library_with_scans(&[]);
    // Minimal single-page PDF with an embedded JPEG.
    let jpg = root.join("tmp.jpg");
    write_scan(&jpg);
    let bytes = std::fs::read(&jpg).unwrap();
    std::fs::remove_file(&jpg).unwrap();
    let pdf = root.join("Inbox/scanner-output.pdf");
    {
        use lopdf::{Document, Object, Stream, dictionary};
        let mut doc = Document::with_version("1.5");
        let pages = doc.new_object_id();
        let img = doc.add_object(Stream::new(
            dictionary! {"Type" => "XObject", "Subtype" => "Image", "Width" => 600, "Height" => 400, "ColorSpace" => "DeviceRGB", "BitsPerComponent" => 8, "Filter" => "DCTDecode"},
            bytes.clone(),
        ));
        let content = doc.add_object(Stream::new(
            dictionary! {},
            b"q 600 0 0 400 0 0 cm /Im0 Do Q".to_vec(),
        ));
        let page = doc.add_object(
            dictionary! {"Type" => "Page", "Parent" => pages, "Contents" => content,
            "Resources" => dictionary! {"XObject" => dictionary! {"Im0" => img}}},
        );
        doc.objects.insert(
            pages,
            Object::Dictionary(
                dictionary! {"Type" => "Pages", "Kids" => vec![page.into()], "Count" => 1},
            ),
        );
        let cat = doc.add_object(dictionary! {"Type" => "Catalog", "Pages" => pages});
        doc.trailer.set("Root", cat);
        doc.save(&pdf).unwrap();
    }
    let h = harness(&root);
    assert_eq!(h.state().inbox_count, 1);
    assert_eq!(h.state().items[0].title(), "scanner-output.jpg");
    assert_eq!(
        std::fs::read(root.join("Inbox/scanner-output.jpg")).unwrap(),
        bytes
    );
    assert!(
        root.join(".found/pdf-originals/scanner-output.pdf")
            .exists()
    );
}

#[test]
fn mouse_select_trash_undo_restore_and_delete_forever() {
    let (_tmp, root) = library_with_scans(&["t1.jpg", "t2.jpg", "t3.jpg", "t4.jpg"]);
    let mut h = harness(&root);
    // Name order so positions are predictable.
    h.state_mut()
        .run_command(&egui::Context::default(), "sort name asc");
    h.run();
    let inbox = root.join("Inbox");

    // Click selects one; ⌘-click adds (the first stays selected, like Finder).
    click(&mut h, "t1.jpg");
    assert!(h.state().editor.is_none(), "single click only selects");
    h.get_by_label("t3.jpg")
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    assert_eq!(h.state().selection().len(), 2);

    // ⌫ moves both to the trash; u brings them back.
    h.key_press(egui::Key::Backspace);
    h.run();
    assert!(!inbox.join("t1.jpg").exists() && !inbox.join("t3.jpg").exists());
    assert!(inbox.join("t2.jpg").exists());
    assert_eq!(h.state().trash_count, 2);
    typ(&mut h, "u");
    assert!(inbox.join("t1.jpg").exists() && inbox.join("t3.jpg").exists());
    assert_eq!(h.state().trash_count, 0);

    // Rubber band from empty space across the first row selects every tile it touches.
    let r2 = h.get_by_label("t2.jpg").rect();
    let r3 = h.get_by_label("t3.jpg").rect();
    let start = egui::pos2(r2.min.x - 6.0, r2.max.y + 40.0); // below the row, left of t2
    let end = egui::pos2(r3.center().x, r3.center().y);
    h.hover_at(start);
    h.run();
    h.drag_at(start);
    h.run();
    for k in 1..=4 {
        h.hover_at(start + (end - start) * (k as f32 / 4.0));
        h.run();
    }
    let sel: Vec<String> = h
        .state()
        .selection()
        .iter()
        .map(|i| h.state().items[*i].title())
        .collect();
    h.drop_at(end);
    h.run();
    assert_eq!(sel, ["t2.jpg", "t3.jpg"], "band selection");
    assert!(h.state().marquee.is_none());

    // Right-click → Move to trash.
    h.get_by_label("t2.jpg").click_secondary();
    h.run();
    // Header button and context-menu entry share the label; the menu is the later one.
    h.get_all_by_label("Move 2 to trash")
        .last()
        .unwrap()
        .click();
    h.run();
    assert_eq!(h.state().trash_count, 2);
    assert_eq!(h.state().items.len(), 2);

    // Trash view: restore one with r, delete the other forever (asks first).
    typ(&mut h, "gx");
    assert_eq!(h.state().source, found::app::Source::Trash);
    assert_eq!(h.state().items.len(), 2);
    let first = h.state().items[0].title();
    open(&mut h, &first); // Enter on a trashed photo does not open it
    assert!(h.state().editor.is_none());
    typ(&mut h, "r");
    assert!(inbox.join(&first).exists(), "restored {first}");
    assert_eq!(h.state().items.len(), 1);
    h.key_press(egui::Key::Backspace);
    h.run();
    assert!(h.state().confirm.is_some(), "permanent delete asks first");
    click(&mut h, "Delete forever");
    assert_eq!(h.state().trash_count, 0);
    assert_eq!(
        std::fs::read_dir(&inbox).unwrap().count(),
        3,
        "one photo deleted for good"
    );
}

#[test]
fn trash_from_the_editor_opens_the_next_photo() {
    let (_tmp, root) = library_with_scans(&["e1.jpg", "e2.jpg"]);
    let mut h = harness(&root);
    h.state_mut()
        .run_command(&egui::Context::default(), "sort name asc");
    h.run();
    open(&mut h, "e1.jpg");
    h.key_press_modifiers(egui::Modifiers::COMMAND, egui::Key::Backspace);
    h.run();
    assert!(!root.join("Inbox/e1.jpg").exists());
    assert_eq!(
        h.state().editor.as_ref().map(|e| e.item.title()),
        Some("e2.jpg".into())
    );
}

#[test]
fn collage_can_be_trashed_and_restored() {
    let (_tmp, root) = library_with_scans(&[]);
    let mut h = harness(&root);
    new_collage(&mut h);
    let path = h.state().collage.as_ref().unwrap().path.clone();
    assert!(path.exists());
    click(&mut h, "Move to trash");
    assert!(!path.exists(), "collage moved out");
    assert!(h.state().collage.is_none());
    assert!(h.state().collage_files.is_empty());
    assert_eq!(h.state().trash_count, 1);
    // Leaving the view must not autosave it back; u in the library restores it.
    h.run_steps(4);
    assert!(!path.exists());
    typ(&mut h, "u");
    assert!(path.exists());
    assert_eq!(h.state().collage_files, [path]);
}

fn sidecar_has_mono(path: &Path) -> bool {
    let sc: Sidecar = std::fs::read_to_string(path)
        .ok()
        .and_then(|s| serde_json::from_str(&s).ok())
        .unwrap_or_default();
    graph::find(
        &sc.graph,
        |o| matches!(o, Op::Saturation { saturation, .. } if *saturation == -100.0),
    )
    .is_some()
}

#[test]
fn edits_are_not_autosaved_and_leaving_asks() {
    let (_tmp, root) = library_with_scans(&["k.jpg", "l.jpg"]);
    let side = root.join(".found/inbox-edits/k.jpg.json");
    let mut h = harness(&root);
    typ(&mut h, ":sort name asc");
    key(&mut h, egui::Key::Enter);
    open(&mut h, "k.jpg");
    typ(&mut h, ":preset mono");
    key(&mut h, egui::Key::Enter);
    h.run_steps(4);
    assert!(h.state().editor_dirty());
    assert!(!sidecar_has_mono(&side), "nothing written yet");

    // q → prompt; Cancel stays in the editor with the edits.
    typ(&mut h, "q");
    assert!(h.state().leave.is_some(), "asks before leaving");
    assert_eq!(h.state().view, found::app::View::Edit);
    click(&mut h, "Cancel");
    assert!(h.state().leave.is_none() && h.state().editor_dirty());

    // ] (next photo) → Don't save: edits dropped, l.jpg opens.
    typ(&mut h, "]");
    assert!(h.state().leave.is_some());
    click(&mut h, "Don't save");
    assert_eq!(h.state().editor.as_ref().unwrap().item.title(), "l.jpg");
    assert!(!sidecar_has_mono(&side));

    // Library button → Save (Enter): written, then we're in the library.
    typ(&mut h, ":preset mono");
    key(&mut h, egui::Key::Enter);
    h.get_by_label("Library").click();
    h.run();
    assert!(h.state().leave.is_some());
    key(&mut h, egui::Key::Enter);
    assert_eq!(h.state().view, found::app::View::Library);
    assert!(sidecar_has_mono(
        &root.join(".found/inbox-edits/l.jpg.json")
    ));

    // :q! leaves without asking and without saving.
    open(&mut h, "k.jpg");
    typ(&mut h, ":preset mono");
    key(&mut h, egui::Key::Enter);
    typ(&mut h, ":q!");
    key(&mut h, egui::Key::Enter);
    assert!(h.state().leave.is_none());
    assert_eq!(h.state().view, found::app::View::Library);
    assert!(!sidecar_has_mono(&side));
}

#[test]
fn save_copy_into_a_collection() {
    let (_tmp, root) = library_with_scans(&["c.jpg"]);
    let mut h = harness(&root);
    create_collection(&mut h, "Coins");
    open(&mut h, "c.jpg");
    typ(&mut h, ":preset mono");
    key(&mut h, egui::Key::Enter);
    click(&mut h, "Save copy to Coins");
    assert!(
        root.join("Inbox/c.jpg").exists(),
        "the scan stays in the Inbox"
    );
    assert!(
        sidecar_has_mono(&root.join("Coins/0001.json")),
        "copy has the edits"
    );
    assert_eq!(
        image::image_dimensions(root.join("Coins/0001.edit.jpg")).unwrap(),
        (600, 400)
    );
    // A second copy (via :saveas) gets the next number; the open photo is still unsaved.
    typ(&mut h, ":saveas coins");
    key(&mut h, egui::Key::Enter);
    assert!(root.join("Coins/0002.orig.jpg").exists());
    assert!(h.state().editor_dirty());
    assert_eq!(h.state().editor.as_ref().unwrap().item.title(), "c.jpg");
}

#[test]
fn collage_edits_ask_before_leaving() {
    let (_tmp, root) = library_with_scans(&[]);
    let mut h = harness(&root);
    new_collage(&mut h);
    let path = h.state().collage.as_ref().unwrap().path.clone();
    let before = std::fs::read_to_string(&path).unwrap();
    click(&mut h, "Template");
    click(&mut h, "2 side by side");
    assert!(h.state().collage_dirty());
    assert_eq!(
        std::fs::read_to_string(&path).unwrap(),
        before,
        "not autosaved"
    );
    typ(&mut h, "gl");
    assert!(h.state().leave.is_some());
    // The dialog's Save (the toolbar has one too, behind the modal): the last match.
    h.get_all_by_label("Save").last().unwrap().click();
    h.run();
    assert_eq!(h.state().view, found::app::View::Library);
    assert_ne!(
        std::fs::read_to_string(&path).unwrap(),
        before,
        "saved on request"
    );
}

#[test]
fn trash_button_in_editor_and_rename_collection() {
    let (_tmp, root) = library_with_scans(&["m1.jpg", "m2.jpg"]);
    let mut h = harness(&root);
    typ(&mut h, ":sort name asc");
    key(&mut h, egui::Key::Enter);
    open(&mut h, "m1.jpg");
    click(&mut h, "Move to trash");
    assert!(!root.join("Inbox/m1.jpg").exists());
    assert_eq!(
        h.state().editor.as_ref().unwrap().item.title(),
        "m2.jpg",
        "next photo opens"
    );

    // File m2 into Coins, then rename Coins from the sidebar (right-click › Rename…).
    create_collection(&mut h, "Coins");
    typ(&mut h, ":file Coins");
    key(&mut h, egui::Key::Enter);
    h.get_by_label("Coins").click_secondary();
    h.run();
    click(&mut h, "Rename…");
    assert!(h.state().renaming.is_some());
    h.state_mut().renaming.as_mut().unwrap().1 = "Old coins".into();
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(root.join("Old coins/0001.orig.jpg").exists());
    assert!(!root.join("Coins").exists());
    assert_eq!(h.state().collections, ["Old coins"]);

    // The open photo follows the rename; :rename works on the current collection too.
    click(&mut h, "Old coins");
    assert_eq!(h.state().items[0].title(), "Old coins/0001");
    typ(&mut h, ":rename Coins");
    key(&mut h, egui::Key::Enter);
    assert_eq!(h.state().collections, ["Coins"]);
    assert_eq!(
        h.state().source,
        found::app::Source::Collection("Coins".into())
    );
    open(&mut h, "Coins/0001");
    assert!(
        h.state().editor.as_ref().unwrap().proxy.is_some()
            || h.state().editor.as_ref().unwrap().load_error.is_none()
    );
}

fn drag(h: &mut Harness<'static, FoundApp>, from: egui::Pos2, to: egui::Pos2) {
    h.hover_at(from);
    h.run();
    h.drag_at(from);
    h.run();
    for k in 1..=6 {
        h.hover_at(from + (to - from) * (k as f32 / 6.0));
        h.run();
    }
    h.drop_at(to);
    h.run();
}

#[test]
fn drag_photos_between_collections_and_to_trash() {
    let (_tmp, root) = library_with_scans(&["d1.jpg", "d2.jpg", "d3.jpg"]);
    let lib = Library::open(&root).unwrap();
    lib.create_collection("Coins").unwrap();
    lib.create_collection("Stamps").unwrap();
    let mut h = harness(&root);
    typ(&mut h, ":sort name asc");
    key(&mut h, egui::Key::Enter);

    // Select d1 + d2, drag d1 onto Coins: both are filed (and rendered).
    click(&mut h, "d1.jpg");
    h.get_by_label("d2.jpg")
        .click_modifiers(egui::Modifiers::COMMAND);
    h.run();
    let from = h.get_by_label("d1.jpg").rect().center();
    let to = h.get_by_label("Coins").rect().center();
    drag(&mut h, from, to);
    assert!(h.state().dragging.is_none());
    assert!(root.join("Coins/0001.orig.jpg").exists() && root.join("Coins/0002.orig.jpg").exists());
    assert!(
        root.join("Coins/0001.edit.jpg").exists(),
        "filed scans get rendered"
    );
    assert!(root.join("Inbox/d3.jpg").exists(), "unselected scan stays");

    // A collage using Coins/0002 keeps working after the photo moves.
    let cpath = found::collage::next_path(&lib.collages_dir());
    let mut c = found::collage::Collage::new("c", found::collage::CANVAS_PRESETS[0]);
    c.add_layer("Coins/0002.edit.jpg".into(), "Coins/0002".into(), 1.5);
    found::collage::save(&cpath, &c).unwrap();

    // In Coins, drag the (unselected) second photo onto Stamps: only that one moves.
    click(&mut h, "Coins");
    let from = h.get_by_label("Coins/0002").rect().center();
    let to = h.get_by_label("Stamps").rect().center();
    drag(&mut h, from, to);
    assert!(
        root.join("Stamps/0001.orig.jpg").exists() && root.join("Stamps/0001.edit.jpg").exists()
    );
    assert!(!root.join("Coins/0002.orig.jpg").exists());
    assert!(root.join("Coins/0001.orig.jpg").exists());
    assert_eq!(
        found::collage::load(&cpath).unwrap().layers[0].source,
        "Stamps/0001.edit.jpg"
    );

    // Dropping on Trash removes it (restorable).
    let from = h.get_by_label("Coins/0001").rect().center();
    let to = h.get_all_by_label("Trash").next().unwrap().rect().center();
    drag(&mut h, from, to);
    assert!(!root.join("Coins/0001.orig.jpg").exists());
    assert_eq!(h.state().trash_count, 1);
}

#[test]
fn all_collections_view_layouts_and_size() {
    use found::settings::{GridLayout, GroupBy};
    let (_tmp, root) = library_with_scans(&["a1.jpg", "a2.jpg", "a3.jpg"]);
    let lib = Library::open(&root).unwrap();
    lib.create_collection("Coins").unwrap();
    lib.create_collection("Stamps").unwrap();
    let mut inbox = lib.inbox_items();
    inbox.sort_by(|a, b| a.orig.cmp(&b.orig));
    lib.file_into(&inbox[0], "Stamps", &Sidecar::default())
        .unwrap();
    lib.file_into(&inbox[1], "Coins", &Sidecar::default())
        .unwrap();
    lib.file_into(&inbox[2], "Coins", &Sidecar::default())
        .unwrap();
    let mut h = harness(&root);

    // Clicking the "Collections" heading shows every filed photo grouped by collection.
    click(&mut h, "Collections");
    assert_eq!(h.state().source, found::app::Source::AllCollections);
    let groups: Vec<(String, usize)> = h
        .state()
        .groups
        .iter()
        .map(|(l, _, n)| (l.clone(), *n))
        .collect();
    assert_eq!(
        groups,
        [("Coins".to_string(), 2), ("Stamps".to_string(), 1)]
    );
    // Grouping here is separate from the Inbox's (which stays by day).
    typ(&mut h, ":group month");
    key(&mut h, egui::Key::Enter);
    assert_eq!(h.state().settings.group_all, GroupBy::Month);
    assert_eq!(h.state().settings.group_by, GroupBy::Day);
    typ(&mut h, ":group collection");
    key(&mut h, egui::Key::Enter);

    // Layouts: Rows keeps each photo's 3:2 shape, List is one full-width line per photo.
    let tile = |h: &Harness<'static, FoundApp>| h.get_by_label("Coins/0001").rect();
    let grid = tile(&h);
    // Tiles are at least the chosen size, widened so each row fills the grid.
    let ts = h.state().settings.tile_size;
    assert!(
        grid.width() >= ts - 1.0 && grid.width() < ts * 2.0,
        "{grid:?}"
    );
    click(&mut h, "Rows");
    h.run(); // thumbnails (and their aspect ratios) arrive
    assert_eq!(h.state().settings.grid_layout, GridLayout::Rows);
    let r = tile(&h);
    assert!(
        r.width() > grid.width() * 1.3,
        "3:2 photo is wider than tall: {r:?}"
    );
    click(&mut h, "List");
    let l = tile(&h);
    assert!(l.width() > 800.0 && l.height() < 80.0, "list row: {l:?}");

    // Size: + grows thumbnails and is remembered in settings.
    click(&mut h, "Grid");
    let before = h.state().settings.tile_size;
    typ(&mut h, "+");
    assert!(h.state().settings.tile_size > before);
    assert!(tile(&h).width() > grid.width());
    // Opening a photo from this view works as usual.
    open(&mut h, "Stamps/0001");
    assert_eq!(
        h.state().editor.as_ref().unwrap().item.title(),
        "Stamps/0001"
    );
}

#[test]
fn apply_last_reuses_the_last_preset_or_saved_edits() {
    let (_tmp, root) = library_with_scans(&["p1.jpg", "p2.jpg", "p3.jpg"]);
    let mut h = harness(&root);
    typ(&mut h, ":sort name asc");
    key(&mut h, egui::Key::Enter);
    let mono = |h: &Harness<'static, FoundApp>| {
        let g = &h.state().editor.as_ref().unwrap().doc.graph;
        graph::find(
            g,
            |o| matches!(o, Op::Saturation { saturation, .. } if *saturation == -100.0),
        )
        .is_some()
    };
    open(&mut h, "p1.jpg");
    assert!(h.state().settings.last_recipe.is_none());
    typ(&mut h, ":preset mono");
    key(&mut h, egui::Key::Enter);
    assert_eq!(
        h.state().settings.last_recipe.as_ref().unwrap().name,
        "Mono Archive"
    );
    typ(&mut h, ":w");
    key(&mut h, egui::Key::Enter);
    assert_eq!(
        h.state().settings.last_recipe.as_ref().unwrap().name,
        "edits of p1.jpg"
    );

    // Next photo: one click applies it.
    typ(&mut h, "]");
    assert_eq!(h.state().editor.as_ref().unwrap().item.title(), "p2.jpg");
    assert!(!mono(&h));
    click(&mut h, "Apply last");
    assert!(mono(&h));
    assert!(h.state().editor_dirty(), "applied, not saved");
    // Vim key A does the same.
    typ(&mut h, ":q!");
    key(&mut h, egui::Key::Enter);
    open(&mut h, "p3.jpg");
    typ(&mut h, "A");
    assert!(mono(&h));
}

#[test]
fn collage_naming_rename_and_layer_stacking() {
    let (_tmp, root) = library_with_scans(&["s1.jpg", "s2.jpg", "s3.jpg"]);
    let lib = Library::open(&root).unwrap();
    lib.create_collection("Coins").unwrap();
    for it in lib.inbox_items() {
        lib.file_into(&it, "Coins", &Sidecar::default()).unwrap();
    }
    let mut h = harness(&root);
    // Sidebar: type a name, a "Create collage" button appears.
    assert!(
        h.query_by_label("Create collage").is_none(),
        "button hidden until you type"
    );
    h.state_mut().new_collage_name = "Trip spread".into();
    h.run();
    click(&mut h, "Create collage");
    let path = root.join("_collages/Trip spread.json");
    assert_eq!(h.state().collage.as_ref().unwrap().path, path);
    for n in ["Add Coins/0001", "Add Coins/0002", "Add Coins/0003"] {
        click(&mut h, n);
    }
    let order = |h: &Harness<'static, FoundApp>| -> Vec<String> {
        h.state()
            .collage
            .as_ref()
            .unwrap()
            .doc
            .layers
            .iter()
            .map(|l| l.name.clone())
            .collect()
    };
    assert_eq!(order(&h), ["Coins/0001", "Coins/0002", "Coins/0003"]);

    // Right-click a layer in the Layers panel → Send to back.
    h.get_by_label("Coins/0003").click_secondary();
    h.run();
    click(&mut h, "Send to back");
    assert_eq!(order(&h), ["Coins/0003", "Coins/0001", "Coins/0002"]);
    h.get_by_label("Coins/0003").click_secondary();
    h.run();
    click(&mut h, "Bring forward");
    assert_eq!(order(&h), ["Coins/0001", "Coins/0003", "Coins/0002"]);

    // Drag the bottom layer (last row) above the top row with its grip.
    let rows: Vec<egui::Rect> = ["Coins/0002", "Coins/0003", "Coins/0001"]
        .iter()
        .map(|n| h.get_by_label(n).rect())
        .collect();
    let grip = egui::pos2(rows[2].min.x - 40.0, rows[2].center().y);
    let to = egui::pos2(grip.x, rows[0].top() + 2.0);
    let from_id = h.state().collage.as_ref().unwrap().doc.layers[0].id;
    h.hover_at(grip);
    h.run();
    h.drag_at(grip);
    h.run();
    for k in 1..=5 {
        h.hover_at(grip + (to - grip) * (k as f32 / 5.0));
        h.run();
    }
    h.drop_at(to);
    h.run();
    let c = h.state().collage.as_ref().unwrap();
    assert_eq!(
        c.doc.layers.last().unwrap().id,
        from_id,
        "dragged to the front: {:?}",
        order(&h)
    );

    // Rename: :rename renames the files and keeps the open document (unsaved edits too).
    typ(&mut h, ":w");
    key(&mut h, egui::Key::Enter);
    typ(&mut h, ":rename Coins wall");
    key(&mut h, egui::Key::Enter);
    let new = root.join("_collages/Coins wall.json");
    assert!(new.exists() && !path.exists());
    assert_eq!(h.state().collage.as_ref().unwrap().path, new);
    assert_eq!(found::collage::load(&new).unwrap().name, "Coins wall");
    assert!(!h.state().collage_dirty());
    // …and from the sidebar: right-click › Rename…
    h.get_by_label("Coins wall").click_secondary();
    h.run();
    click(&mut h, "Rename…");
    h.state_mut().renaming.as_mut().unwrap().1 = "Final".into();
    h.run();
    h.key_press(egui::Key::Enter);
    h.run();
    assert!(root.join("_collages/Final.json").exists());
    assert_eq!(h.state().collage_files, [root.join("_collages/Final.json")]);
}
