//! Patcher-style theme: mid-grey chrome, a light patcher canvas, dark
//! "object box" controls with light text, pastel panel tints for groups, thin coloured
//! inlet bars as category markers and blue for selection.
//!
//! Readability rules (every text/background pair is one of these):
//! - dark text (`TEXT`) on chrome, canvas, panels, tints and popups;
//! - light text (`BOX_TEXT`) on object boxes, fields and the dark status pill;
//! - white text only on `ACCENT`.

use egui::{
    Color32, CornerRadius, FontFamily, FontId, Stroke, TextStyle, Visuals, style::WidgetVisuals,
};

// ---- chrome (light → dark)
pub const CANVAS: Color32 = Color32::from_rgb(196, 199, 204); // patcher / grid / preview background
pub const PANEL: Color32 = Color32::from_rgb(180, 183, 188); // inspector, popups, windows
pub const TOOLBAR: Color32 = Color32::from_rgb(170, 173, 178); // top + bottom toolbars
pub const SIDEBAR: Color32 = Color32::from_rgb(160, 163, 168); // list column
pub const RAIL: Color32 = Color32::from_rgb(170, 173, 178); // compact icon rail
pub const LIST_SEP: Color32 = Color32::from_rgb(176, 179, 184); // 1px row separators in lists
pub const LIST_SEL: Color32 = Color32::from_rgb(196, 199, 204); // selected list row
pub const LINE: Color32 = Color32::from_rgb(138, 141, 146); // hairlines between areas
pub const HOVER: Color32 = Color32::from_rgb(150, 153, 158); // hover behind dark text

// ---- object boxes (buttons, fields, node headers)
pub const BOX: Color32 = Color32::from_rgb(88, 90, 94);
pub const BOX_HOVER: Color32 = Color32::from_rgb(106, 108, 112);
pub const BOX_ACTIVE: Color32 = Color32::from_rgb(68, 70, 74);
pub const BOX_TEXT: Color32 = Color32::from_rgb(238, 238, 238);
pub const FIELD: Color32 = Color32::from_rgb(78, 80, 84);

// ---- text
pub const TEXT: Color32 = Color32::from_rgb(22, 23, 25);
pub const TEXT_DIM: Color32 = Color32::from_rgb(58, 60, 64);
pub const TEXT_ON_CANVAS: Color32 = Color32::from_rgb(22, 23, 25);
/// Blue text for selected list rows (on `LIST_SEL`, ≥ 4.5:1).
pub const SEL_TEXT: Color32 = Color32::from_rgb(14, 80, 180);

// ---- state / markers
pub const ACCENT: Color32 = Color32::from_rgb(52, 132, 228);
pub const ACCENT_SOFT: Color32 = Color32::from_rgb(170, 196, 230);
pub const GREEN: Color32 = Color32::from_rgb(84, 170, 72);
pub const ORANGE: Color32 = Color32::from_rgb(214, 128, 36);
pub const RED: Color32 = Color32::from_rgb(196, 60, 52);
/// Inlet-bar colours per node category (yellow / green / blue object bars).
pub const IO: Color32 = Color32::from_rgb(96, 156, 236);
pub const BASIC: Color32 = Color32::from_rgb(84, 170, 72);
pub const EXPERIMENTAL: Color32 = Color32::from_rgb(226, 194, 52);

/// Pastel panel tints (coloured background panels).
pub const TINT_IO: Color32 = Color32::from_rgb(164, 184, 208);
pub const TINT_BASIC: Color32 = Color32::from_rgb(178, 198, 170);
pub const TINT_EXP: Color32 = Color32::from_rgb(206, 202, 164);
pub const TINT_TAN: Color32 = Color32::from_rgb(190, 180, 154);

// Names used by older call sites.
pub const CHROME: Color32 = TOOLBAR;
pub const SURFACE: Color32 = CANVAS;
pub const CONTROL: Color32 = BOX;
pub const CONTROL_HOVER: Color32 = HOVER;

pub fn apply(ctx: &egui::Context) {
    let mut v = Visuals::light();
    v.panel_fill = PANEL;
    v.window_fill = PANEL;
    v.extreme_bg_color = FIELD; // text fields: dark box, light text
    v.faint_bg_color = LIST_SEP;
    v.code_bg_color = BOX;
    v.override_text_color = None;
    v.weak_text_color = Some(Color32::from_rgb(160, 162, 166)); // hint text inside dark fields
    v.hyperlink_color = SEL_TEXT;
    v.selection.bg_fill = ACCENT;
    v.selection.stroke = Stroke::new(1.0, Color32::WHITE);
    v.window_stroke = Stroke::new(1.0, LINE);
    v.window_shadow = egui::Shadow {
        offset: [0, 6],
        blur: 18,
        spread: 0,
        color: Color32::from_black_alpha(60),
    };
    v.popup_shadow = egui::Shadow {
        offset: [0, 3],
        blur: 10,
        spread: 0,
        color: Color32::from_black_alpha(50),
    };
    v.window_corner_radius = CornerRadius::same(7);
    v.menu_corner_radius = CornerRadius::same(5);
    v.slider_trailing_fill = true;
    v.handle_shape = egui::style::HandleShape::Rect { aspect_ratio: 0.5 };
    let r = CornerRadius::same(4);
    let w = |bg: Color32, fg: Color32, stroke: Stroke| WidgetVisuals {
        bg_fill: bg,
        weak_bg_fill: bg,
        bg_stroke: stroke,
        corner_radius: r,
        fg_stroke: Stroke::new(1.0, fg),
        expansion: 0.0,
    };
    v.widgets.noninteractive = w(PANEL, TEXT, Stroke::new(1.0, LINE));
    v.widgets.inactive = w(BOX, BOX_TEXT, Stroke::NONE);
    v.widgets.hovered = w(BOX_HOVER, Color32::WHITE, Stroke::new(1.0, ACCENT));
    v.widgets.active = w(BOX_ACTIVE, Color32::WHITE, Stroke::new(1.0, ACCENT));
    v.widgets.open = w(BOX_ACTIVE, Color32::WHITE, Stroke::new(1.0, ACCENT));
    ctx.set_visuals_of(egui::Theme::Light, v);
    ctx.set_theme(egui::Theme::Light);

    ctx.all_styles_mut(|s| {
        s.spacing.item_spacing = egui::vec2(6.0, 5.0);
        s.spacing.button_padding = egui::vec2(8.0, 3.0);
        s.spacing.slider_width = 150.0;
        s.spacing.interact_size.y = 21.0;
        s.text_styles = [
            (
                TextStyle::Heading,
                FontId::new(16.0, FontFamily::Proportional),
            ),
            (TextStyle::Body, FontId::new(13.0, FontFamily::Proportional)),
            (
                TextStyle::Button,
                FontId::new(13.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Small,
                FontId::new(11.0, FontFamily::Proportional),
            ),
            (
                TextStyle::Monospace,
                FontId::new(12.5, FontFamily::Monospace),
            ),
        ]
        .into();
    });
}

/// Call first thing inside a ComboBox / menu closure. egui's menu style makes buttons
/// frameless, so their (light) object-box text would sit on the light popup: switch to dark
/// text and a blue hover, like a native menu.
pub fn popup(ui: &mut egui::Ui) {
    let w = &mut ui.visuals_mut().widgets;
    w.inactive.fg_stroke.color = TEXT;
    w.inactive.weak_bg_fill = Color32::TRANSPARENT;
    w.inactive.bg_fill = BOX;
    for s in [&mut w.hovered, &mut w.active, &mut w.open] {
        s.weak_bg_fill = ACCENT;
        s.bg_fill = ACCENT;
        s.fg_stroke.color = Color32::WHITE;
        s.bg_stroke = Stroke::NONE;
    }
}

/// Use light hover fills for frameless widgets that carry dark text.
fn light_hover(ui: &mut egui::Ui) {
    let w = &mut ui.visuals_mut().widgets;
    for s in [&mut w.hovered, &mut w.active, &mut w.open] {
        s.weak_bg_fill = HOVER;
        s.bg_fill = HOVER;
        s.bg_stroke = Stroke::NONE;
        s.fg_stroke.color = TEXT;
    }
}

/// Tinted panel (group background).
pub fn section(tint: Color32) -> egui::Frame {
    egui::Frame::new()
        .fill(tint)
        .corner_radius(CornerRadius::same(4))
        .inner_margin(egui::Margin::same(8))
        .stroke(Stroke::new(1.0, tint.linear_multiply(0.85)))
}

/// Dark object-box frame (node header look).
pub fn object_box() -> egui::Frame {
    egui::Frame::new()
        .fill(BOX)
        .corner_radius(CornerRadius::same(3))
        .inner_margin(egui::Margin::symmetric(6, 3))
}

/// Patch-object decorations on `r`: a coloured inlet bar on top, and grey inlet / outlet
/// nubs. Draw after the box.
pub fn object_bars(p: &egui::Painter, r: egui::Rect, color: Color32) {
    let bar = egui::Rect::from_min_max(r.min, egui::pos2(r.max.x, r.min.y + 3.0));
    p.rect_filled(
        bar,
        CornerRadius {
            nw: 3,
            ne: 3,
            sw: 0,
            se: 0,
        },
        color,
    );
    let nub = |x: f32, y: f32| egui::Rect::from_center_size(egui::pos2(x, y), egui::vec2(8.0, 3.0));
    p.rect_filled(
        nub(r.min.x + 8.0, r.max.y - 1.5),
        1.0,
        Color32::from_rgb(52, 54, 58),
    );
}

/// Small-caps section heading (list group title).
pub fn heading(ui: &mut egui::Ui, text: &str) {
    ui.label(egui::RichText::new(text).strong().color(TEXT));
}

fn label_text(text: impl Into<String>, selected: bool) -> egui::RichText {
    egui::RichText::new(text).color(if selected { Color32::WHITE } else { TEXT })
}

/// Selectable: frameless with dark text when off, accent fill + white text when on.
pub fn sel(ui: &mut egui::Ui, selected: bool, text: impl Into<String>) -> egui::Response {
    ui.scope(|ui| {
        light_hover(ui);
        if selected {
            let w = &mut ui.visuals_mut().widgets;
            for s in [&mut w.hovered, &mut w.active] {
                s.weak_bg_fill = ACCENT;
                s.bg_fill = ACCENT;
            }
        }
        ui.add(egui::Button::selectable(
            selected,
            label_text(text, selected),
        ))
    })
    .inner
}

pub fn sel_value<V: PartialEq>(
    ui: &mut egui::Ui,
    current: &mut V,
    value: V,
    text: impl Into<String>,
) -> egui::Response {
    let selected = *current == value;
    let mut r = sel(ui, selected, text);
    if r.clicked() && !selected {
        *current = value;
        r.mark_changed();
    }
    r
}

pub fn toggle(ui: &mut egui::Ui, on: &mut bool, text: impl Into<String>) -> egui::Response {
    let mut r = sel(ui, *on, text);
    if r.clicked() {
        *on = !*on;
        r.mark_changed();
    }
    r
}

pub fn check(ui: &mut egui::Ui, on: &mut bool, text: impl Into<String>) -> egui::Response {
    ui.checkbox(on, egui::RichText::new(text).color(TEXT))
}

/// Frameless toolbar icon (dark glyph, light hover).
pub fn icon(ui: &mut egui::Ui, glyph: &str, tip: &str) -> egui::Response {
    ui.scope(|ui| {
        light_hover(ui);
        ui.add(
            egui::Button::new(egui::RichText::new(glyph).color(TEXT).size(15.0))
                .frame_when_inactive(false),
        )
        .on_hover_text(tip)
    })
    .inner
}

/// Toolbar icon that is "on" (blue glyph).
pub fn icon_toggle(ui: &mut egui::Ui, on: bool, glyph: &str, tip: &str) -> egui::Response {
    ui.scope(|ui| {
        light_hover(ui);
        let col = if on { SEL_TEXT } else { TEXT };
        ui.add(
            egui::Button::new(egui::RichText::new(glyph).color(col).size(15.0))
                .frame_when_inactive(false),
        )
        .on_hover_text(tip)
    })
    .inner
}

/// Small status pill: dark text on a coloured fill, sized to its text (never stretched by
/// the surrounding layout).
pub fn badge(ui: &mut egui::Ui, text: &str, fill: Color32) -> egui::Response {
    let galley = ui
        .painter()
        .layout_no_wrap(text.to_string(), FontId::proportional(11.0), TEXT);
    let size = galley.size() + egui::vec2(14.0, 4.0);
    let (rect, resp) = ui.allocate_exact_size(size, egui::Sense::hover());
    ui.painter().rect_filled(rect, CornerRadius::same(9), fill);
    ui.painter()
        .galley(rect.min + egui::vec2(7.0, 2.0), galley, TEXT);
    resp
}

/// Line icons for the bottom toolbars, drawn with the painter (no icon font involved).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Icon {
    Sliders,
    Patch,
    Crop,
    Compare,
    Undo,
    Redo,
    Save,
    Bolt,
    Trash,
    Fit,
    Move,
    Lasso,
    RectMask,
    Export,
}

/// Flat glyph colours on the toolbar: black = on / available, grey = off, light = disabled.
pub const GLYPH_ON: Color32 = Color32::from_rgb(20, 21, 23);
pub const GLYPH_OFF: Color32 = Color32::from_rgb(92, 94, 98);
pub const GLYPH_DISABLED: Color32 = Color32::from_rgb(138, 140, 144);

/// Flat icon button (32×28) for the bottom bar: a solid glyph, no box. `label` is its
/// accessible name, `tip` the hover text. Toggles (`on = Some(..)`) are black when on and grey
/// when off; plain buttons (`on = None`) are black, light grey when disabled.
pub fn tool(
    ui: &mut egui::Ui,
    icon: Icon,
    label: &str,
    tip: &str,
    on: Option<bool>,
    enabled: bool,
) -> egui::Response {
    let sense = if enabled {
        egui::Sense::click()
    } else {
        egui::Sense::hover()
    };
    let (rect, resp) = ui.allocate_exact_size(egui::vec2(32.0, 28.0), sense);
    resp.widget_info(|| {
        egui::WidgetInfo::selected(
            egui::WidgetType::Button,
            enabled,
            on.unwrap_or(false),
            label,
        )
    });
    let col = match (enabled, on) {
        (false, _) => GLYPH_DISABLED,
        (true, Some(false)) => GLYPH_OFF,
        _ => GLYPH_ON,
    };
    // Hover: the glyph lifts slightly (flat — no box).
    let c = if enabled && resp.hovered() {
        rect.center() - egui::vec2(0.0, 1.0)
    } else {
        rect.center()
    };
    draw_icon(ui.painter(), c, icon, col, TOOLBAR);
    resp.on_hover_text(tip)
}

/// Solid, flat glyphs (~20 px). `bg` is used for cut-outs.
fn draw_icon(p: &egui::Painter, c: egui::Pos2, icon: Icon, col: Color32, bg: Color32) {
    use egui::{Shape, pos2};
    let at = |x: f32, y: f32| pos2(c.x + x, c.y + y);
    let r = |x0: f32, y0: f32, x1: f32, y1: f32| egui::Rect::from_min_max(at(x0, y0), at(x1, y1));
    let fill =
        |x0: f32, y0: f32, x1: f32, y1: f32, rad: f32| p.rect_filled(r(x0, y0, x1, y1), rad, col);
    let poly = |pts: &[(f32, f32)], color: Color32| {
        p.add(Shape::convex_polygon(
            pts.iter().map(|(x, y)| at(*x, *y)).collect(),
            color,
            Stroke::NONE,
        ));
    };
    let thick = Stroke::new(3.0, col);
    let line = |pts: &[(f32, f32)], st: Stroke| {
        p.add(Shape::line(
            pts.iter().map(|(x, y)| at(*x, *y)).collect(),
            st,
        ));
    };
    match icon {
        // Presentation: an easel / screen on legs.
        Icon::Sliders => {
            fill(-10.0, -9.0, 10.0, 3.0, 1.0);
            poly(&[(-1.5, 3.0), (1.5, 3.0), (7.0, 10.0), (4.0, 10.0)], col);
            poly(&[(-1.5, 3.0), (1.5, 3.0), (-4.0, 10.0), (-7.0, 10.0)], col);
        }
        // Patch: two solid boxes joined by a cord.
        Icon::Patch => {
            fill(-10.0, -9.0, -1.0, -2.0, 1.0);
            fill(1.0, 2.0, 10.0, 9.0, 1.0);
            line(
                &[(-5.5, -2.0), (-5.5, 0.0), (5.5, 0.0), (5.5, 2.0)],
                Stroke::new(2.4, col),
            );
        }
        Icon::Crop => {
            line(&[(-5.0, -10.0), (-5.0, 5.0), (10.0, 5.0)], thick);
            line(&[(-10.0, -5.0), (5.0, -5.0), (5.0, 10.0)], thick);
        }
        Icon::Compare => {
            fill(-9.0, -8.0, 0.0, 8.0, 1.0);
            p.rect_stroke(
                r(-9.0, -8.0, 9.0, 8.0),
                1.0,
                Stroke::new(2.4, col),
                egui::StrokeKind::Inside,
            );
        }
        Icon::Undo | Icon::Redo => {
            let f = if icon == Icon::Redo { -1.0 } else { 1.0 };
            let arc: Vec<egui::Pos2> = (0..=18)
                .map(|i| {
                    let a = -std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * i as f32 / 18.0;
                    at(f * (1.0 + 6.5 * a.cos()), 0.5 + 6.5 * a.sin())
                })
                .collect();
            p.add(Shape::line(arc, thick));
            line(&[(f * 1.0, 7.0), (f * -5.0, 7.0)], thick);
            poly(
                &[(-f, -11.0), (-f, -1.0), (f * -8.0, -6.0)].map(|(x, y)| (x, y)),
                col,
            );
        }
        // Save: a solid floppy with a label and shutter cut out.
        Icon::Save => {
            poly(
                &[
                    (-9.0, -9.0),
                    (6.0, -9.0),
                    (9.0, -6.0),
                    (9.0, 9.0),
                    (-9.0, 9.0),
                ],
                col,
            );
            p.rect_filled(r(-5.0, -9.0, 3.0, -3.5), 0.0, bg);
            p.rect_filled(r(-5.5, 1.5, 5.5, 9.0), 1.0, bg);
        }
        Icon::Bolt => {
            poly(&[(3.0, -11.0), (-7.0, 2.0), (0.0, 2.0)], col);
            poly(&[(-3.0, 11.0), (7.0, -2.0), (0.0, -2.0)], col);
            poly(&[(0.0, 2.0), (-2.0, -2.0), (0.0, -2.0), (2.0, 2.0)], col);
            poly(&[(-2.0, 2.5), (-1.0, -2.5), (2.0, -2.5), (1.0, 2.5)], col);
        }
        Icon::Trash => {
            fill(-10.0, -7.0, 10.0, -4.5, 1.0);
            fill(-3.0, -10.0, 3.0, -7.0, 1.0);
            poly(&[(-8.0, -3.0), (8.0, -3.0), (6.5, 10.0), (-6.5, 10.0)], col);
            for x in [-3.0, 0.0, 3.0] {
                p.rect_filled(r(x - 0.7, -0.5, x + 0.7, 7.5), 0.0, bg);
            }
        }
        Icon::Fit => {
            for (sx, sy) in [(-1.0, -1.0), (1.0, -1.0), (1.0, 1.0), (-1.0, 1.0)] {
                line(
                    &[
                        (9.5 * sx, 3.0 * sy),
                        (9.5 * sx, 9.5 * sy),
                        (3.0 * sx, 9.5 * sy),
                    ],
                    thick,
                );
            }
        }
        // Move: the classic arrow pointer, solid.
        Icon::Move => {
            poly(&[(-6.0, -10.0), (-6.0, 7.0), (6.0, 2.0)], col);
            poly(&[(-1.0, 2.0), (2.5, 1.0), (6.0, 9.0), (3.0, 10.5)], col);
        }
        Icon::Lasso => {
            let pts: Vec<egui::Pos2> = (0..=32)
                .map(|i| {
                    let a = i as f32 / 32.0 * std::f32::consts::TAU;
                    at(1.0 + 8.5 * a.cos(), -3.0 + 5.5 * a.sin())
                })
                .collect();
            p.add(Shape::line(pts, Stroke::new(2.6, col)));
            line(
                &[(-4.5, 2.0), (-6.5, 6.5), (-3.5, 10.0)],
                Stroke::new(2.6, col),
            );
        }
        Icon::RectMask => {
            // Solid corner blocks + dashes: a marquee.
            for (x, y) in [(-10.0, -9.0), (7.0, -9.0), (7.0, 6.0), (-10.0, 6.0)] {
                fill(x, y, x + 3.0, y + 3.0, 0.0);
            }
            for x in [-5.0, 0.5] {
                fill(x, -9.0, x + 3.5, -6.5, 0.0);
                fill(x, 6.5, x + 3.5, 9.0, 0.0);
            }
            for y in [-4.0, 1.0] {
                fill(-10.0, y, -7.5, y + 3.0, 0.0);
                fill(7.5, y, 10.0, y + 3.0, 0.0);
            }
        }
        Icon::Export => {
            fill(-10.0, -4.0, 6.0, 10.0, 1.5);
            p.rect_filled(r(-7.5, -1.5, 3.5, 7.5), 0.5, bg);
            line(&[(-3.0, 3.0), (7.5, -7.5)], thick);
            poly(&[(1.0, -10.5), (10.5, -10.5), (10.5, -1.0)], col);
        }
    }
}
