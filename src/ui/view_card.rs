//! One slice view: header, image, orientation letters, scale bar, slice
//! slider and window controls.

use std::sync::Arc;

use egui::{
    Align2, Color32, ColorImage, FontId, Key, Pos2, Rect, RichText, Sense, Stroke, TextureHandle,
    TextureOptions, Ui, pos2, vec2,
};

use super::theme::{self, Theme};
use crate::data::Dataset;
use crate::geom::{Plane, letter};
use crate::render::compose::{self, Window};
use crate::render::slice::{self, Slice};

/// What the texture on screen was built from; rebuilt when any part changes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TexKey {
    generation: u64,
    plane: Plane,
    index: usize,
    window: [u32; 2],
    left_is_left: bool,
}

/// The state of one view card.
pub struct ViewCard {
    plane: Plane,
    /// Current voxel position `[i, j, k]` (the slice index is one of these).
    ijk: [usize; 3],
    /// User-set window, or `None` for automatic.
    window: Option<Window>,
    /// The displayed sub-brick and its automatic window, with the generation
    /// they were made for. Reading a sub-brick converts every voxel, so this
    /// must not happen per frame.
    cache: Option<(u64, Arc<Vec<f32>>, Window)>,
    texture: Option<(TexKey, TextureHandle)>,
}

impl Default for ViewCard {
    fn default() -> Self {
        Self {
            plane: Plane::Axial,
            ijk: [0; 3],
            window: None,
            cache: None,
            texture: None,
        }
    }
}

impl ViewCard {
    /// Point the card at a new dataset: centered position, automatic window.
    pub fn reset(&mut self, ds: &Dataset) {
        self.ijk = ds.dims.map(|n| n / 2);
        self.window = None;
        self.cache = None;
        self.texture = None;
    }

    /// The index of the current slice along the plane's slice axis.
    fn index(&self, ds: &Dataset) -> usize {
        self.ijk[ds.orient.slice_axis(self.plane)]
    }

    /// Move the slice by `delta`, clamped to the dataset.
    pub fn step(&mut self, ds: &Dataset, delta: i32) {
        let axis = ds.orient.slice_axis(self.plane);
        let max = ds.dims[axis].saturating_sub(1);
        self.ijk[axis] = self.ijk[axis]
            .saturating_add_signed(delta as isize)
            .min(max);
    }

    /// Page Up / Page Down change the slice.
    pub fn handle_keys(&mut self, ctx: &egui::Context, ds: &Dataset) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let (up, down) = ctx.input(|i| (i.key_pressed(Key::PageUp), i.key_pressed(Key::PageDown)));
        if up {
            self.step(ds, 1);
        }
        if down {
            self.step(ds, -1);
        }
    }

    /// Draw the card filling `ui`.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        ds: &Dataset,
        generation: u64,
        left_is_left: bool,
    ) {
        if !matches!(&self.cache, Some((g, ..)) if *g == generation) {
            self.cache = ds.frame(0).map(|f| {
                let w = Window::auto(&f);
                (generation, Arc::new(f), w)
            });
        }
        let Some((_, frame, auto)) = self.cache.clone() else {
            ui.label(RichText::new("this dataset has no readable sub-brick").color(theme.error));
            return;
        };
        let window = self.window.unwrap_or(auto);
        let index = self.index(ds);

        self.header(ui, theme, ds, index);
        ui.add_space(4.0);

        let footer_h = 56.0;
        let avail = ui.available_size();
        let (canvas, _) = ui.allocate_exact_size(
            vec2(avail.x, (avail.y - footer_h).max(40.0)),
            Sense::hover(),
        );
        ui.painter().rect_filled(canvas, 4.0, theme.canvas);

        if let Some(s) = slice::extract(
            &frame,
            ds.dims,
            ds.voxel_mm,
            &ds.orient,
            self.plane,
            index,
            left_is_left,
        ) {
            let key = TexKey {
                generation,
                plane: self.plane,
                index,
                window: [window.lo.to_bits(), window.hi.to_bits()],
                left_is_left,
            };
            self.paint_slice(ui, theme, canvas, &s, window, key);
        }

        ui.add_space(4.0);
        self.footer(ui, theme, ds, auto);
    }

    fn header(&mut self, ui: &mut Ui, theme: &Theme, ds: &Dataset, index: usize) {
        ui.horizontal(|ui| {
            for plane in Plane::ALL {
                let selected = plane == self.plane;
                // The default fonts have no "●", so paint the plane's dot.
                let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
                ui.painter()
                    .circle_filled(dot.center(), 4.0, plane_color(plane));
                if ui.selectable_label(selected, plane.name()).clicked() {
                    self.plane = plane;
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.label(
                    RichText::new(slice_coordinate(ds, self.plane, self.ijk, index))
                        .color(theme.text_dim)
                        .monospace(),
                );
            });
        });
    }

    fn paint_slice(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        canvas: Rect,
        s: &Slice,
        window: Window,
        key: TexKey,
    ) {
        // Upload only when something changed.
        let image = || {
            ColorImage::from_rgba_unmultiplied([s.width, s.height], &compose::gray_rgba(s, window))
        };
        match &mut self.texture {
            Some((k, handle)) if *k == key => {
                let _ = handle;
            }
            Some((k, handle)) => {
                handle.set(image(), TextureOptions::NEAREST);
                *k = key;
            }
            None => {
                let handle = ui
                    .ctx()
                    .load_texture("slice", image(), TextureOptions::NEAREST);
                self.texture = Some((key, handle));
            }
        }
        let Some((_, texture)) = &self.texture else {
            return;
        };

        // Fit, preserving physical aspect: pixels per mm.
        let mm = vec2(
            (s.width as f64 * s.pixel_mm[0]) as f32,
            (s.height as f64 * s.pixel_mm[1]) as f32,
        );
        let margin = 22.0;
        let fit =
            ((canvas.width() - 2.0 * margin) / mm.x).min((canvas.height() - 2.0 * margin) / mm.y);
        if fit <= 0.0 || !fit.is_finite() {
            return;
        }
        let rect = Rect::from_center_size(canvas.center(), mm * fit);
        let painter = ui.painter_at(canvas);
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        // Orientation letters and scale bar contrast with the canvas.
        let ink = if theme.canvas.r() > 128 {
            Color32::from_gray(60)
        } else {
            Color32::from_gray(200)
        };
        let font = FontId::proportional(13.0);
        let c = rect.center();
        painter.text(
            pos2(rect.left() - 6.0, c.y),
            Align2::RIGHT_CENTER,
            s.left,
            font.clone(),
            ink,
        );
        painter.text(
            pos2(rect.right() + 6.0, c.y),
            Align2::LEFT_CENTER,
            s.right,
            font.clone(),
            ink,
        );
        painter.text(
            pos2(c.x, rect.top() - 4.0),
            Align2::CENTER_BOTTOM,
            s.top,
            font.clone(),
            ink,
        );
        painter.text(
            pos2(c.x, rect.bottom() + 4.0),
            Align2::CENTER_TOP,
            s.bottom,
            font.clone(),
            ink,
        );
        scale_bar(&painter, canvas, fit, ink);
    }

    fn footer(&mut self, ui: &mut Ui, theme: &Theme, ds: &Dataset, auto: Window) {
        let axis = ds.orient.slice_axis(self.plane);
        let max = ds.dims[axis].saturating_sub(1);
        ui.horizontal(|ui| {
            let mut idx = self.ijk[axis];
            ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(60.0);
            if ui
                .add(egui::Slider::new(&mut idx, 0..=max).show_value(false))
                .changed()
            {
                self.ijk[axis] = idx;
            }
            ui.label(
                RichText::new(format!("{idx} / {max}"))
                    .color(theme.text_dim)
                    .monospace(),
            );
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("window").color(theme.text_dim));
            let mut w = self.window.unwrap_or(auto);
            let before = w;
            // Enough decimals to tell values apart across the data range.
            let range = (auto.hi - auto.lo).max(1e-6);
            let decimals = (2.0 - range.log10().floor()).clamp(0.0, 6.0) as usize;
            let speed = range / 200.0;
            for v in [&mut w.lo, &mut w.hi] {
                ui.add(egui::DragValue::new(v).speed(speed).max_decimals(decimals));
            }
            if w != before {
                self.window = Some(w);
            }
            if ui
                .add_enabled(self.window.is_some(), egui::Button::new("Auto"))
                .clicked()
            {
                self.window = None;
            }
        });
    }
}

fn plane_color(plane: Plane) -> Color32 {
    match plane {
        Plane::Axial => theme::AXIAL,
        Plane::Coronal => theme::CORONAL,
        Plane::Sagittal => theme::SAGITTAL,
    }
}

/// Header text for the slice: the plane's world coordinate in mm with an
/// anatomical letter, e.g. `z = 12.0 S`, and the voxel index.
fn slice_coordinate(ds: &Dataset, plane: Plane, ijk: [usize; 3], index: usize) -> String {
    let mut p = ijk;
    p[ds.orient.slice_axis(plane)] = index;
    let ras = afni_io::geometry::transform_point(&ds.ijk_to_ras, p.map(|v| v as f64));
    let axis = plane.fixed_ras_axis();
    let v = ras[axis];
    // `-0.0` would print as "-0.0".
    let shown = if v.abs() < 0.05 { 0.0 } else { v.abs() };
    format!(
        "{} = {:.1} {}",
        ["x", "y", "z"][axis],
        shown,
        letter(axis, v >= 0.0)
    )
}

/// A scale bar of a round length (1, 2, 5 × 10ⁿ mm) near 80 px wide, bottom
/// left of the canvas.
fn scale_bar(painter: &egui::Painter, canvas: Rect, px_per_mm: f32, ink: Color32) {
    let mm = nice_length_mm(80.0 / px_per_mm);
    let len = mm * px_per_mm;
    let y = canvas.bottom() - 10.0;
    let x0 = canvas.left() + 12.0;
    let stroke = Stroke::new(2.0, ink);
    painter.line_segment([pos2(x0, y), pos2(x0 + len, y)], stroke);
    for x in [x0, x0 + len] {
        painter.line_segment([Pos2::new(x, y - 3.0), Pos2::new(x, y + 3.0)], stroke);
    }
    painter.text(
        pos2(x0 + len + 6.0, y),
        Align2::LEFT_CENTER,
        format!("{mm} mm"),
        FontId::proportional(11.0),
        ink,
    );
}

/// The largest of 1, 2, 5 × 10ⁿ that is ≤ `target`.
fn nice_length_mm(target: f32) -> f32 {
    let target = target.max(1e-3);
    let mag = 10f32.powf(target.log10().floor());
    [5.0, 2.0, 1.0]
        .into_iter()
        .map(|m| m * mag)
        .find(|v| *v <= target)
        .unwrap_or(mag)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::synthetic;

    #[test]
    fn nice_lengths() {
        assert_eq!(nice_length_mm(80.0), 50.0);
        assert_eq!(nice_length_mm(30.0), 20.0);
        assert_eq!(nice_length_mm(12.0), 10.0);
        assert_eq!(nice_length_mm(1.5), 1.0);
        assert_eq!(nice_length_mm(0.3), 0.2);
    }

    #[test]
    fn step_clamps_and_reset_centers() {
        let ds = synthetic::phantom();
        let mut v = ViewCard::default();
        v.reset(&ds);
        assert_eq!(v.index(&ds), 75); // nz = 150
        v.step(&ds, -1000);
        assert_eq!(v.index(&ds), 0);
        v.step(&ds, 1000);
        assert_eq!(v.index(&ds), 149);
    }

    #[test]
    fn slice_coordinate_has_letters() {
        let ds = synthetic::phantom();
        // Phantom: z = -75 at k = 0 (I), +1 mm per slice.
        assert_eq!(
            slice_coordinate(&ds, Plane::Axial, [0, 0, 0], 0),
            "z = 75.0 I"
        );
        assert_eq!(
            slice_coordinate(&ds, Plane::Axial, [0, 0, 0], 75),
            "z = 0.0 S"
        );
        assert_eq!(
            slice_coordinate(&ds, Plane::Axial, [0, 0, 0], 85),
            "z = 10.0 S"
        );
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::data::synthetic;
    use crate::prefs::{CanvasBackground, Prefs, ThemeChoice};

    fn render(plane: Plane, theme_choice: ThemeChoice, canvas: CanvasBackground, name: &str) {
        let ds = synthetic::phantom();
        let prefs = Prefs {
            theme: theme_choice,
            canvas,
            ..Prefs::default()
        };
        let theme = Theme::resolve(&prefs, true);
        let mut card = ViewCard::default();
        card.reset(&ds);
        card.plane = plane;
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(520.0, 560.0))
            .build_ui(move |ui| {
                theme.apply(ui.ctx());
                ui.painter().rect_filled(ui.max_rect(), 0.0, theme.panel);
                card.ui(ui, &theme, &ds, 1, false);
            });
        harness.run();
        harness.snapshot(name);
    }

    #[test]
    fn view_card_axial_dark() {
        render(
            Plane::Axial,
            ThemeChoice::Dark,
            CanvasBackground::Black,
            "view_card_axial_dark",
        );
    }

    #[test]
    fn view_card_sagittal_light_white_canvas() {
        render(
            Plane::Sagittal,
            ThemeChoice::Light,
            CanvasBackground::White,
            "view_card_sagittal_light_white",
        );
    }
}
