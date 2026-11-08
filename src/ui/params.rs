use super::theme;
use crate::ops::{Op, Param};
use egui::Ui;

/// Generic parameter widgets for an op. Returns true when something changed.
/// Generic parameter widgets. `hl` = row under the keyboard cursor (highlighted; scrolled to
/// when `scroll`).
pub fn op_params(
    ui: &mut Ui,
    op: &mut Op,
    salt: impl std::hash::Hash + Copy + std::fmt::Debug,
    hl: Option<usize>,
    scroll: bool,
) -> bool {
    let mut changed = false;
    for (i, p) in op.params().into_iter().enumerate() {
        let bg = ui.painter().add(egui::Shape::Noop);
        let top = ui.cursor().min.y;
        match p {
            Param::F { label, v, min, max } => {
                let step = if max - min <= 2.0 {
                    0.01
                } else if max - min <= 20.0 {
                    0.05
                } else {
                    1.0
                };
                let r = ui.add(
                    egui::Slider::new(v, min..=max)
                        .text(label)
                        .step_by(step)
                        .smart_aim(false),
                );
                if r.double_clicked() {
                    // Double-click resets to the neutral value (quick reset).
                    *v = if min <= 0.0 && max >= 0.0 { 0.0 } else { min };
                    changed = true;
                }
                changed |= r.changed();
            }
            Param::I { label, v, min, max } => {
                changed |= ui
                    .add(egui::Slider::new(v, min..=max).text(label))
                    .changed();
            }
            Param::B { label, v } => {
                changed |= theme::check(ui, v, label).changed();
            }
            Param::C { label, v, options } => {
                ui.horizontal(|ui| {
                    egui::ComboBox::from_id_salt((salt, i, label))
                        .selected_text(options[(*v).min(options.len() - 1)])
                        .show_ui(ui, |ui| {
                            theme::popup(ui);
                            for (k, o) in options.iter().enumerate() {
                                changed |= theme::sel_value(ui, v, k, *o).changed();
                            }
                        });
                    ui.label(label);
                });
            }
        }
        if hl == Some(i) {
            let r = egui::Rect::from_x_y_ranges(
                ui.max_rect().x_range(),
                top - 2.0..=ui.cursor().min.y - 2.0,
            )
            .expand2(egui::vec2(4.0, 0.0));
            ui.painter()
                .set(bg, egui::Shape::rect_filled(r, 3.0, theme::ACCENT_SOFT));
            if scroll {
                ui.scroll_to_rect(r, None);
            }
        }
    }
    changed
}
