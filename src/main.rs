use found::{AppConfig, FoundApp};

fn main() -> eframe::Result<()> {
    env_logger::init();
    // Optional: `found <library-folder>` opens that library directly; otherwise the startup
    // picker (or the last library, if enabled in Settings) is used.
    // (Ignore flags such as the `-psn_…` process id older macOS passes to apps opened from Finder.)
    let library = std::env::args()
        .skip(1)
        .find(|a| !a.starts_with('-'))
        .map(std::path::PathBuf::from);
    let settings_file = found::settings::Settings::default_file();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("Found")
            .with_inner_size([1480.0, 940.0])
            .with_min_inner_size([900.0, 600.0])
            .with_drag_and_drop(true)
            .with_icon(std::sync::Arc::new(app_icon()))
            // Toolbar lives in the title bar (macOS): content under a transparent title bar.
            .with_fullsize_content_view(true)
            .with_titlebar_shown(false)
            .with_title_shown(false),
        ..Default::default()
    };
    eframe::run_native(
        "Found",
        options,
        Box::new(move |cc| {
            Ok(Box::new(FoundApp::from_creation_context(
                cc,
                AppConfig {
                    library,
                    settings_file,
                    sync: false,
                },
            )))
        }),
    )
}

/// The app icon (assets/icon.png — see scripts/set-icon.sh), also shown in the Dock when
/// running unbundled.
fn app_icon() -> egui::IconData {
    let img = image::load_from_memory(include_bytes!("../assets/icon.png")).map(|i| i.to_rgba8());
    match img {
        Ok(i) => egui::IconData {
            width: i.width(),
            height: i.height(),
            rgba: i.into_raw(),
        },
        Err(_) => egui::IconData::default(),
    }
}
