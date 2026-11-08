//! Startup library picker and the Settings page.

use super::theme;
use crate::app::{FoundApp, View};
use crate::settings::{Layout, Settings};
use egui::{RichText, Ui};
use std::path::PathBuf;

impl FoundApp {
    /// Shown on launch (unless "open last library" is on) and after "Switch library".
    pub fn startup_picker(&mut self, ui: &mut Ui) {
        let mut open: Option<PathBuf> = None;
        let mut forget: Option<PathBuf> = None;
        ui.vertical_centered(|ui| {
            ui.add_space(80.0);
            ui.heading("Found");
            ui.label(RichText::new("Scans of found objects").color(theme::TEXT_DIM));
            ui.label(RichText::new("Choose a library folder. Collections are its sub-folders; Inbox, collages and presets folders can be changed in Settings.").color(theme::TEXT_DIM));
            ui.add_space(16.0);
        });
        let w = 560.0f32.min(ui.available_width() - 32.0);
        ui.vertical_centered(|ui| {
            ui.set_max_width(w);
            theme::section(theme::TINT_IO).show(ui, |ui| {
                ui.set_width(w);
                ui.label(
                    RichText::new("RECENT LIBRARIES")
                        .small()
                        .strong()
                        .color(theme::TEXT),
                );
                if self.settings.recent_libraries.is_empty() {
                    ui.label(RichText::new("None yet.").small());
                }
                for p in self.settings.recent_libraries.clone() {
                    ui.horizontal(|ui| {
                        let exists = p.is_dir();
                        let label = p.display().to_string();
                        if ui
                            .add_enabled(exists, egui::Button::new(&label))
                            .on_disabled_hover_text("Folder not found")
                            .clicked()
                        {
                            open = Some(p.clone());
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("🗙")
                                .on_hover_text("Remove from list (keeps files)")
                                .clicked()
                            {
                                forget = Some(p.clone());
                            }
                        });
                    });
                }
            });
            ui.add_space(10.0);
            theme::section(theme::TINT_BASIC).show(ui, |ui| {
                ui.set_width(w);
                ui.label(
                    RichText::new("OPEN OR CREATE")
                        .small()
                        .strong()
                        .color(theme::TEXT),
                );
                if ui.button("Choose library folder…").clicked()
                    && let Some(p) = rfd::FileDialog::new()
                        .set_title("Choose library folder")
                        .pick_folder()
                {
                    open = Some(p);
                }
                ui.horizontal(|ui| {
                    ui.add(
                        egui::TextEdit::singleline(&mut self.library_path_input)
                            .hint_text("Library folder path")
                            .desired_width(w - 80.0),
                    );
                    if ui
                        .add_enabled(
                            !self.library_path_input.trim().is_empty(),
                            egui::Button::new("Open"),
                        )
                        .clicked()
                    {
                        open = Some(expand_home(self.library_path_input.trim()));
                    }
                });
            });
            ui.add_space(10.0);
            if theme::check(
                ui,
                &mut self.settings.open_last_on_startup,
                "Open the last library automatically on startup",
            )
            .changed()
            {
                self.save_settings();
            }
            if !self.status.is_empty() {
                ui.add_space(8.0);
                ui.label(RichText::new(&self.status).small());
            }
        });
        if let Some(p) = forget {
            self.settings.forget(&p);
            self.save_settings();
        }
        if let Some(p) = open {
            self.library_path_input.clear();
            self.set_library(p);
        }
    }

    pub fn settings_view(&mut self, ui: &mut Ui) {
        egui::ScrollArea::vertical().show(ui, |ui| {
            ui.heading("Settings");
            if let Some(p) = &self.settings_file {
                ui.label(RichText::new(format!("Saved to {}", p.display())).small().color(theme::TEXT_DIM));
            }
            ui.add_space(8.0);
            let w = 640.0f32.min(ui.available_width());
            ui.set_max_width(w);

            if let Some(root) = self.lib.as_ref().map(|l| l.root.clone()) {
                let draft = self.layout_draft.get_or_insert_with(|| self.settings.layout_for(&root));
                let mut apply = false;
                let mut reset = false;
                theme::section(theme::TINT_IO).show(ui, |ui| {
                    ui.set_width(w);
                    ui.label(RichText::new("LIBRARY FOLDERS").small().strong().color(theme::TEXT));
                    ui.label(RichText::new(format!("Library: {}", root.display())).small());
                    ui.label(RichText::new("Relative paths are inside the library; absolute paths can be anywhere (e.g. point Inbox at your scanner's output folder).").small().color(theme::TEXT_DIM));
                    ui.add_space(4.0);
                    for (label, value) in [("Inbox", &mut draft.inbox), ("Collages", &mut draft.collages), ("Presets", &mut draft.presets)] {
                        ui.horizontal(|ui| {
                            ui.add_sized([70.0, 20.0], egui::Label::new(label));
                            ui.add(egui::TextEdit::singleline(value).desired_width(w - 230.0).id_salt(("layout", label)));
                            if ui.button(format!("Browse {label}…")).clicked()
                                && let Some(p) = rfd::FileDialog::new().set_directory(Layout::resolve(&root, value)).pick_folder()
                            {
                                *value = Layout::entry_for(&root, &p);
                            }
                        });
                    }
                    ui.horizontal(|ui| {
                        let changed = *draft != self.settings.layout_for(&root);
                        if ui.add_enabled(changed, egui::Button::new("Apply folders")).clicked() {
                            apply = true;
                        }
                        if ui.button("Defaults").clicked() {
                            reset = true;
                        }
                    });
                    ui.label(RichText::new("Changing a folder doesn't move existing files.").small().color(theme::TEXT_DIM));
                });
                if reset {
                    self.layout_draft = Some(Layout::default());
                }
                if apply && let Some(d) = self.layout_draft.take() {
                    if d.inbox.trim().is_empty() || d.collages.trim().is_empty() || d.presets.trim().is_empty() {
                        self.status = "Folder entries can't be empty".into();
                        self.layout_draft = Some(d);
                    } else {
                        self.settings.set_layout(&root, d);
                        self.save_settings();
                        self.set_library(root.clone());
                        self.view = View::Settings;
                        self.status = "Library folders updated".into();
                    }
                }
                ui.add_space(10.0);
            }

            let before = self.settings.clone();
            theme::section(theme::TINT_BASIC).show(ui, |ui| {
                ui.set_width(w);
                ui.label(RichText::new("APP").small().strong().color(theme::TEXT));
                theme::check(ui, &mut self.settings.open_last_on_startup, "Open the last library automatically on startup");
                ui.add(egui::Slider::new(&mut self.settings.jpeg_quality, 70..=100).text("JPEG export quality"));
                ui.add(egui::Slider::new(&mut self.settings.preview_edge, 1200..=4096).step_by(100.0).text("Preview size (px, long edge)"));
                ui.horizontal(|ui| {
                    ui.label("Thumbnail cache");
                    let shown = self.settings.thumb_dir().map(|p| p.display().to_string()).unwrap_or_else(|| "off".into());
                    ui.label(RichText::new(shown).small());
                });
                ui.horizontal(|ui| {
                    if ui.button("Choose cache folder…").clicked()
                        && let Some(p) = rfd::FileDialog::new().pick_folder()
                    {
                        self.settings.thumb_cache = Some(p);
                    }
                    if ui.add_enabled(self.settings.thumb_cache.is_some(), egui::Button::new("System default")).clicked() {
                        self.settings.thumb_cache = None;
                    }
                });
            });
            if self.settings != before {
                if self.settings.thumb_cache != before.thumb_cache {
                    self.thumbs.clear();
                }
                self.save_settings();
            }
            ui.add_space(10.0);
            ui.horizontal(|ui| {
                if ui.button("Switch library…").clicked() {
                    self.close_library();
                }
                if ui.button("Reset all settings").on_hover_text("Forgets recent libraries and folder layouts; no files are touched").clicked() {
                    self.settings = Settings::default();
                    self.layout_draft = None;
                    self.save_settings();
                }
                if ui
                    .button("Credits & licence…")
                    .on_hover_text("GPL-3.0; fonts and open-source components used by Found")
                    .clicked()
                    && let Some(p) = licences_file()
                {
                    let _ = std::process::Command::new("open").arg("-t").arg(p).spawn();
                }
            });
        });
    }
}

/// CREDITS.md: next to the binary in the .app (Contents/Resources), else the repo.
fn licences_file() -> Option<PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let bundled = exe.parent()?.parent()?.join("Resources/CREDITS.md");
    let repo = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("CREDITS.md");
    [bundled, repo].into_iter().find(|p| p.exists())
}

fn expand_home(s: &str) -> PathBuf {
    match s.strip_prefix("~/") {
        Some(rest) => dirs::home_dir()
            .map(|h| h.join(rest))
            .unwrap_or_else(|| PathBuf::from(s)),
        None => PathBuf::from(s),
    }
}
