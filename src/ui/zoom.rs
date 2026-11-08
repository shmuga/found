//! Zoom / pan for the preview and the collage canvas.
//!
//! Mouse: pinch or ⌘-scroll zooms around the pointer, scroll pans, middle-drag or
//! Space-drag pans (primary drag too when `drag_pans`). Double-click toggles fit / 100 %.
//! Keys (see `keys.rs`): `+` `-` `=` `z1`.

use egui::{Pos2, Rect, Response, Ui, Vec2, vec2};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ZoomPan {
    /// 1.0 = fit to the view.
    pub zoom: f32,
    /// Screen-space offset of the content centre from the view centre.
    pub pan: Vec2,
    /// Last layout, for keyboard zooming outside the paint pass.
    last_view: Rect,
    last_content: Vec2,
}

impl Default for ZoomPan {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pan: Vec2::ZERO,
            last_view: Rect::NOTHING,
            last_content: Vec2::ZERO,
        }
    }
}

pub const MIN_ZOOM: f32 = 0.1;
pub const MAX_ZOOM: f32 = 64.0;

impl ZoomPan {
    fn fit_scale(view: Rect, content: Vec2) -> f32 {
        if content.x <= 0.0 || content.y <= 0.0 {
            return 1.0;
        }
        (view.width() / content.x).min(view.height() / content.y)
    }

    /// Screen rect of the content.
    pub fn rect(&mut self, view: Rect, content: Vec2) -> Rect {
        self.last_view = view;
        self.last_content = content;
        let s = Self::fit_scale(view, content) * self.zoom;
        Rect::from_center_size(view.center() + self.pan, content * s)
    }

    /// Screen pixels per content pixel.
    pub fn scale(&self) -> f32 {
        Self::fit_scale(self.last_view, self.last_content) * self.zoom
    }

    pub fn percent(&self) -> f32 {
        self.scale() * 100.0
    }

    pub fn zoom_at(&mut self, factor: f32, anchor: Pos2) {
        let (view, content) = (self.last_view, self.last_content);
        if !view.is_positive() {
            self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
            return;
        }
        let before = self.rect(view, content);
        let uv = (anchor - before.min) / before.size();
        self.zoom = (self.zoom * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        let after = self.rect(view, content);
        let moved = after.min + uv * after.size();
        self.pan += anchor - moved;
        self.clamp_pan();
    }

    pub fn zoom_in(&mut self) {
        let c = self.last_view.center();
        self.zoom_at(1.25, c);
    }

    pub fn zoom_out(&mut self) {
        let c = self.last_view.center();
        self.zoom_at(0.8, c);
    }

    pub fn fit(&mut self) {
        self.zoom = 1.0;
        self.pan = Vec2::ZERO;
    }

    /// One content pixel per screen point.
    pub fn actual(&mut self) {
        let fit = Self::fit_scale(self.last_view, self.last_content);
        if fit > 0.0 {
            self.zoom = (1.0 / fit).clamp(MIN_ZOOM, MAX_ZOOM);
            self.pan = Vec2::ZERO;
        }
    }

    /// Keep at least a corner of the content on screen.
    fn clamp_pan(&mut self) {
        let s = Self::fit_scale(self.last_view, self.last_content) * self.zoom;
        let half = self.last_content * s * 0.5 + self.last_view.size() * 0.5 - vec2(40.0, 40.0);
        self.pan.x = self.pan.x.clamp(-half.x.max(0.0), half.x.max(0.0));
        self.pan.y = self.pan.y.clamp(-half.y.max(0.0), half.y.max(0.0));
    }

    /// Mouse / trackpad handling on `resp` (the view's response). Returns true if the
    /// gesture was consumed (so tools shouldn't also act on it).
    pub fn handle(&mut self, ui: &Ui, resp: &Response, drag_pans: bool) -> bool {
        let mut used = false;
        if resp.hovered() {
            let (zoom, scroll, pointer) =
                ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.pointer.hover_pos()));
            if zoom != 1.0
                && let Some(p) = pointer
            {
                self.zoom_at(zoom, p);
                used = true;
            }
            if scroll != Vec2::ZERO && !ui.input(|i| i.modifiers.alt) {
                self.pan += scroll;
                self.clamp_pan();
                used = true;
            }
        }
        let space = ui.input(|i| i.key_down(egui::Key::Space));
        let pan_drag = resp.dragged_by(egui::PointerButton::Middle)
            || (resp.dragged_by(egui::PointerButton::Primary) && (space || drag_pans));
        if pan_drag {
            self.pan += resp.drag_delta();
            self.clamp_pan();
            used = true;
        }
        if resp.double_clicked() && drag_pans {
            if (self.zoom - 1.0).abs() < 1e-3 {
                if let Some(p) = resp.interact_pointer_pos() {
                    let target = 1.0 / Self::fit_scale(self.last_view, self.last_content);
                    self.zoom_at(target / self.zoom, p);
                }
            } else {
                self.fit();
            }
            used = true;
        }
        used
    }
}
