//! One plane's view card: header, image with crosshair, orientation letters,
//! scale bar, zoom, and slice slider.

use std::collections::HashMap;
use std::sync::Arc;

use afni_core::color::Rgba;
use egui::{
    Align2, Color32, ColorImage, FontId, Pos2, Rect, RichText, Sense, Stroke, TextureHandle,
    TextureOptions, Ui, pos2, vec2,
};

use super::theme::{self, Theme};
use super::view_state::ViewOptions;
use crate::data::Dataset;
use crate::geom::Plane;
use crate::geom::coords::{ijk_to_ras, magnitude_and_letter};
use crate::render::compose::{self, Window};
use crate::render::export::{self, ExportOptions, ExportWhat, Rgba8Image, ViewsLayout};
use crate::render::label::{Corner, LabelSize, SliceLabel};
use crate::render::layers::{self, LayerInput};
use crate::render::overlay::{OverlayFrames, contrast_outline, outline_only};
use crate::render::slice::{self, PlaneMap, Slice};
use crate::session::store::DatasetId;
use crate::session::{Action as SessionAction, Cursor, OverlayLayer};

/// Radius of the gap left in the crosshair around the focus point, in points.
const GAP: f32 = 7.0;

/// What the texture on screen was built from; rebuilt when any part changes.
#[derive(Debug, Clone, Copy, PartialEq)]
struct TexKey {
    generation: u64,
    index: usize,
    window: [u32; 2],
    left_is_left: bool,
    /// The visible layers' ids and display keys, combined; 0 when no overlay
    /// is drawn.
    overlay: u64,
}

/// What a card's right-click menu asked for.
#[derive(Debug, Default)]
pub struct CardEvents {
    /// A new slice-number setting (for all three views).
    pub label: Option<SliceLabel>,
    /// Saving images, and the like, for the app to carry out.
    pub actions: Vec<SessionAction>,
}

/// What every card needs to draw, shared by the three planes.
pub struct CardContext<'a> {
    /// Colors.
    pub theme: &'a Theme,
    /// The displayed dataset.
    pub ds: &'a Dataset,
    /// Its displayed sub-brick.
    pub frame: &'a [f32],
    /// The gray window.
    pub window: Window,
    /// Bumped whenever the dataset changes (texture cache key).
    pub generation: u64,
    /// Layout, crosshair and orientation options.
    pub options: ViewOptions,
    /// The overlay layers to draw over the underlay, bottom first.
    pub overlays: Vec<OverlayView<'a>>,
    /// Dataset sub-bricks on the underlay grid that mask rules read.
    pub sub_frames: &'a HashMap<(DatasetId, usize), Arc<Vec<f32>>>,
}

/// An overlay layer with its data on the underlay's grid.
pub struct OverlayView<'a> {
    /// What to draw and how.
    pub layer: &'a OverlayLayer,
    /// The OLay and Thr values on the underlay grid.
    pub frames: &'a OverlayFrames,
}

/// A number that changes when any layer, its settings or the stacking order
/// does (hidden layers included: others may read them); 0 when no layer is
/// drawn.
fn overlay_key(layers: &[OverlayView]) -> u64 {
    use std::hash::{Hash, Hasher};
    if !layers.iter().any(|o| o.layer.visible) {
        return 0;
    }
    let mut h = std::collections::hash_map::DefaultHasher::new();
    for o in layers {
        (o.layer.id, o.layer.display_key()).hash(&mut h);
    }
    h.finish() | 1
}

/// The colors of each visible layer for one slice of `plane`, in drawing
/// order: filled layers bottom to top, then the outlines of boxed ("B")
/// layers, so outlines are always on top.
fn overlay_planes(
    cx: &CardContext,
    plane: Plane,
    index: usize,
    left_is_left: bool,
) -> Vec<Vec<Rgba>> {
    let ds = cx.ds;
    let map = PlaneMap::new(ds.dims, &ds.orient, plane, left_is_left);
    let voxels: Vec<[usize; 3]> = (0..map.height)
        .flat_map(|row| (0..map.width).map(move |col| (col, row)))
        .map(|(col, row)| map.voxel(col, row, index))
        .collect();
    let inputs: Vec<LayerInput> = cx
        .overlays
        .iter()
        .map(|o| LayerInput {
            layer: o.layer,
            frames: o.frames,
        })
        .collect();
    let lookup = |d: DatasetId, s: usize| cx.sub_frames.get(&(d, s)).map(|f| f.as_slice());
    let results = layers::evaluate(
        &layers::Context {
            under: ds,
            layers: &inputs,
            sub_frame: &lookup,
            apply_keep: true,
        },
        &voxels,
    );
    let (mut filled, mut outlines) = (Vec::new(), Vec::new());
    for (o, r) in cx.overlays.iter().zip(results) {
        if !o.layer.visible {
            continue;
        }
        let colors = r.colors;
        if o.layer.boxed {
            // B draws the clusters filled (with A's fade, if on) and adds a
            // solid black or white outline around the suprathreshold regions
            // on top (an outline in the fill's own color would be invisible).
            let mut edge = colors.clone();
            outline_only(&mut edge, &r.passed, map.width, map.height);
            contrast_outline(&mut edge);
            outlines.push(edge);
        }
        filled.push(colors);
    }
    filled.extend(outlines);
    filled
}

/// The plane whose slice is the fixed one along `ras_axis` (what a crosshair
/// line along that axis stands for in the other views).
pub fn plane_fixed_on(ras_axis: usize) -> Plane {
    match ras_axis {
        0 => Plane::Sagittal,
        1 => Plane::Coronal,
        _ => Plane::Axial,
    }
}

/// The plane's color: its slider and its crosshair line in the other views.
pub fn plane_color(plane: Plane) -> Color32 {
    match plane {
        Plane::Axial => theme::AXIAL,
        Plane::Coronal => theme::CORONAL,
        Plane::Sagittal => theme::SAGITTAL,
    }
}

/// One plane's view card.
pub struct PlaneCard {
    plane: Plane,
    texture: Option<(TexKey, TextureHandle)>,
}

impl PlaneCard {
    /// A card for `plane`.
    pub fn new(plane: Plane) -> Self {
        Self {
            plane,
            texture: None,
        }
    }

    /// Forget the texture (new dataset).
    pub fn reset(&mut self) {
        self.texture = None;
    }

    /// Draw the card filling `ui`; clicking or dragging on the image moves
    /// the crosshair, and right-clicking opens the menu (slice number, saving
    /// images). Returns what the menu asked for.
    pub fn ui(&mut self, ui: &mut Ui, cx: &CardContext, cur: &mut Cursor) -> CardEvents {
        let mut events = CardEvents::default();
        let ds = cx.ds;
        let left_is_left = cx.options.left_is_left;
        let map = PlaneMap::new(ds.dims, &ds.orient, self.plane, left_is_left);
        let index = cur.ijk[map.fixed_axis];

        self.header(ui, cx, cur);
        ui.add_space(4.0);

        let avail = ui.available_size();
        let canvas_size = vec2(avail.x, (avail.y - 30.0).max(40.0));
        let (canvas, response) = ui.allocate_exact_size(canvas_size, Sense::click_and_drag());
        ui.painter().rect_filled(canvas, 4.0, cx.theme.canvas);
        if response.hovered() || response.dragged() {
            cur.active = self.plane;
        }
        response.context_menu(|ui| self.menu(ui, cx, &mut events));

        if let Some(s) = slice::extract(
            cx.frame,
            ds.dims,
            ds.voxel_mm,
            &ds.orient,
            self.plane,
            index,
            left_is_left,
        ) {
            let key = TexKey {
                generation: cx.generation,
                index,
                window: [cx.window.lo.to_bits(), cx.window.hi.to_bits()],
                left_is_left,
                overlay: overlay_key(&cx.overlays),
            };
            self.paint_slice(ui, cx, canvas, &response, &map, &s, key, cur);
        }

        ui.add_space(4.0);
        self.footer(ui, cx, cur, &map);
        events
    }

    /// The right-click menu: the slice number (for all three views) and
    /// saving images.
    fn menu(&self, ui: &mut Ui, cx: &CardContext, events: &mut CardEvents) {
        let mut label = cx.options.slice_label;
        ui.checkbox(&mut label.show, "Slice number")
            .on_hover_text("Draw the slice number on every view");
        ui.menu_button("Number position", |ui| {
            for corner in Corner::ALL {
                if ui
                    .selectable_label(label.corner == corner, corner.label())
                    .clicked()
                {
                    label.corner = corner;
                    label.show = true;
                    ui.close();
                }
            }
        });
        ui.menu_button("Number size", |ui| {
            for size in LabelSize::ALL {
                if ui
                    .selectable_label(label.size == size, size.label())
                    .clicked()
                {
                    label.size = size;
                    label.show = true;
                    ui.close();
                }
            }
        });
        if label != cx.options.slice_label {
            events.label = Some(label);
        }
        ui.separator();
        if ui.button("Save this slice…").clicked() {
            events
                .actions
                .push(SessionAction::Export(ExportWhat::Slice(self.plane)));
            ui.close();
        }
        ui.menu_button("Save the three views", |ui| {
            for layout in ViewsLayout::ALL {
                if ui.button(layout.label()).clicked() {
                    events
                        .actions
                        .push(SessionAction::Export(ExportWhat::Views(layout)));
                    ui.close();
                }
            }
        });
        if ui.button("Montage and more options…").clicked() {
            events.actions.push(SessionAction::ExportDialog(self.plane));
            ui.close();
        }
    }

    fn header(&self, ui: &mut Ui, cx: &CardContext, cur: &Cursor) {
        ui.horizontal(|ui| {
            let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), Sense::hover());
            ui.painter()
                .circle_filled(dot.center(), 4.0, plane_color(self.plane));
            ui.label(
                RichText::new(self.plane.name())
                    .color(cx.theme.text)
                    .strong(),
            );
            ui.label(
                RichText::new(slice_coordinate(cx.ds, self.plane, cur.ijk))
                    .color(cx.theme.text_dim)
                    .monospace(),
            );
        });
    }

    #[allow(clippy::too_many_arguments)]
    fn paint_slice(
        &mut self,
        ui: &mut Ui,
        cx: &CardContext,
        canvas: Rect,
        response: &egui::Response,
        map: &PlaneMap,
        s: &Slice,
        key: TexKey,
        cur: &mut Cursor,
    ) {
        // Upload only when something changed.
        let image = || {
            let layers = overlay_planes(cx, self.plane, key.index, key.left_is_left);
            ColorImage::from_rgba_unmultiplied(
                [s.width, s.height],
                &compose::compose_rgba(s, cx.window, &layers),
            )
        };
        match &mut self.texture {
            Some((k, _)) if *k == key => {}
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

        // Fit, preserving physical aspect: points per mm.
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

        // Click or drag: move the crosshair to the voxel under the pointer.
        if let Some(p) = response
            .interact_pointer_pos()
            .filter(|_| response.clicked() || response.dragged())
        {
            let col = pixel_at(p.x, rect.left(), rect.width(), s.width);
            let row = pixel_at(p.y, rect.top(), rect.height(), s.height);
            cur.ijk = map.voxel(col, row, cur.ijk[map.fixed_axis]);
            cur.active = self.plane;
        }

        let painter = ui.painter_at(canvas);
        painter.image(
            texture.id(),
            rect,
            Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)),
            Color32::WHITE,
        );

        // Letters, scale bar and crosshair ring contrast with the canvas.
        let ink = if cx.theme.canvas.r() > 128 {
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
            font,
            ink,
        );
        let label = cx.options.slice_label;
        if label.show {
            slice_number(&painter, rect, key_index(cur, map), &label);
        }
        scale_bar(&painter, canvas, fit, ink);
        painter.text(
            pos2(canvas.right() - 8.0, canvas.bottom() - 8.0),
            Align2::RIGHT_BOTTOM,
            format!("{:.0}%", fit as f64 * s.pixel_mm[0] * 100.0),
            FontId::proportional(11.0),
            ink,
        );

        if cx.options.crosshair {
            self.crosshair(&painter, rect, cx, map, s, cur, ink);
        }
    }

    /// The other two planes' positions as colored lines with a gap at the
    /// focus point.
    #[allow(clippy::too_many_arguments)]
    fn crosshair(
        &self,
        painter: &egui::Painter,
        rect: Rect,
        cx: &CardContext,
        map: &PlaneMap,
        s: &Slice,
        cur: &Cursor,
        ink: Color32,
    ) {
        let (col, row) = map.pixel(cur.ijk);
        let focus = pos2(
            rect.left() + (col as f32 + 0.5) / s.width as f32 * rect.width(),
            rect.top() + (row as f32 + 0.5) / s.height as f32 * rect.height(),
        );
        let (h, v) = self.plane.screen_axes(cx.options.left_is_left);
        // The vertical line marks position along the horizontal axis, so it
        // stands for the plane that is fixed on that axis, and vice versa.
        let vertical = Stroke::new(1.0, plane_color(plane_fixed_on(h.ras_axis)));
        let horizontal = Stroke::new(1.0, plane_color(plane_fixed_on(v.ras_axis)));
        painter.line_segment(
            [pos2(focus.x, rect.top()), pos2(focus.x, focus.y - GAP)],
            vertical,
        );
        painter.line_segment(
            [pos2(focus.x, focus.y + GAP), pos2(focus.x, rect.bottom())],
            vertical,
        );
        painter.line_segment(
            [pos2(rect.left(), focus.y), pos2(focus.x - GAP, focus.y)],
            horizontal,
        );
        painter.line_segment(
            [pos2(focus.x + GAP, focus.y), pos2(rect.right(), focus.y)],
            horizontal,
        );
        painter.circle_stroke(focus, 2.5, Stroke::new(1.0, ink));
    }

    fn footer(&mut self, ui: &mut Ui, cx: &CardContext, cur: &mut Cursor, map: &PlaneMap) {
        let axis = map.fixed_axis;
        let max = cx.ds.dims[axis].saturating_sub(1);
        ui.horizontal(|ui| {
            let mut idx = cur.ijk[axis];
            ui.spacing_mut().slider_width = (ui.available_width() - 90.0).max(60.0);
            let changed = ui
                .scope(|ui| {
                    // The slider's filled part takes the plane's color.
                    ui.visuals_mut().selection.bg_fill = plane_color(self.plane);
                    ui.add(egui::Slider::new(&mut idx, 0..=max).show_value(false))
                        .changed()
                })
                .inner;
            if changed {
                cur.ijk[axis] = idx;
                cur.active = self.plane;
            }
            ui.label(
                RichText::new(format!("{idx} / {max}"))
                    .color(cx.theme.text_dim)
                    .monospace(),
            );
        });
    }
}

/// The pixel index under screen coordinate `p`, clamped to the image.
fn pixel_at(p: f32, start: f32, extent: f32, pixels: usize) -> usize {
    let f = ((p - start) / extent * pixels as f32).floor();
    (f.max(0.0) as usize).min(pixels.saturating_sub(1))
}

/// Header text for the slice: its world position along the fixed axis with a
/// letter, e.g. `z = 12.0 mm S`.
fn slice_coordinate(ds: &Dataset, plane: Plane, ijk: [usize; 3]) -> String {
    let ras = ijk_to_ras(&ds.ijk_to_ras, ijk);
    let axis = plane.fixed_ras_axis();
    let (mag, letter) = magnitude_and_letter(ras)[axis];
    format!("{} = {:.1} mm {}", ["x", "y", "z"][axis], mag, letter)
}

/// Slice `index` of `plane` as saved-image pixels: the underlay and every
/// visible overlay layer, made square-pixeled at `px_per_mm`, with the slice
/// number, the crosshair and the orientation letters as `opts` asks.
#[allow(clippy::too_many_arguments)]
pub fn export_tile(
    cx: &CardContext,
    plane: Plane,
    index: usize,
    cursor: Option<[usize; 3]>,
    opts: &ExportOptions,
    px_per_mm: f64,
    letters: bool,
    background: [u8; 3],
) -> Option<Rgba8Image> {
    let ds = cx.ds;
    let left_is_left = cx.options.left_is_left;
    let map = PlaneMap::new(ds.dims, &ds.orient, plane, left_is_left);
    let s = slice::extract(
        cx.frame,
        ds.dims,
        ds.voxel_mm,
        &ds.orient,
        plane,
        index,
        left_is_left,
    )?;
    let layers = overlay_planes(cx, plane, index, left_is_left);
    let rgba = compose::compose_rgba(&s, cx.window, &layers);
    let voxels = Rgba8Image::from_rgba(s.width, s.height, rgba);
    let out_w = ((s.width as f64 * s.pixel_mm[0] * px_per_mm).round() as usize).max(1);
    let out_h = ((s.height as f64 * s.pixel_mm[1] * px_per_mm).round() as usize).max(1);
    let mut img = voxels.resized(out_w, out_h);
    if let Some(ijk) = cursor {
        let (col, row) = map.pixel(ijk);
        let focus = (
            ((col as f64 + 0.5) * out_w as f64 / s.width as f64) as i64,
            ((row as f64 + 0.5) * out_h as f64 / s.height as f64) as i64,
        );
        let (h, v) = plane.screen_axes(left_is_left);
        let rgb = |c: Color32| [c.r(), c.g(), c.b()];
        export::draw_crosshair(
            &mut img,
            focus,
            rgb(plane_color(plane_fixed_on(h.ras_axis))),
            rgb(plane_color(plane_fixed_on(v.ras_axis))),
            i64::from(opts.zoom) * 2,
            i64::from((opts.zoom / 4).max(1)),
        );
    }
    export::draw_slice_number(&mut img, index, &opts.label);
    if letters {
        img = export::with_letters(&img, [s.left, s.right, s.top, s.bottom], background);
    }
    Some(img)
}

/// The slice index shown in `map`'s plane.
fn key_index(cur: &Cursor, map: &PlaneMap) -> usize {
    cur.ijk[map.fixed_axis]
}

/// The slice number in its corner of the image, white with a dark shadow so it
/// reads on any picture.
fn slice_number(painter: &egui::Painter, image: Rect, number: usize, label: &SliceLabel) {
    let font = FontId::monospace(label.size.points());
    let m = 6.0;
    let (anchor, align) = match label.corner {
        Corner::TopLeft => (image.left_top() + vec2(m, m), Align2::LEFT_TOP),
        Corner::TopRight => (image.right_top() + vec2(-m, m), Align2::RIGHT_TOP),
        Corner::BottomLeft => (image.left_bottom() + vec2(m, -m), Align2::LEFT_BOTTOM),
        Corner::BottomRight => (image.right_bottom() + vec2(-m, -m), Align2::RIGHT_BOTTOM),
    };
    let text = number.to_string();
    for d in [
        vec2(1.0, 1.0),
        vec2(-1.0, 1.0),
        vec2(1.0, -1.0),
        vec2(-1.0, -1.0),
    ] {
        painter.text(anchor + d, align, &text, font.clone(), Color32::BLACK);
    }
    painter.text(anchor, align, text, font, Color32::WHITE);
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
    fn pixel_at_clamps_to_image() {
        assert_eq!(pixel_at(50.0, 0.0, 100.0, 10), 5);
        assert_eq!(pixel_at(-20.0, 0.0, 100.0, 10), 0);
        assert_eq!(pixel_at(500.0, 0.0, 100.0, 10), 9);
    }

    #[test]
    fn crosshair_lines_stand_for_the_other_planes() {
        // In an axial card the vertical line is a sagittal position and the
        // horizontal line a coronal one.
        let (h, v) = Plane::Axial.screen_axes(false);
        assert_eq!(plane_fixed_on(h.ras_axis), Plane::Sagittal);
        assert_eq!(plane_fixed_on(v.ras_axis), Plane::Coronal);
        let (h, v) = Plane::Coronal.screen_axes(false);
        assert_eq!(plane_fixed_on(h.ras_axis), Plane::Sagittal);
        assert_eq!(plane_fixed_on(v.ras_axis), Plane::Axial);
        let (h, v) = Plane::Sagittal.screen_axes(false);
        assert_eq!(plane_fixed_on(h.ras_axis), Plane::Coronal);
        assert_eq!(plane_fixed_on(v.ras_axis), Plane::Axial);
    }

    #[test]
    fn slice_coordinate_has_letters() {
        let ds = synthetic::phantom();
        // Phantom: z = -75 at k = 0 (I), +1 mm per slice; x = +75 at i = 0 (R).
        assert_eq!(
            slice_coordinate(&ds, Plane::Axial, [0, 0, 0]),
            "z = 75.0 mm I"
        );
        assert_eq!(
            slice_coordinate(&ds, Plane::Axial, [0, 0, 75]),
            "z = 0.0 mm S"
        );
        assert_eq!(
            slice_coordinate(&ds, Plane::Axial, [0, 0, 85]),
            "z = 10.0 mm S"
        );
        assert_eq!(
            slice_coordinate(&ds, Plane::Sagittal, [0, 0, 0]),
            "x = 75.0 mm R"
        );
    }

    // ---- Overlay stacking ----

    use std::sync::Arc;

    use afni_core::afni_colors::AfniColorScale;

    use crate::session::LayerId;
    use crate::session::store::DatasetId;

    fn layer(id: u64, opacity: f32) -> OverlayLayer {
        let mut l = OverlayLayer::new(DatasetId(1), AfniColorScale::RedsAndBlues);
        l.id = LayerId(id);
        l.threshold = 1.0;
        l.opacity = opacity;
        l
    }

    /// Frames that are 5 everywhere (so everything passes) or 5 on the first
    /// slab of x and 0 elsewhere (so a region has an edge).
    fn frames(ds: &Dataset, slab: bool) -> OverlayFrames {
        let [nx, ny, nz] = ds.dims;
        let v: Vec<f32> = (0..nx * ny * nz)
            .map(|n| if !slab || n % nx < nx / 2 { 5.0 } else { 0.0 })
            .collect();
        let v = Arc::new(v);
        OverlayFrames {
            olay: v.clone(),
            thr: v,
            auto_range: 5.0,
            thr_max: 5.0,
            keep: None,
        }
    }

    #[test]
    fn overlay_planes_are_bottom_to_top_with_outlines_last_and_hidden_layers_skipped() {
        let ds = crate::data::synthetic::phantom();
        let theme = Theme::dark();
        let (fa, fb, fc, fd) = (
            frames(&ds, false),
            frames(&ds, true),
            frames(&ds, false),
            frames(&ds, false),
        );
        let (a, mut b, mut c, d) = (layer(1, 0.2), layer(2, 1.0), layer(3, 0.9), layer(4, 0.4));
        b.boxed = true;
        c.visible = false;
        let cx = CardContext {
            theme: &theme,
            ds: &ds,
            frame: &[],
            window: Window { lo: 0.0, hi: 1.0 },
            generation: 1,
            options: ViewOptions {
                layout: Default::default(),
                crosshair: false,
                left_is_left: false,
                slice_label: SliceLabel::default(),
            },
            sub_frames: &HashMap::new(),
            overlays: vec![
                OverlayView {
                    layer: &a,
                    frames: &fa,
                },
                OverlayView {
                    layer: &b,
                    frames: &fb,
                },
                OverlayView {
                    layer: &c,
                    frames: &fc,
                },
                OverlayView {
                    layer: &d,
                    frames: &fd,
                },
            ],
        };
        let planes = overlay_planes(&cx, Plane::Axial, 75, false);
        // Fills bottom to top (a, b, d; c is hidden), then b's solid outline.
        assert_eq!(planes.len(), 4);
        assert!((planes[0][0].a - 0.2).abs() < 1e-6);
        assert!((planes[1][0].a - 1.0).abs() < 1e-6); // b stays filled
        assert!((planes[2][0].a - 0.4).abs() < 1e-6);
        let outline = &planes[3];
        assert!(outline.iter().any(|c| c.a > 0.0) && outline.iter().any(|c| c.a == 0.0));
        assert!(
            outline
                .iter()
                .filter(|c| c.a > 0.0)
                .all(|c| c.a == 1.0 && c.r == c.g && c.g == c.b && (c.r == 0.0 || c.r == 1.0)),
            "the outline is solid black or white"
        );
        // The cache key sees order, visibility and settings.
        let key = overlay_key(&cx.overlays);
        assert_ne!(key, 0);
        let mut reordered = vec![
            OverlayView {
                layer: &d,
                frames: &fd,
            },
            OverlayView {
                layer: &a,
                frames: &fa,
            },
        ];
        assert_ne!(
            overlay_key(&reordered),
            overlay_key(&[
                OverlayView {
                    layer: &a,
                    frames: &fa
                },
                OverlayView {
                    layer: &d,
                    frames: &fd
                }
            ])
        );
        reordered.clear();
        assert_eq!(overlay_key(&reordered), 0);
        let only_hidden = [OverlayView {
            layer: &c,
            frames: &fc,
        }];
        assert_eq!(overlay_key(&only_hidden), 0);
    }
}
