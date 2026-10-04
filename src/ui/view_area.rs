//! The view area: three linked plane cards and the Graph view,
//! arranged 1×3, 3×1 or 2×2, sharing one crosshair and one window.

use std::collections::HashMap;
use std::sync::Arc;

use egui::{Context, Frame, Key, Margin, Rect, RichText, Stroke, Ui, UiBuilder, pos2, vec2};

use super::theme::Theme;
use super::view_card::{CardContext, OverlayView, PlaneCard, export_tile, tile_size};
use super::view_state::{Layout, ViewOptions};
use super::widgets::readout::format_value;
use crate::data::Dataset;
use crate::geom::coords::{ijk_to_ras, magnitude_and_letter};
use crate::geom::{CoordOrient, Plane};
use crate::prefs::Prefs;
use crate::render::compose::Window;
use crate::render::export::{self, ExportOptions, ExportWhat, Rgba8Image, ViewsLayout};
use crate::render::graph_image;
use crate::render::layers::{self, LayerInput};
use crate::render::overlay::{OverlayFrames, max_abs};
use crate::render::resample::{self, Grid};
use crate::render::slice::{self, PlaneMap};
use afni_core::color::Rgba;

use super::graph_view::{GraphEvents, GraphInput, center_series, graph_view};
use crate::session::overlay::Binding;
use crate::session::store::{DatasetId, DatasetStore};
use crate::session::{Action as SessionAction, Cursor, LayerId, OverlayLayer, SeriesSettings};

/// Gap between cards.
const GAP: f32 = 8.0;
/// Inner margin of a card.
const PAD: f32 = 8.0;

/// How the views of one controller are zoomed and panned.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ZoomState {
    /// The zoom factor (1: the image fits its card).
    pub zoom: f32,
    /// Each plane's pan, as a fraction of the fitted image.
    pub pans: [egui::Vec2; 3],
}

impl Default for ZoomState {
    fn default() -> Self {
        Self {
            zoom: 1.0,
            pans: [egui::Vec2::ZERO; 3],
        }
    }
}

/// What the view area shows: the dataset, which of its sub-bricks, and a
/// number that changes whenever either does (the cache key).
pub struct Target<'a> {
    /// The underlay.
    pub ds: &'a Dataset,
    /// Which sub-brick of it.
    pub sub_brick: usize,
    /// Changes whenever the underlay or sub-brick does.
    pub generation: u64,
    /// The overlay layers drawn over it, bottom first.
    pub overlays: Vec<OverlayTarget<'a>>,
    /// Every dataset, for the sub-bricks that mask rules read.
    pub store: &'a DatasetStore,
    /// What the Graph view plots.
    pub series: &'a SeriesSettings,
}

/// What a layer shows at one voxel.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct LayerProbe {
    /// The color it is drawn in there; `None` when it shows nothing.
    pub drawn: Option<Rgba>,
    /// Why the layer shows nothing anywhere, when its rule cannot be
    /// evaluated (parse error, unbound letter, missing layer).
    pub problem: Option<String>,
}

/// An overlay layer and its dataset.
pub struct OverlayTarget<'a> {
    /// The layer's settings.
    pub layer: &'a OverlayLayer,
    /// The overlay dataset.
    pub ds: &'a Dataset,
    /// Underlay voxels inside the layer's surviving clusters, when the layer
    /// is restricted to them.
    pub keep: Option<Arc<Vec<bool>>>,
}

/// The overlay resampled onto the underlay's grid; rebuilt only when the
/// underlay, the overlay dataset or its sub-bricks change, never for
/// threshold or color changes.
struct OverlayCache {
    key: (crate::session::store::DatasetId, usize, usize, u64),
    frames: OverlayFrames,
}

/// The displayed sub-brick and its automatic window, valid for one
/// generation. Reading a sub-brick converts every voxel, so this must not
/// happen per frame.
struct FrameCache {
    generation: u64,
    frame: Arc<Vec<f32>>,
    auto: Window,
}

/// Dataset sub-bricks on the underlay grid by (dataset, sub-brick), each with
/// the generation it was made for.
type SubFrames = HashMap<(DatasetId, usize), (u64, Arc<Vec<f32>>)>;

/// The view area's state.
pub struct ViewArea {
    /// Layout, crosshair and orientation options (toolbar controls).
    pub options: ViewOptions,
    coord_orient: CoordOrient,
    /// User-set window, or `None` for automatic.
    window: Option<Window>,
    cache: Option<FrameCache>,
    /// The resampled overlays by layer.
    overlay_cache: HashMap<LayerId, OverlayCache>,
    /// Dataset sub-bricks that mask rules read, on the underlay grid, with the
    /// generation they were made for.
    sub_cache: SubFrames,
    /// Axial, coronal, sagittal (the order of [`Plane::ALL`]).
    cards: [PlaneCard; 3],
    /// Zoom factor of the three planes (1 fits the image in its card).
    zoom: f32,
    /// Changes the Graph view asked for (a new time point), for the app to
    /// apply after the frame.
    actions: Vec<SessionAction>,
}

impl ViewArea {
    /// A view area with options from the preferences.
    pub fn new(prefs: &Prefs) -> Self {
        Self {
            options: ViewOptions::from_prefs(prefs),
            coord_orient: prefs.coord_orient,
            window: None,
            cache: None,
            overlay_cache: HashMap::new(),
            sub_cache: HashMap::new(),
            cards: Plane::ALL.map(PlaneCard::new),
            zoom: 1.0,
            actions: Vec::new(),
        }
    }

    /// Point the view at a new dataset: automatic window, fresh textures.
    pub fn reset(&mut self) {
        self.window = None;
        self.cache = None;
        self.overlay_cache.clear();
        self.sub_cache.clear();
        self.cards.iter_mut().for_each(PlaneCard::reset);
    }

    fn ensure_cache(&mut self, t: &Target) -> Option<&FrameCache> {
        let generation = t.generation;
        if !matches!(&self.cache, Some(c) if c.generation == generation) {
            self.cache = t.ds.frame(t.sub_brick).map(|f| FrameCache {
                generation,
                auto: Window::auto(&f),
                frame: Arc::new(f),
            });
        }
        self.cache.as_ref()
    }

    /// Resample each layer's overlay onto the underlay grid where that is not
    /// done yet, and forget layers that are gone.
    fn ensure_overlays(&mut self, t: &Target) {
        self.overlay_cache
            .retain(|id, _| t.overlays.iter().any(|o| o.layer.id == *id));
        for o in &t.overlays {
            let key = (
                o.layer.dataset,
                o.layer.olay_sub,
                o.layer.thr_sub,
                t.generation,
            );
            if !matches!(self.overlay_cache.get(&o.layer.id), Some(c) if c.key == key) {
                match build_overlay(t.ds, o.ds, o.layer) {
                    Some(frames) => {
                        self.overlay_cache
                            .insert(o.layer.id, OverlayCache { key, frames });
                    }
                    None => {
                        self.overlay_cache.remove(&o.layer.id);
                    }
                }
            }
            if let Some(c) = self.overlay_cache.get_mut(&o.layer.id) {
                c.frames.keep = o.keep.clone();
            }
        }
        self.ensure_sub_frames(t);
    }

    /// Build the dataset sub-bricks that the layers' mask rules read, on the
    /// underlay grid, and forget the ones no rule reads any more.
    fn ensure_sub_frames(&mut self, t: &Target) {
        let needed: Vec<(DatasetId, usize)> = t
            .overlays
            .iter()
            .filter(|o| o.layer.as_mask)
            .flat_map(|o| o.layer.bindings.values())
            .filter_map(|b| match b {
                Binding::Sub { dataset, sub } => Some((*dataset, *sub)),
                _ => None,
            })
            .collect();
        self.sub_cache.retain(|k, _| needed.contains(k));
        for key in needed {
            if matches!(self.sub_cache.get(&key), Some((g, _)) if *g == t.generation) {
                continue;
            }
            let frame = t
                .store
                .get(key.0)
                .and_then(|d| resampled_frame(t.ds, d, key.1));
            match frame {
                Some((f, _)) => {
                    self.sub_cache.insert(key, (t.generation, f));
                }
                None => {
                    self.sub_cache.remove(&key);
                }
            }
        }
    }

    /// The sub-bricks mask rules read, for the cards.
    fn sub_frames(&self) -> HashMap<(DatasetId, usize), Arc<Vec<f32>>> {
        self.sub_cache
            .iter()
            .map(|(k, (_, f))| (*k, f.clone()))
            .collect()
    }

    /// What each layer shows at the crosshair voxel, bottom layer first,
    /// from what the views have already built. Layers are passed in because
    /// the view area does not own them.
    pub fn probe(&self, ds: &Dataset, layers: &[OverlayLayer], cur: &Cursor) -> Vec<LayerProbe> {
        let inputs: Vec<LayerInput> = layers
            .iter()
            .filter_map(|l| {
                Some(LayerInput {
                    layer: l,
                    frames: &self.overlay_cache.get(&l.id)?.frames,
                })
            })
            .collect();
        let subs = &self.sub_cache;
        let lookup = |d: DatasetId, s: usize| subs.get(&(d, s)).map(|(_, f)| f.as_slice());
        let results = layers::evaluate(
            &layers::Context {
                under: ds,
                layers: &inputs,
                sub_frame: &lookup,
                apply_keep: true,
            },
            &[cur.ijk],
        );
        layers
            .iter()
            .map(|l| {
                let Some(r) = results.iter().find(|r| r.id == l.id) else {
                    return LayerProbe::default();
                };
                LayerProbe {
                    drawn: (l.visible && r.passed[0] && r.colors[0].a > 0.0).then(|| r.colors[0]),
                    problem: r.problem.clone(),
                }
            })
            .collect()
    }

    /// Where layer `id` is on (passing its threshold, or inside its mask) at
    /// every voxel of the underlay, before any restriction to clusters; `None`
    /// until the views have built every layer. This is what a mask layer is
    /// clustered on.
    pub fn passed_everywhere(
        &self,
        ds: &Dataset,
        layers: &[OverlayLayer],
        id: LayerId,
    ) -> Option<Vec<bool>> {
        let inputs: Vec<LayerInput> = layers
            .iter()
            .map(|l| {
                Some(LayerInput {
                    layer: l,
                    frames: &self.overlay_cache.get(&l.id)?.frames,
                })
            })
            .collect::<Option<_>>()?;
        let subs = &self.sub_cache;
        let lookup = |d: DatasetId, s: usize| subs.get(&(d, s)).map(|(_, f)| f.as_slice());
        let cx = layers::Context {
            under: ds,
            layers: &inputs,
            sub_frame: &lookup,
            apply_keep: false,
        };
        let [nx, ny, nz] = ds.dims;
        let mut out = Vec::with_capacity(nx * ny * nz);
        // One plane at a time keeps the list of voxels small.
        for k in 0..nz {
            let plane: Vec<[usize; 3]> = (0..ny)
                .flat_map(|j| (0..nx).map(move |i| [i, j, k]))
                .collect();
            let result = layers::evaluate(&cx, &plane)
                .into_iter()
                .find(|r| r.id == id)?;
            if result.problem.is_some() {
                return None;
            }
            out.extend(result.passed);
        }
        Some(out)
    }

    /// A layer's OLay and Thr sub-bricks on the underlay grid (for the
    /// controls and the readout).
    pub fn overlay_frames(&self, id: LayerId) -> Option<&OverlayFrames> {
        self.overlay_cache.get(&id).map(|c| &c.frames)
    }

    /// The OLay and Thr values of a layer at the crosshair.
    pub fn overlay_values_at(&self, id: LayerId, ds: &Dataset, cur: &Cursor) -> Option<(f32, f32)> {
        let [i, j, k] = cur.ijk;
        let [nx, ny, _] = ds.dims;
        let n = i + nx * (j + ny * k);
        let f = self.overlay_frames(id)?;
        Some((*f.olay.get(n)?, *f.thr.get(n)?))
    }

    /// The map of the card the keyboard acts on.
    fn active_map(&self, ds: &Dataset, cur: &Cursor) -> PlaneMap {
        PlaneMap::new(ds.dims, &ds.orient, cur.active, self.options.left_is_left)
    }

    /// Arrow keys move the crosshair one voxel on screen in the active card;
    /// Page Up / Page Down change its slice.
    pub fn handle_keys(&mut self, ctx: &Context, ds: &Dataset, cur: &mut Cursor) {
        if ctx.egui_wants_keyboard_input() {
            return;
        }
        let pressed = |k| ctx.input(|i| i.key_pressed(k));
        let map = self.active_map(ds, cur);
        let (col, row) = map.pixel(cur.ijk);
        let index = cur.ijk[map.fixed_axis];
        let max_col = map.width - 1;
        let max_row = map.height - 1;
        let (mut c, mut r, mut idx) = (col, row, index);
        if pressed(Key::ArrowLeft) {
            c = c.saturating_sub(1);
        }
        if pressed(Key::ArrowRight) {
            c = (c + 1).min(max_col);
        }
        if pressed(Key::ArrowUp) {
            r = r.saturating_sub(1);
        }
        if pressed(Key::ArrowDown) {
            r = (r + 1).min(max_row);
        }
        if pressed(Key::PageUp) {
            idx = (idx + 1).min(ds.dims[map.fixed_axis] - 1);
        }
        if pressed(Key::PageDown) {
            idx = idx.saturating_sub(1);
        }
        cur.ijk = map.voxel(c, r, idx);
    }

    /// Draw the cards.
    pub fn ui(&mut self, ui: &mut Ui, theme: &Theme, t: &Target, cur: &mut Cursor) {
        let (ds, generation) = (t.ds, t.generation);
        let Some(cache) = self.ensure_cache(t) else {
            ui.label(RichText::new("this dataset has no readable sub-brick").color(theme.error));
            return;
        };
        let (frame, auto) = (cache.frame.clone(), cache.auto);
        let window = self.window.unwrap_or(auto);
        self.ensure_overlays(t);
        let overlays: Vec<OverlayView> = t
            .overlays
            .iter()
            .filter_map(|o| {
                Some(OverlayView {
                    layer: o.layer,
                    frames: &self.overlay_cache.get(&o.layer.id)?.frames,
                })
            })
            .collect();
        let sub_frames = self.sub_frames();
        let cx = CardContext {
            theme,
            ds,
            frame: &frame,
            window,
            generation,
            options: self.options,
            zoom: self.zoom,
            overlays,
            sub_frames: &sub_frames,
        };

        let area = ui.available_rect_before_wrap();
        // In the row layout the Graph is docked under the views when there is a
        // time series to plot: a graph dataset was chosen, or the underlay is 4D.
        let graph_docked = t.series.source.is_some() || ds.nvols > 1;
        let cells = cell_rects_with_graph(area, self.options.layout, graph_docked);
        // Axial, sagittal, coronal, then the Graph (AFNI's usual arrangement).
        let order = [Plane::Axial, Plane::Sagittal, Plane::Coronal];
        let mut label_change = None;
        let mut zoom_change = None;
        for (cell, plane) in cells.iter().zip(order) {
            let card = &mut self.cards[Plane::ALL.iter().position(|p| *p == plane).unwrap_or(0)];
            let active = cur.active == plane;
            let events = card_frame(ui, theme, *cell, active, |ui| card.ui(ui, &cx, cur));
            if let Some(label) = events.label {
                label_change = Some(label);
            }
            if let Some(z) = events.zoom {
                zoom_change = Some(z);
            }
            self.actions.extend(events.actions);
        }
        if let Some(z) = zoom_change {
            self.zoom = z;
        }
        // The slice number is one setting for all three views.
        if let Some(label) = label_change {
            self.options.slice_label = label;
        }
        if let Some(cell) = cells.get(3) {
            let events = card_frame(ui, theme, *cell, false, |ui| self.graph(ui, theme, t, cur));
            if let Some(tr) = events.set_tr {
                self.actions.push(SessionAction::SetUnderlaySubBrick(tr));
            }
            if let Some(ijk) = events.move_to {
                cur.ijk = ijk;
            }
            if events.export {
                self.actions.push(SessionAction::Export(ExportWhat::Graph));
            }
        }
    }

    /// What the Graph view needs for the crosshair.
    fn graph_input<'a>(&self, theme: &'a Theme, t: &'a Target<'a>, cur: &Cursor) -> GraphInput<'a> {
        let source = t.series.source.and_then(|id| t.store.get(id));
        let fit = t.series.fit.and_then(|id| t.store.get(id));
        GraphInput {
            theme,
            under: t.ds,
            source: source.map_or(t.ds, |d| d.as_ref()),
            source_is_under: source.is_none(),
            fit: fit.map(|d| d.as_ref()),
            settings: t.series,
            cursor: cur.ijk,
            plane: cur.active,
            left_is_left: self.options.left_is_left,
            current_tr: t.sub_brick,
        }
    }

    /// The Graph view for the crosshair, or why there is none.
    fn graph(&self, ui: &mut Ui, theme: &Theme, t: &Target, cur: &Cursor) -> GraphEvents {
        graph_view(ui, &self.graph_input(theme, t, cur))
    }

    /// The Graph as a picture of `size`, text `text_px` high, for a saved image.
    fn graph_picture(
        &self,
        theme: &Theme,
        t: &Target,
        cur: &Cursor,
        size: (usize, usize),
        text_px: f32,
        background: [u8; 3],
    ) -> Result<Rgba8Image, String> {
        let input = self.graph_input(theme, t, cur);
        let (first, values, fit) = center_series(&input)
            .ok_or("the Graph has no time series to save: choose a 4D dataset in the Graph card")?;
        let stim: Vec<(usize, usize)> = t
            .series
            .stim
            .as_ref()
            .map(|s| crate::tools::graph::series::stim_blocks(&s.on))
            .unwrap_or_default();
        Ok(graph_image::render(
            size,
            &graph_image::GraphPicture {
                first,
                values: &values,
                fit: fit.as_deref(),
                stim: &stim,
                marker: input.source_is_under.then_some(input.current_tr),
            },
            background,
            text_px,
        ))
    }

    /// How the views are zoomed and panned, to be copied to another view.
    pub fn zoom_state(&self) -> ZoomState {
        ZoomState {
            zoom: self.zoom,
            pans: [self.cards[0].pan, self.cards[1].pan, self.cards[2].pan],
        }
    }

    /// Zoom and pan like `state`.
    pub fn set_zoom_state(&mut self, state: ZoomState) {
        self.zoom = state.zoom;
        for (card, pan) in self.cards.iter_mut().zip(state.pans) {
            card.pan = pan;
        }
    }

    /// The changes the Graph view asked for since the last call.
    pub fn take_actions(&mut self) -> Vec<SessionAction> {
        std::mem::take(&mut self.actions)
    }

    /// Render what `what` asks for as pictures: the files to write, each with
    /// the suffix to add to the chosen name (empty for a single picture).
    /// Uses the same slices, window, overlays and orientation as the screen.
    pub fn export_images(
        &mut self,
        t: &Target,
        cur: &Cursor,
        what: ExportWhat,
        opts: &ExportOptions,
        background: [u8; 3],
        theme: &Theme,
    ) -> Result<Vec<(String, Rgba8Image)>, String> {
        let ds = t.ds;
        let Some(cache) = self.ensure_cache(t) else {
            return Err("this dataset has no readable sub-brick".into());
        };
        let (frame, auto) = (cache.frame.clone(), cache.auto);
        let window = self.window.unwrap_or(auto);
        self.ensure_overlays(t);
        let overlays: Vec<OverlayView> = t
            .overlays
            .iter()
            .filter_map(|o| {
                Some(OverlayView {
                    layer: o.layer,
                    frames: &self.overlay_cache.get(&o.layer.id)?.frames,
                })
            })
            .collect();
        let sub_frames = self.sub_frames();
        let cx = CardContext {
            theme,
            ds,
            frame: &frame,
            window,
            generation: t.generation,
            options: self.options,
            zoom: 1.0,
            overlays,
            sub_frames: &sub_frames,
        };
        // One scale for every picture: the smallest voxel edge is `zoom` pixels.
        let smallest = ds.voxel_mm.iter().copied().fold(f64::INFINITY, f64::min);
        let px_per_mm = f64::from(opts.zoom.clamp(1, 8)) / smallest.max(1e-6);
        // Text is sized against the tallest picture of the figure, so every
        // slice number and letter comes out the same size.
        let index_of = |plane: Plane| {
            let axis = ds.orient.slice_axis(plane);
            cur.ijk[axis]
        };
        let views = [Plane::Axial, Plane::Sagittal, Plane::Coronal];
        let reference = match what {
            ExportWhat::Views(_) => views
                .iter()
                .filter_map(|&p| tile_size(&cx, p, index_of(p), px_per_mm))
                .map(|(_, h)| h)
                .max(),
            _ => None,
        };
        let tile = |plane: Plane, index: usize, with_cursor: bool, letters: bool| {
            export_tile(
                &cx,
                plane,
                index,
                with_cursor.then_some(cur.ijk),
                opts,
                px_per_mm,
                letters,
                background,
                reference,
            )
            .ok_or_else(|| format!("the {} slice {index} cannot be drawn", plane.name()))
        };
        match what {
            ExportWhat::Slice(plane) => Ok(vec![(
                String::new(),
                tile(plane, index_of(plane), opts.crosshair, opts.letters)?,
            )]),
            ExportWhat::Views(layout) => {
                let tiles = views
                    .iter()
                    .map(|&p| tile(p, index_of(p), opts.crosshair, opts.letters))
                    .collect::<Result<Vec<_>, _>>()?;
                let gap = 2 * opts.zoom as usize;
                // The Graph, if asked for, as big as the biggest view.
                let mut tiles = tiles;
                if opts.graph {
                    // As big as the biggest view, and big enough to read.
                    let cell_w = tiles.iter().map(|t| t.width).max().unwrap_or(1).max(240);
                    let cell_h = tiles.iter().map(|t| t.height).max().unwrap_or(1).max(150);
                    let px = (cell_h as f32 * 0.045).max(10.0);
                    tiles.push(self.graph_picture(
                        theme,
                        t,
                        cur,
                        (cell_w, cell_h),
                        px,
                        background,
                    )?);
                }
                let n = tiles.len();
                Ok(match layout {
                    ViewsLayout::Individual => {
                        let mut names: Vec<String> =
                            views.iter().map(|p| p.name().to_lowercase()).collect();
                        names.push("graph".into());
                        names.into_iter().zip(tiles).collect()
                    }
                    ViewsLayout::Row => vec![(
                        String::new(),
                        export::arrange(&tiles, 1, n, gap, background),
                    )],
                    ViewsLayout::Column => vec![(
                        String::new(),
                        export::arrange(&tiles, n, 1, gap, background),
                    )],
                    // The fourth cell is where the Graph is on screen: the
                    // Graph if it was asked for, else empty.
                    ViewsLayout::Grid => vec![(
                        String::new(),
                        export::arrange(&tiles, 2, 2, gap, background),
                    )],
                })
            }
            ExportWhat::Graph => {
                let zoom = opts.zoom.clamp(1, 8) as usize;
                let size = (200 * zoom, 100 * zoom);
                let px = (size.1 as f32 * 0.045).max(10.0);
                Ok(vec![(
                    String::new(),
                    self.graph_picture(theme, t, cur, size, px, background)?,
                )])
            }
            ExportWhat::Montage(spec) => {
                let count = ds.dims[ds.orient.slice_axis(spec.plane)];
                let slices = spec.slices(count);
                if slices.is_empty() {
                    return Err("no slices in that range".into());
                }
                let tiles = slices
                    .iter()
                    .map(|&i| tile(spec.plane, i, false, false))
                    .collect::<Result<Vec<_>, _>>()?;
                // Fewer slices than tiles: use only the rows needed.
                let cols = spec.cols.max(1).min(tiles.len());
                let rows = tiles.len().div_ceil(cols);
                let mut img = export::arrange(&tiles, rows, cols, opts.zoom as usize, background);
                if opts.letters {
                    let m =
                        PlaneMap::new(ds.dims, &ds.orient, spec.plane, self.options.left_is_left);
                    let _ = m;
                    if let Some(s) = slice::extract(
                        &frame,
                        ds.dims,
                        ds.voxel_mm,
                        &ds.orient,
                        spec.plane,
                        slices[0],
                        self.options.left_is_left,
                    ) {
                        img = export::with_letters(
                            &img,
                            [s.left, s.right, s.top, s.bottom],
                            background,
                            Some(tiles[0].height),
                        );
                    }
                }
                Ok(vec![(String::new(), img)])
            }
        }
    }

    /// The strip under the views: where the crosshair is, and the window.
    pub fn readout(&mut self, ui: &mut Ui, theme: &Theme, t: &Target, cur: &Cursor) {
        self.ensure_overlays(t);
        let Some(cache) = self.ensure_cache(t) else {
            return;
        };
        let (frame, auto) = (cache.frame.clone(), cache.auto);
        let text = self.readout_text(t.ds, t.sub_brick, &frame, cur);
        // The window controls keep their place; the text gives way (and is
        // cut off with an ellipsis) when the strip is narrow.
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            self.window_controls(ui, theme, auto);
            ui.with_layout(egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(
                    egui::Label::new(RichText::new(text).color(theme.text).monospace()).truncate(),
                );
            });
        });
    }

    fn readout_text(&self, ds: &Dataset, sub_brick: usize, frame: &[f32], cur: &Cursor) -> String {
        let [i, j, k] = cur.ijk;
        let [nx, ny, _] = ds.dims;
        let ras = ijk_to_ras(&ds.ijk_to_ras, cur.ijk);
        let letters = magnitude_and_letter(ras);
        // No "-0.0": values that round to zero print as 0.0.
        let [sx, sy, sz] = self
            .coord_orient
            .ras_to_coords(ras)
            .map(|v| if v.abs() < 0.05 { 0.0 } else { v });
        let value = frame
            .get(i + nx * (j + ny * k))
            .map_or("--".to_string(), |v| format_value(*v));
        let label = ds
            .labels
            .get(sub_brick)
            .map_or(String::new(), |l| format!(" ({l})"));
        format!(
            "ijk {i} {j} {k}   x {:.1} {}  y {:.1} {}  z {:.1} {}   {} ({sx:.1}, {sy:.1}, {sz:.1})   value {value}{label}",
            letters[0].0,
            letters[0].1,
            letters[1].0,
            letters[1].1,
            letters[2].0,
            letters[2].1,
            self.coord_orient.name(),
        )
    }

    fn window_controls(&mut self, ui: &mut Ui, theme: &Theme, auto: Window) {
        // Right-to-left: widgets appear in reverse order.
        if ui
            .add_enabled(self.window.is_some(), egui::Button::new("Auto"))
            .clicked()
        {
            self.window = None;
        }
        let mut w = self.window.unwrap_or(auto);
        let before = w;
        // Enough decimals to tell values apart across the data range.
        let range = (auto.hi - auto.lo).max(1e-6);
        let decimals = (2.0 - range.log10().floor()).clamp(0.0, 6.0) as usize;
        let speed = range / 200.0;
        for v in [&mut w.hi, &mut w.lo] {
            ui.add(egui::DragValue::new(v).speed(speed).max_decimals(decimals));
        }
        ui.label(RichText::new("window").color(theme.text_dim));
        if w != before {
            self.window = Some(w);
        }
    }

    /// The underlay value at the crosshair, if the displayed sub-brick is
    /// loaded.
    pub fn value_at(&self, ds: &Dataset, cur: &Cursor) -> Option<f32> {
        let [i, j, k] = cur.ijk;
        let [nx, ny, _] = ds.dims;
        self.cache
            .as_ref()?
            .frame
            .get(i + nx * (j + ny * k))
            .copied()
    }

    /// Short text for the status bar: coordinate and display conventions.
    pub fn conventions(&self, ds: Option<&Dataset>) -> String {
        let grid = ds.map_or(String::new(), |d| format!("grid {}  ", d.orient.code()));
        format!(
            "{grid}coords {} · {}",
            self.coord_orient.name(),
            if self.options.left_is_left {
                "neurological"
            } else {
                "radiological"
            }
        )
    }
}

/// The overlay's OLay and Thr sub-bricks on the underlay grid.
/// A sub-brick of `over` on the underlay's grid (nearest neighbor; NaN
/// outside), with the largest absolute value of the original.
fn resampled_frame(under: &Dataset, over: &Dataset, sub: usize) -> Option<(Arc<Vec<f32>>, f64)> {
    let src = over.frame(sub)?;
    let top = max_abs(&src);
    let from = Grid {
        dims: over.dims,
        ijk_to_ras: &over.ijk_to_ras,
    };
    let onto = Grid {
        dims: under.dims,
        ijk_to_ras: &under.ijk_to_ras,
    };
    let frame = if from.same_as(&onto) {
        src
    } else {
        resample::nearest(&src, &from, &onto)
    };
    Some((Arc::new(frame), top))
}

/// The overlay's OLay and Thr sub-bricks on the underlay grid.
fn build_overlay(under: &Dataset, over: &Dataset, layer: &OverlayLayer) -> Option<OverlayFrames> {
    let (olay, auto_range) = resampled_frame(under, over, layer.olay_sub)?;
    let (thr, thr_max) = if layer.thr_sub == layer.olay_sub {
        (olay.clone(), auto_range)
    } else {
        resampled_frame(under, over, layer.thr_sub)?
    };
    Some(OverlayFrames {
        olay,
        thr,
        auto_range,
        thr_max,
        keep: None,
    })
}

/// Rectangles for the cells of `layout` inside `area`: three planes, plus the
/// Graph as a fourth in the grid.
pub fn cell_rects(area: Rect, layout: Layout) -> Vec<Rect> {
    cell_rects_with_graph(area, layout, false)
}

/// [`cell_rects`], and with `graph` the row layout gets a fourth cell: the
/// Graph across the whole bottom, under the three views.
pub fn cell_rects_with_graph(area: Rect, layout: Layout, graph: bool) -> Vec<Rect> {
    if layout == Layout::Row && graph {
        let top_h = (area.height() - GAP) * 0.62;
        let top = Rect::from_min_size(area.min, vec2(area.width(), top_h));
        let mut cells = cell_rects(top, Layout::Row);
        cells.push(Rect::from_min_max(
            pos2(area.left(), area.top() + top_h + GAP),
            area.max,
        ));
        return cells;
    }
    let split = |n: usize, horizontal: bool| -> Vec<Rect> {
        (0..n)
            .map(|i| {
                if horizontal {
                    let w = (area.width() - GAP * (n - 1) as f32) / n as f32;
                    Rect::from_min_size(
                        pos2(area.left() + i as f32 * (w + GAP), area.top()),
                        vec2(w, area.height()),
                    )
                } else {
                    let h = (area.height() - GAP * (n - 1) as f32) / n as f32;
                    Rect::from_min_size(
                        pos2(area.left(), area.top() + i as f32 * (h + GAP)),
                        vec2(area.width(), h),
                    )
                }
            })
            .collect()
    };
    match layout {
        Layout::Row => split(3, true),
        Layout::Column => split(3, false),
        Layout::Grid => {
            let w = (area.width() - GAP) / 2.0;
            let h = (area.height() - GAP) / 2.0;
            let at = |c: f32, r: f32| {
                Rect::from_min_size(
                    pos2(area.left() + c * (w + GAP), area.top() + r * (h + GAP)),
                    vec2(w, h),
                )
            };
            vec![at(0.0, 0.0), at(1.0, 0.0), at(0.0, 1.0), at(1.0, 1.0)]
        }
    }
}

/// A card: a framed rectangle exactly `rect` in size, active one outlined.
fn card_frame<R>(
    ui: &mut Ui,
    theme: &Theme,
    rect: Rect,
    active: bool,
    add: impl FnOnce(&mut Ui) -> R,
) -> R {
    ui.scope_builder(UiBuilder::new().max_rect(rect), |ui| {
        let stroke = Stroke::new(1.0, if active { theme.accent } else { theme.border });
        Frame::new()
            .fill(theme.card)
            .stroke(stroke)
            .corner_radius(6)
            .inner_margin(Margin::same(PAD as i8))
            .show(ui, |ui| {
                let inner = rect.size() - vec2(2.0 * PAD + 2.0, 2.0 * PAD + 2.0);
                ui.set_min_size(inner);
                ui.set_max_size(inner);
                add(ui)
            })
            .inner
    })
    .inner
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::synthetic;

    #[test]
    fn grid_has_four_cells_row_and_column_three() {
        let area = Rect::from_min_size(pos2(0.0, 0.0), vec2(1000.0, 600.0));
        let grid = cell_rects(area, Layout::Grid);
        assert_eq!(grid.len(), 4);
        assert_eq!(grid[0].width(), 496.0);
        assert_eq!(grid[1].left(), 504.0);
        assert_eq!(grid[2].top(), 304.0);
        assert_eq!(cell_rects(area, Layout::Row).len(), 3);
        // With a graph the row layout docks it across the bottom.
        let docked = cell_rects_with_graph(area, Layout::Row, true);
        assert_eq!(docked.len(), 4);
        assert_eq!(docked[3].width(), 1000.0);
        assert!(docked[3].top() > docked[0].bottom() && docked[3].bottom() == 600.0);
        assert_eq!(cell_rects_with_graph(area, Layout::Column, true).len(), 3);
        assert_eq!(cell_rects(area, Layout::Column).len(), 3);
        // Cells stay inside the area and do not overlap.
        for layout in [Layout::Row, Layout::Column, Layout::Grid] {
            let cells = cell_rects(area, layout);
            for (a, ca) in cells.iter().enumerate() {
                assert!(area.contains_rect(*ca));
                for cb in &cells[a + 1..] {
                    assert!(!ca.intersects(*cb));
                }
            }
        }
    }

    fn overlay_layer() -> OverlayLayer {
        OverlayLayer::new(
            crate::session::store::DatasetId(1),
            afni_core::afni_colors::AfniColorScale::SpectrumRedToBlue,
        )
    }

    #[test]
    fn an_overlay_on_the_same_grid_is_used_as_is() {
        let under = synthetic::phantom();
        let over = synthetic::tmap();
        let f = build_overlay(&under, &over, &overlay_layer()).unwrap();
        assert_eq!(*f.olay, over.frame(0).unwrap());
        assert!(Arc::ptr_eq(&f.olay, &f.thr)); // OLay and Thr are the same sub-brick
        assert!(f.auto_range > 5.0 && f.auto_range == f.thr_max);
    }

    #[test]
    fn an_overlay_on_another_grid_is_resampled_with_nan_outside() {
        let under = synthetic::phantom();
        let mut over = synthetic::tmap();
        over.ijk_to_ras[0][3] += 10.0; // the overlay sits 10 mm to the right
        let f = build_overlay(&under, &over, &overlay_layer()).unwrap();
        let [nx, ny, _] = under.dims;
        let n = |i: usize, j: usize, k: usize| i + nx * (j + ny * k);
        let src = over.frame(0).unwrap();
        // RAS x falls with i. The overlay's origin is 10 mm further right, so
        // underlay voxel i is at the same place as overlay voxel i + 10.
        let (i, j, k) = (60, 90, 75);
        assert_eq!(f.olay[n(i, j, k)], src[n(i + 10, j, k)]);
        // The last 10 underlay voxels along x are beyond the overlay.
        assert!(f.olay[n(nx - 1, j, k)].is_nan());
        assert!(!f.olay[n(nx - 11, j, k)].is_nan());
    }

    #[test]
    fn a_separate_thr_sub_brick_gets_its_own_frame() {
        let under = synthetic::phantom();
        let mut over = synthetic::tmap();
        over.nvols = 2;
        if let crate::data::Data::Synthetic(f) = &mut over.data {
            f.push(f[0].iter().map(|v| v * 2.0).collect());
        }
        over.labels.push("second".into());
        over.stats.push(None);
        over.fdr_curves.push(None);
        let layer = OverlayLayer {
            thr_sub: 1,
            ..overlay_layer()
        };
        let f = build_overlay(&under, &over, &layer).unwrap();
        assert!(!Arc::ptr_eq(&f.olay, &f.thr));
        assert!((f.thr_max - 2.0 * f.auto_range).abs() < 1e-3);
    }

    fn view() -> ViewArea {
        ViewArea::new(&Prefs::default())
    }

    fn center(ds: &Dataset) -> Cursor {
        Cursor {
            ijk: ds.dims.map(|n| n / 2),
            active: Plane::Axial,
        }
    }

    fn key_ctx(key: Key) -> Context {
        let ctx = Context::default();
        let mut input = egui::RawInput::default();
        input.events.push(egui::Event::Key {
            key,
            physical_key: Some(key),
            pressed: true,
            repeat: false,
            modifiers: Default::default(),
        });
        // Run one pass so the key press is visible to `ctx.input`.
        let mut output = ctx.run_ui(input, |ui| {
            ui.label("");
        });
        output.textures_delta.clear();
        ctx
    }

    #[test]
    fn arrows_move_on_screen_and_page_keys_change_slice() {
        let ds = synthetic::phantom(); // RAI: screen-left in axial = lower i
        let mut v = view();
        let mut cur = center(&ds);
        cur.active = Plane::Axial;
        v.handle_keys(&key_ctx(Key::ArrowLeft), &ds, &mut cur);
        assert_eq!(cur.ijk, [74, 90, 75]);
        v.handle_keys(&key_ctx(Key::ArrowUp), &ds, &mut cur); // anterior = lower j
        assert_eq!(cur.ijk, [74, 89, 75]);
        v.handle_keys(&key_ctx(Key::PageUp), &ds, &mut cur);
        assert_eq!(cur.ijk, [74, 89, 76]);
        cur.active = Plane::Sagittal; // slice axis is i
        v.handle_keys(&key_ctx(Key::PageDown), &ds, &mut cur);
        assert_eq!(cur.ijk, [73, 89, 76]);
    }

    #[test]
    fn keys_stop_at_the_edges() {
        let ds = synthetic::phantom();
        let mut v = view();
        let mut cur = center(&ds);
        cur.ijk = [0, 0, 0];
        v.handle_keys(&key_ctx(Key::ArrowLeft), &ds, &mut cur);
        v.handle_keys(&key_ctx(Key::ArrowUp), &ds, &mut cur);
        v.handle_keys(&key_ctx(Key::PageDown), &ds, &mut cur);
        assert_eq!(cur.ijk, [0, 0, 0]);
    }

    #[test]
    fn readout_shows_ijk_letters_both_conventions_value_and_label() {
        let ds = synthetic::phantom();
        let mut v = view();
        let mut cur = center(&ds);
        cur.ijk = [0, 0, 0];
        let frame = ds.frame(0).unwrap();
        let text = v.readout_text(&ds, 0, &frame, &cur);
        // i=0 is x=+75 RAS (Right), j=0 is y=+90 (Anterior), k=0 z=-75 (I).
        assert!(
            text.starts_with("ijk 0 0 0   x 75.0 R  y 90.0 A  z 75.0 I"),
            "{text}"
        );
        assert!(text.contains("RAI (-75.0, -90.0, -75.0)"), "{text}");
        assert!(text.ends_with("(anat)"), "{text}");
        let mid = center(&ds);
        assert!(
            v.readout_text(&ds, 0, &frame, &mid)
                .contains("RAI (0.0, 0.0, 0.0)")
        );
        v.coord_orient = CoordOrient::Lpi;
        assert!(
            v.readout_text(&ds, 0, &frame, &cur)
                .contains("LPI (75.0, 90.0, -75.0)")
        );
    }
}

#[cfg(test)]
mod render_tests {
    use super::*;
    use crate::data::synthetic;
    use crate::prefs::{CanvasBackground, ThemeChoice};
    use crate::ui::shell;

    /// Render toolbar + views + readout and snapshot it.
    fn snapshot(
        name: &str,
        with_overlay: bool,
        layout: Layout,
        theme: ThemeChoice,
        canvas: CanvasBackground,
        size: egui::Vec2,
    ) {
        let ds = synthetic::phantom();
        let prefs = Prefs {
            theme,
            canvas,
            ..Prefs::default()
        };
        let theme = Theme::resolve(&prefs, true);
        let mut view = ViewArea::new(&prefs);
        view.options.layout = layout;
        let mut cur = Cursor {
            ijk: [58, 70, 90],
            active: Plane::Axial,
        };
        let over = synthetic::tmap();
        let store = DatasetStore::default();
        let series = SeriesSettings::default();
        let mut layer = OverlayLayer::new(crate::session::store::DatasetId(1), prefs.colorscale);
        layer.threshold = 3.1;
        let mut harness = egui_kittest::Harness::builder()
            .with_size(size)
            .build_ui(move |ui| {
                theme.apply(ui.ctx());
                let t = Target {
                    overlays: if with_overlay {
                        vec![OverlayTarget {
                            layer: &layer,
                            ds: &over,
                            keep: None,
                        }]
                    } else {
                        Vec::new()
                    },
                    ds: &ds,
                    sub_brick: 0,
                    generation: 1,
                    store: &store,
                    series: &series,
                };
                egui::Panel::top("toolbar").show(ui, |ui| {
                    shell::toolbar(ui, &theme, Some(&ds), &mut view.options, None);
                });
                egui::Panel::bottom("readout").show(ui, |ui| {
                    view.readout(ui, &theme, &t, &cur);
                });
                egui::CentralPanel::default().show(ui, |ui| view.ui(ui, &theme, &t, &mut cur));
            });
        harness.run();
        harness.snapshot(name);
    }

    #[test]
    fn grid_dark() {
        snapshot(
            "view_area_grid_dark",
            false,
            Layout::Grid,
            ThemeChoice::Dark,
            CanvasBackground::Black,
            vec2(1100.0, 760.0),
        );
    }

    #[test]
    fn row_light_white_canvas() {
        snapshot(
            "view_area_row_light_white",
            false,
            Layout::Row,
            ThemeChoice::Light,
            CanvasBackground::White,
            vec2(1300.0, 520.0),
        );
    }

    #[test]
    fn column_dark() {
        snapshot(
            "view_area_column_dark",
            false,
            Layout::Column,
            ThemeChoice::Dark,
            CanvasBackground::Black,
            vec2(700.0, 1000.0),
        );
    }

    #[test]
    fn grid_dark_with_overlay() {
        snapshot(
            "view_area_grid_overlay",
            true,
            Layout::Grid,
            ThemeChoice::Dark,
            CanvasBackground::Black,
            vec2(1100.0, 760.0),
        );
    }
}
