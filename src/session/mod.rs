//! All application state, with no egui types.
//!
//! [`Session`] holds the dataset store and the controllers (A, B, ...). The UI
//! reads it and returns [`Action`]s; [`Session::apply`] is how tools change
//! it. The view area is the one exception: dragging the crosshair is
//! high-frequency, so it edits the active controller's [`Cursor`] directly.

pub mod action;
pub mod controller;
pub mod graph;
pub mod overlay;
pub mod series;
pub mod store;

use std::sync::Arc;

pub use action::Action;
pub use controller::{ControllerState, Cursor};
pub use overlay::{ClusterSettings, LayerId, OverlayChange, OverlayLayer, SizeUnit};
pub use series::{SeriesChange, SeriesSettings};
pub use store::{DatasetId, DatasetStore};

use afni_core::afni_colors::AfniColorScale;

use crate::data::Dataset;
use crate::geom::coords::{ijk_to_ras, ras_to_ijk};

/// Everything the viewer knows, apart from how it is drawn.
#[derive(Debug)]
pub struct Session {
    /// Every opened dataset.
    pub store: DatasetStore,
    /// The controllers; only A exists until Milestone 8.
    pub controllers: Vec<ControllerState>,
    /// Index of the controller the UI is showing.
    pub active: usize,
    /// Bumped whenever what the views display changes (underlay or
    /// sub-brick): the cache key for textures and the displayed frame.
    pub generation: u64,
    /// The color scale a new overlay starts with (`AFNI_COLORSCALE_DEFAULT`).
    pub colorscale: AfniColorScale,
    /// The number of overlay layers ever created: the next layer's id.
    next_layer: u64,
}

impl Session {
    /// A session with controller A and no data.
    pub fn new() -> Self {
        Self {
            store: DatasetStore::default(),
            controllers: vec![ControllerState::default()],
            active: 0,
            generation: 0,
            colorscale: AfniColorScale::afni_default(),
            next_layer: 0,
        }
    }

    /// The active controller.
    pub fn controller(&self) -> &ControllerState {
        &self.controllers[self.active]
    }

    /// The active controller, mutably.
    pub fn controller_mut(&mut self) -> &mut ControllerState {
        &mut self.controllers[self.active]
    }

    /// The active controller's underlay dataset.
    pub fn underlay(&self) -> Option<&Arc<Dataset>> {
        self.controller().underlay.and_then(|id| self.store.get(id))
    }

    /// Add a dataset and make it the active controller's underlay.
    pub fn add_dataset(&mut self, dataset: Dataset) -> DatasetId {
        let id = self.store.add(dataset);
        self.apply(Action::SetUnderlay(id));
        id
    }

    /// The active controller's overlay layers with their datasets, bottom
    /// first (copies: layers are small and datasets are shared). Layers whose
    /// dataset is missing are left out.
    pub fn overlay_layers(&self) -> Vec<(OverlayLayer, Arc<Dataset>)> {
        self.controller()
            .overlays
            .iter()
            .filter_map(|l| self.store.get(l.dataset).map(|d| (l.clone(), d.clone())))
            .collect()
    }

    /// The layer with this id in the active controller.
    pub fn layer(&self, id: LayerId) -> Option<&OverlayLayer> {
        self.controller().overlays.iter().find(|l| l.id == id)
    }

    /// A new layer for `dataset` with AFNI's starting settings: OLay is
    /// sub-brick 0, Thr the first statistic (else sub-brick 0), and the
    /// threshold 0.
    fn new_layer(&mut self, id: DatasetId) -> Option<OverlayLayer> {
        use overlay::first_stat_sub_brick;
        let ds = self.store.get(id)?.clone();
        self.next_layer += 1;
        let mut layer = OverlayLayer::new(id, self.colorscale);
        layer.id = LayerId(self.next_layer);
        layer.mask.color = OverlayLayer::mask_color_for(layer.id);
        layer.thr_sub = first_stat_sub_brick(&ds).unwrap_or(0);
        Some(layer)
    }

    /// May layer `id` use `binding`? The dataset and sub-brick must exist,
    /// and a reference to another layer must name an existing layer other
    /// than itself that does not (even indirectly) read `id` back.
    fn binding_is_valid(&self, id: LayerId, binding: &overlay::Binding) -> bool {
        use overlay::Binding;
        match *binding {
            Binding::Olay | Binding::Thr | Binding::Coord(_) => true,
            Binding::Sub { dataset, sub } => self.store.get(dataset).is_some_and(|d| sub < d.nvols),
            Binding::LayerMask(other) | Binding::LayerValue(other) => {
                other != id && self.layer(other).is_some() && !self.layer_reads(other, id)
            }
        }
    }

    /// A number that changes when the voxels layer `id` selects could change:
    /// its own selection and that of every layer its rule reads, through any
    /// number of others. The cache key of its clusters.
    pub fn selection_key(&self, id: LayerId) -> u64 {
        use std::hash::{Hash, Hasher};
        let mut h = std::collections::hash_map::DefaultHasher::new();
        let mut seen: Vec<LayerId> = Vec::new();
        let mut pending = vec![id];
        while let Some(l) = pending.pop() {
            if seen.contains(&l) {
                continue;
            }
            seen.push(l);
            if let Some(layer) = self.layer(l) {
                (l, layer.selection_key()).hash(&mut h);
                if layer.as_mask {
                    pending.extend(layer.referenced_layers());
                }
            }
        }
        h.finish()
    }

    /// Does layer `from` read layer `target`, directly or through others?
    pub fn layer_reads(&self, from: LayerId, target: LayerId) -> bool {
        let mut seen = vec![from];
        let mut pending = vec![from];
        while let Some(l) = pending.pop() {
            for next in self
                .layer(l)
                .map(|l| l.referenced_layers())
                .unwrap_or_default()
            {
                if next == target {
                    return true;
                }
                if !seen.contains(&next) {
                    seen.push(next);
                    pending.push(next);
                }
            }
        }
        false
    }

    /// Is dataset `id` the underlay, a layer's dataset, read by a rule, or
    /// plotted by the Graph?
    fn in_use(&self, id: DatasetId) -> bool {
        self.controllers.iter().any(|c| {
            c.underlay == Some(id)
                || c.series.source == Some(id)
                || c.series.fit == Some(id)
                || c.overlays.iter().any(|l| {
                    l.dataset == id
                        || l.bindings.values().any(
                            |b| matches!(b, overlay::Binding::Sub { dataset, .. } if *dataset == id),
                        )
                })
        })
    }

    /// Free the voxels of `id` if nothing uses it any more. Only datasets the
    /// user chose (as underlay or overlay) are ever in memory, and a dataset
    /// that is replaced is not kept.
    fn release_if_unused(&mut self, id: DatasetId) {
        if !self.in_use(id) {
            self.store.release(id);
        }
    }

    fn apply_series(&mut self, change: SeriesChange) {
        use series::MAX_IGNORE;
        let known = |s: &Self, id: DatasetId| s.store.get(id).is_some();
        let s = match change {
            SeriesChange::Source(Some(id)) if !self.store.get(id).is_some_and(|d| d.nvols > 1) => {
                return;
            }
            SeriesChange::Fit(Some(id)) if !known(self, id) => return,
            other => other,
        };
        let (old_source, old_fit) = (
            self.controller().series.source,
            self.controller().series.fit,
        );
        let cfg = &mut self.controller_mut().series;
        match s {
            SeriesChange::Source(id) => cfg.source = id,
            SeriesChange::Fit(id) => cfg.fit = id,
            SeriesChange::Matrix(n) if matches!(n, 1 | 3 | 5) => cfg.matrix = n,
            SeriesChange::Ignore(n) => cfg.ignore = n.min(MAX_IGNORE),
            SeriesChange::Detrend(d) => cfg.detrend = d,
            SeriesChange::Percent(p) => cfg.percent = p,
            SeriesChange::Stim(s) => cfg.stim = s,
            SeriesChange::Matrix(_) => {}
        }
        for old in [old_source, old_fit].into_iter().flatten() {
            self.release_if_unused(old);
        }
    }

    fn apply_layer_change(&mut self, id: LayerId, change: OverlayChange) {
        use overlay::first_stat_sub_brick;
        let Some(layer) = self.layer(id).cloned() else {
            return;
        };
        // A new dataset needs its own facts; others use the current one.
        let target = match &change {
            OverlayChange::Dataset(d) => self.store.get(*d).cloned(),
            _ => self.store.get(layer.dataset).cloned(),
        };
        let Some(ds) = target else {
            return;
        };
        // Bindings are checked against the other layers and datasets first.
        if let OverlayChange::Bind(_, Some(binding)) = &change
            && !self.binding_is_valid(id, binding)
        {
            return;
        }
        let Some(layer) = self
            .controller_mut()
            .overlays
            .iter_mut()
            .find(|l| l.id == id)
        else {
            return;
        };
        let mut replaced = None;
        match change {
            OverlayChange::Dataset(d) => {
                replaced = Some(layer.dataset);
                layer.dataset = d;
                layer.olay_sub = 0;
                layer.thr_sub = first_stat_sub_brick(&ds).unwrap_or(0);
                // The threshold stays where it was, as in AFNI.
            }
            OverlayChange::SubBricks { olay, thr } if olay < ds.nvols && thr < ds.nvols => {
                layer.olay_sub = olay;
                layer.thr_sub = thr;
            }
            OverlayChange::ColorScale(s) => layer.colorscale = s,
            OverlayChange::Signed(s) => layer.signed = s,
            OverlayChange::Range(Some(r)) if r.is_finite() && r > 0.0 => layer.range = Some(r),
            OverlayChange::Range(None) => layer.range = None,
            OverlayChange::Threshold(t) if t.is_finite() && t >= 0.0 => layer.threshold = t,
            OverlayChange::ThresholdByP(p) if p > 0.0 && p <= 1.0 => {
                if let Some(t) = layer
                    .threshold_for_p(&ds, p)
                    .filter(|t| t.is_finite() && *t >= 0.0)
                {
                    layer.threshold = t;
                }
            }
            OverlayChange::Opacity(o) if o.is_finite() => layer.opacity = o.clamp(0.0, 1.0),
            OverlayChange::Visible(v) => layer.visible = v,
            OverlayChange::Fade(f) => layer.fade = f,
            OverlayChange::Boxed(b) => layer.boxed = b,
            OverlayChange::MaskMode(on) => layer.as_mask = on,
            OverlayChange::MaskRule(rule) => layer.mask.rule = rule,
            OverlayChange::MaskColor(c) => layer.mask.color = c,
            OverlayChange::Cluster(None) => layer.cluster = None,
            OverlayChange::Cluster(Some(c)) if c.is_valid() => layer.cluster = Some(c),
            OverlayChange::Bind(letter, Some(b)) if letter.is_ascii_lowercase() => {
                layer.bindings.insert(letter, b);
            }
            OverlayChange::Bind(letter, None) => {
                layer.bindings.remove(&letter);
            }
            _ => {} // invalid values are ignored
        }
        if let Some(old) = replaced {
            self.release_if_unused(old);
        }
    }

    /// Carry out an action. Invalid ones (unknown dataset, coordinates
    /// outside the grid) are ignored.
    pub fn apply(&mut self, action: Action) {
        match action {
            Action::SetUnderlay(id) => {
                let Some(ds) = self.store.get(id).cloned() else {
                    return;
                };
                // Keep the crosshair at the same place in the world when the
                // new underlay covers it; otherwise start at its center.
                let world = self
                    .underlay()
                    .map(|old| ijk_to_ras(&old.ijk_to_ras, self.controller().cursor.ijk));
                let kept = world.and_then(|ras| ras_to_ijk(&ds.ijk_to_ras, ds.dims, ras));
                let old = self.controller().underlay;
                let c = self.controller_mut();
                c.cursor.ijk = kept.unwrap_or_else(|| ds.dims.map(|n| n / 2));
                c.underlay = Some(id);
                c.underlay_sub_brick = 0;
                self.generation += 1;
                // The dataset being replaced is not kept in memory.
                if let Some(old) = old {
                    self.release_if_unused(old);
                }
            }
            Action::SetUnderlaySubBrick(t) => {
                if self.underlay().is_some_and(|ds| t < ds.nvols) {
                    self.controller_mut().underlay_sub_brick = t;
                    self.generation += 1;
                }
            }
            Action::MoveCrosshair(ijk) => {
                if let Some(ds) = self.underlay().cloned()
                    && (0..3).all(|a| ijk[a] < ds.dims[a])
                {
                    self.controller_mut().cursor.ijk = ijk;
                }
            }
            Action::AddOverlay(id) => {
                if let Some(layer) = self.new_layer(id) {
                    self.controller_mut().overlays.push(layer);
                }
            }
            Action::RemoveOverlay(id) => {
                let freed = self.layer(id).map(|l| l.dataset);
                let layers = &mut self.controller_mut().overlays;
                layers.retain(|l| l.id != id);
                // Rules that read the removed layer lose that binding.
                for l in layers.iter_mut() {
                    l.bindings.retain(|_, b| {
                        !matches!(b, overlay::Binding::LayerMask(x) | overlay::Binding::LayerValue(x) if *x == id)
                    });
                }
                if let Some(d) = freed {
                    self.release_if_unused(d);
                }
            }
            Action::SaveClusters(_)
            | Action::LoadStim
            | Action::LoadDataset(..)
            | Action::ScanFolder(_)
            | Action::CloseFolder(_)
            | Action::CancelLoad(_) => {} // the app does the file work
            Action::Series(change) => self.apply_series(change),
            Action::MoveOverlay { id, to } => {
                let layers = &mut self.controller_mut().overlays;
                if let Some(from) = layers.iter().position(|l| l.id == id) {
                    let layer = layers.remove(from);
                    layers.insert(to.min(layers.len()), layer);
                }
            }
            Action::Layer(id, change) => self.apply_layer_change(id, change),
            Action::JumpToRas(ras) => {
                if let Some(ds) = self.underlay().cloned()
                    && let Some(ijk) = ras_to_ijk(&ds.ijk_to_ras, ds.dims, ras)
                {
                    self.controller_mut().cursor.ijk = ijk;
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::synthetic;

    #[test]
    fn adding_a_dataset_makes_it_the_underlay_and_centers_the_cursor() {
        let mut s = Session::new();
        assert!(s.underlay().is_none());
        let g = s.generation;
        let id = s.add_dataset(synthetic::phantom());
        assert_eq!(s.controller().underlay, Some(id));
        assert_eq!(s.controller().cursor.ijk, [75, 90, 75]);
        assert_eq!(s.generation, g + 1);
        assert_eq!(s.underlay().unwrap().name, "phantom");
    }

    #[test]
    fn switching_to_a_dataset_on_the_same_grid_keeps_the_crosshair() {
        let mut s = Session::new();
        s.add_dataset(synthetic::phantom());
        s.apply(Action::MoveCrosshair([10, 20, 30]));
        s.add_dataset(synthetic::tmap_for_tests());
        assert_eq!(s.controller().cursor.ijk, [10, 20, 30]);
    }

    #[test]
    fn the_crosshair_keeps_its_place_in_the_world_across_different_grids() {
        let mut s = Session::new();
        s.add_dataset(synthetic::phantom());
        s.apply(Action::MoveCrosshair([10, 20, 30]));
        let before = ijk_to_ras(&s.underlay().unwrap().ijk_to_ras, [10, 20, 30]);
        // Same size, but the grid is shifted 2 mm along x (RAS x = 77 - i).
        let mut shifted = synthetic::phantom();
        shifted.ijk_to_ras[0][3] += 2.0;
        s.add_dataset(shifted);
        let c = s.controller().cursor.ijk;
        assert_eq!(c, [12, 20, 30]);
        let after = ijk_to_ras(&s.underlay().unwrap().ijk_to_ras, c);
        assert_eq!(before, after);
    }

    #[test]
    fn switching_grids_recenters() {
        let mut s = Session::new();
        s.add_dataset(synthetic::phantom());
        s.apply(Action::MoveCrosshair([10, 20, 30]));
        let mut small = synthetic::phantom();
        small.dims = [4, 6, 8];
        s.add_dataset(small);
        assert_eq!(s.controller().cursor.ijk, [2, 3, 4]);
    }

    #[test]
    fn set_underlay_ignores_unknown_ids_and_resets_the_sub_brick() {
        let mut s = Session::new();
        let a = s.add_dataset(synthetic::phantom());
        let g = s.generation;
        s.apply(Action::SetUnderlay(DatasetId(99)));
        assert_eq!(s.generation, g);
        s.controller_mut().underlay_sub_brick = 0;
        s.apply(Action::SetUnderlay(a));
        assert_eq!(s.controller().underlay_sub_brick, 0);
    }

    #[test]
    fn sub_brick_must_exist() {
        let mut s = Session::new();
        s.add_dataset(synthetic::phantom()); // one sub-brick
        let g = s.generation;
        s.apply(Action::SetUnderlaySubBrick(1));
        assert_eq!(s.controller().underlay_sub_brick, 0);
        assert_eq!(s.generation, g);
    }

    #[test]
    fn crosshair_moves_stay_inside_the_grid() {
        let mut s = Session::new();
        s.add_dataset(synthetic::phantom());
        s.apply(Action::MoveCrosshair([1, 2, 3]));
        assert_eq!(s.controller().cursor.ijk, [1, 2, 3]);
        s.apply(Action::MoveCrosshair([150, 0, 0]));
        assert_eq!(s.controller().cursor.ijk, [1, 2, 3]);
    }

    #[test]
    fn jump_to_ras_lands_on_the_nearest_voxel_or_does_nothing() {
        let mut s = Session::new();
        s.add_dataset(synthetic::phantom());
        // Phantom: x = 75 - i, y = 90 - j, z = -75 + k.
        s.apply(Action::JumpToRas([70.2, 80.0, 10.0]));
        assert_eq!(s.controller().cursor.ijk, [5, 10, 85]);
        s.apply(Action::JumpToRas([1000.0, 0.0, 0.0]));
        assert_eq!(s.controller().cursor.ijk, [5, 10, 85]);
    }

    // ---- Overlay layers ----

    fn with_overlay() -> (Session, DatasetId, LayerId) {
        let mut s = Session::new();
        s.add_dataset(synthetic::phantom());
        let over = s.store.add(synthetic::tmap());
        s.apply(Action::AddOverlay(over));
        let id = s.controller().overlays[0].id;
        (s, over, id)
    }

    fn layer(s: &Session, id: LayerId) -> &OverlayLayer {
        s.layer(id).unwrap()
    }

    fn change(s: &mut Session, id: LayerId, c: OverlayChange) {
        s.apply(Action::Layer(id, c));
    }

    #[test]
    fn adding_an_overlay_starts_at_threshold_0_with_the_default_scale() {
        let (s, over, id) = with_overlay();
        let l = layer(&s, id);
        assert_eq!(l.dataset, over);
        assert_eq!((l.olay_sub, l.thr_sub), (0, 0));
        assert_eq!(l.colorscale, AfniColorScale::afni_default());
        assert_eq!(l.threshold, 0.0, "AFNI starts at 0");
        // p of a zero threshold is 1: everything passes.
        let p = l.p_value(&s.store.get(over).unwrap().clone()).unwrap();
        assert!((p - 1.0).abs() < 1e-9);
    }

    #[test]
    fn layers_get_distinct_ids_and_stack_bottom_first() {
        let (mut s, over, a) = with_overlay();
        s.apply(Action::AddOverlay(over));
        s.apply(Action::AddOverlay(over));
        let ids: Vec<_> = s.controller().overlays.iter().map(|l| l.id).collect();
        assert_eq!(ids.len(), 3);
        assert_eq!(ids[0], a);
        assert!(ids[0] < ids[1] && ids[1] < ids[2]);
        // An unknown dataset adds nothing.
        s.apply(Action::AddOverlay(DatasetId(99)));
        assert_eq!(s.controller().overlays.len(), 3);
    }

    #[test]
    fn layer_ids_are_not_reused_after_removal() {
        let (mut s, _over, a) = with_overlay();
        s.apply(Action::RemoveOverlay(a));
        assert!(s.controller().overlays.is_empty());
        let over = s.store.add(synthetic::tmap()); // the removed layer's was freed
        s.apply(Action::AddOverlay(over));
        assert!(s.controller().overlays[0].id > a);
    }

    #[test]
    fn moving_a_layer_restacks_without_losing_any() {
        let (mut s, over, a) = with_overlay();
        s.apply(Action::AddOverlay(over));
        s.apply(Action::AddOverlay(over));
        let ids: Vec<_> = s.controller().overlays.iter().map(|l| l.id).collect();
        s.apply(Action::MoveOverlay { id: a, to: 2 }); // bottom -> top
        let now: Vec<_> = s.controller().overlays.iter().map(|l| l.id).collect();
        assert_eq!(now, [ids[1], ids[2], ids[0]]);
        s.apply(Action::MoveOverlay { id: a, to: 0 });
        let now: Vec<_> = s.controller().overlays.iter().map(|l| l.id).collect();
        assert_eq!(now, ids);
        s.apply(Action::MoveOverlay { id: a, to: 99 }); // clamped
        assert_eq!(s.controller().overlays.last().unwrap().id, a);
        s.apply(Action::MoveOverlay {
            id: LayerId(999),
            to: 0,
        }); // unknown: ignored
        assert_eq!(s.controller().overlays.len(), 3);
    }

    #[test]
    fn changes_apply_to_the_named_layer_only() {
        let (mut s, over, a) = with_overlay();
        s.apply(Action::AddOverlay(over));
        let b = s.controller().overlays[1].id;
        change(&mut s, b, OverlayChange::Threshold(5.0));
        change(&mut s, b, OverlayChange::Opacity(0.25));
        assert_eq!(layer(&s, b).threshold, 5.0);
        assert_eq!(layer(&s, b).opacity, 0.25);
        assert_eq!(layer(&s, a).threshold, 0.0);
        assert_eq!(layer(&s, a).opacity, 1.0);
    }

    #[test]
    fn overlay_changes_apply_and_invalid_ones_are_ignored() {
        let (mut s, _, id) = with_overlay();
        change(&mut s, id, OverlayChange::Threshold(4.5));
        change(&mut s, id, OverlayChange::Signed(false));
        change(&mut s, id, OverlayChange::Range(Some(6.0)));
        change(&mut s, id, OverlayChange::Opacity(2.0)); // clamped
        change(&mut s, id, OverlayChange::Fade(true));
        change(&mut s, id, OverlayChange::Boxed(true));
        change(
            &mut s,
            id,
            OverlayChange::ColorScale(AfniColorScale::RedsAndBlues),
        );
        let l = layer(&s, id).clone();
        assert_eq!(
            (l.threshold, l.signed, l.range, l.opacity, l.fade, l.boxed),
            (4.5, false, Some(6.0), 1.0, true, true)
        );
        assert_eq!(l.colorscale, AfniColorScale::RedsAndBlues);

        for bad in [
            OverlayChange::Threshold(-1.0),
            OverlayChange::Threshold(f64::NAN),
            OverlayChange::Range(Some(0.0)),
            OverlayChange::Range(Some(-3.0)),
            OverlayChange::SubBricks { olay: 5, thr: 0 },
            OverlayChange::ThresholdByP(0.0),
            OverlayChange::ThresholdByP(2.0),
            OverlayChange::Dataset(DatasetId(99)),
        ] {
            change(&mut s, id, bad);
        }
        assert_eq!(layer(&s, id), &l);
        change(&mut s, id, OverlayChange::Range(None));
        assert_eq!(layer(&s, id).range, None);
        // A layer that does not exist is ignored.
        change(&mut s, LayerId(999), OverlayChange::Threshold(1.0));
    }

    #[test]
    fn threshold_by_p_sets_the_threshold() {
        let (mut s, _, id) = with_overlay();
        change(&mut s, id, OverlayChange::ThresholdByP(0.01));
        assert!((layer(&s, id).threshold - 2.61814).abs() < 1e-4);
    }

    #[test]
    fn the_threshold_survives_changing_the_dataset_or_sub_bricks() {
        let (mut s, _, id) = with_overlay();
        change(&mut s, id, OverlayChange::Threshold(4.0));
        let other = s.store.add(synthetic::tmap());
        change(&mut s, id, OverlayChange::Dataset(other));
        assert_eq!(layer(&s, id).threshold, 4.0);
        change(&mut s, id, OverlayChange::SubBricks { olay: 0, thr: 0 });
        assert_eq!(layer(&s, id).threshold, 4.0);
    }

    #[test]
    fn changing_a_layers_dataset_keeps_its_display_settings() {
        let (mut s, _, id) = with_overlay();
        change(
            &mut s,
            id,
            OverlayChange::ColorScale(AfniColorScale::RedsAndBlues),
        );
        change(&mut s, id, OverlayChange::Opacity(0.4));
        let other = s.store.add(synthetic::tmap());
        change(&mut s, id, OverlayChange::Dataset(other));
        assert_eq!(layer(&s, id).dataset, other);
        assert_eq!(layer(&s, id).colorscale, AfniColorScale::RedsAndBlues);
        assert_eq!(layer(&s, id).opacity, 0.4);
    }

    #[test]
    fn overlay_layers_lists_datasets_bottom_first_and_skips_nothing_valid() {
        let (mut s, over, _) = with_overlay();
        s.apply(Action::AddOverlay(over));
        let layers = s.overlay_layers();
        assert_eq!(layers.len(), 2);
        assert!(layers[0].0.id < layers[1].0.id);
        assert_eq!(layers[0].1.name, "tmap");
    }

    #[test]
    fn overlay_changes_do_not_touch_the_underlay_generation() {
        let (mut s, over, id) = with_overlay();
        let g = s.generation;
        change(&mut s, id, OverlayChange::Threshold(3.0));
        s.apply(Action::AddOverlay(over));
        s.apply(Action::RemoveOverlay(id));
        assert_eq!(s.generation, g);
    }

    // ---- Mask layers ----

    use overlay::{Binding, Coord, MaskRule};

    fn two_layers() -> (Session, LayerId, LayerId) {
        let (mut s, over, a) = with_overlay();
        s.apply(Action::AddOverlay(over));
        let b = s.controller().overlays[1].id;
        (s, a, b)
    }

    #[test]
    fn a_new_layer_is_a_color_map_with_a_and_b_bound_to_its_own_sub_bricks() {
        let (s, _, id) = with_overlay();
        let l = layer(&s, id);
        assert!(!l.as_mask);
        assert_eq!(l.mask.rule, MaskRule::Threshold);
        assert_eq!(l.binding_for('a'), Some(Binding::Olay));
        assert_eq!(l.binding_for('b'), Some(Binding::Thr));
        assert_eq!(l.binding_for('c'), None);
        // Coordinate letters mean what they do in 3dcalc unless rebound.
        assert_eq!(l.binding_for('x'), Some(Binding::Coord(Coord::X)));
        assert_eq!(l.binding_for('k'), Some(Binding::Coord(Coord::K)));
    }

    #[test]
    fn layers_get_different_mask_colors() {
        let (s, a, b) = two_layers();
        assert_ne!(layer(&s, a).mask.color, layer(&s, b).mask.color);
    }

    #[test]
    fn switching_to_a_mask_and_back_keeps_the_rule_and_color() {
        let (mut s, _, id) = with_overlay();
        change(&mut s, id, OverlayChange::MaskMode(true));
        change(
            &mut s,
            id,
            OverlayChange::MaskRule(MaskRule::Expression("step(a-3)".into())),
        );
        change(&mut s, id, OverlayChange::MaskColor([1, 2, 3]));
        change(&mut s, id, OverlayChange::MaskMode(false));
        let l = layer(&s, id);
        assert!(!l.as_mask);
        assert_eq!(l.mask.rule, MaskRule::Expression("step(a-3)".into()));
        assert_eq!(l.mask.color, [1, 2, 3]);
        change(&mut s, id, OverlayChange::MaskMode(true));
        assert!(layer(&s, id).as_mask);
    }

    #[test]
    fn a_rule_can_read_another_layers_mask_but_not_itself_or_in_a_cycle() {
        let (mut s, a, b) = two_layers();
        let bind = |s: &mut Session, id, c, b| change(s, id, OverlayChange::Bind(c, Some(b)));
        bind(&mut s, b, 'a', Binding::LayerMask(a));
        assert_eq!(layer(&s, b).bindings[&'a'], Binding::LayerMask(a));
        // Itself.
        bind(&mut s, b, 'c', Binding::LayerMask(b));
        assert!(!layer(&s, b).bindings.contains_key(&'c'));
        // A cycle: a would read b, which reads a.
        bind(&mut s, a, 'c', Binding::LayerValue(b));
        assert!(!layer(&s, a).bindings.contains_key(&'c'));
        // Unknown layer.
        bind(&mut s, a, 'c', Binding::LayerMask(LayerId(99)));
        assert!(!layer(&s, a).bindings.contains_key(&'c'));
    }

    #[test]
    fn indirect_cycles_are_rejected_too() {
        let (mut s, a, b) = two_layers();
        let over = s.controller().overlays[0].dataset;
        s.apply(Action::AddOverlay(over));
        let c = s.controller().overlays[2].id;
        change(
            &mut s,
            c,
            OverlayChange::Bind('a', Some(Binding::LayerMask(b))),
        );
        change(
            &mut s,
            b,
            OverlayChange::Bind('a', Some(Binding::LayerMask(a))),
        );
        assert!(s.layer_reads(c, a)); // c -> b -> a
        change(
            &mut s,
            a,
            OverlayChange::Bind('a', Some(Binding::LayerMask(c))),
        ); // would close the loop
        assert_eq!(layer(&s, a).bindings[&'a'], Binding::Olay);
    }

    #[test]
    fn dataset_bindings_need_an_existing_dataset_and_sub_brick() {
        let (mut s, over, id) = with_overlay();
        let sub = |dataset, sub| Some(Binding::Sub { dataset, sub });
        change(&mut s, id, OverlayChange::Bind('c', sub(over, 0)));
        assert_eq!(
            layer(&s, id).bindings[&'c'],
            Binding::Sub {
                dataset: over,
                sub: 0
            }
        );
        change(&mut s, id, OverlayChange::Bind('d', sub(over, 5)));
        change(&mut s, id, OverlayChange::Bind('e', sub(DatasetId(99), 0)));
        assert!(!layer(&s, id).bindings.contains_key(&'d'));
        assert!(!layer(&s, id).bindings.contains_key(&'e'));
    }

    #[test]
    fn only_lower_case_letters_can_be_bound_and_none_unbinds() {
        let (mut s, _, id) = with_overlay();
        change(&mut s, id, OverlayChange::Bind('C', Some(Binding::Thr)));
        change(&mut s, id, OverlayChange::Bind('3', Some(Binding::Thr)));
        assert!(!layer(&s, id).bindings.contains_key(&'C'));
        change(&mut s, id, OverlayChange::Bind('a', None));
        assert_eq!(layer(&s, id).binding_for('a'), None);
    }

    #[test]
    fn removing_a_layer_clears_the_bindings_that_read_it() {
        let (mut s, a, b) = two_layers();
        change(
            &mut s,
            b,
            OverlayChange::Bind('c', Some(Binding::LayerMask(a))),
        );
        change(
            &mut s,
            b,
            OverlayChange::Bind('d', Some(Binding::LayerValue(a))),
        );
        s.apply(Action::RemoveOverlay(a));
        let l = layer(&s, b);
        assert!(!l.bindings.contains_key(&'c') && !l.bindings.contains_key(&'d'));
        assert!(l.bindings.contains_key(&'a')); // its own bindings stay
    }

    // ---- Clusterize ----

    #[test]
    fn clusterize_attaches_changes_and_detaches_per_layer() {
        let (mut s, a, b) = two_layers();
        assert!(layer(&s, a).cluster.is_none());
        let on = ClusterSettings::default();
        change(&mut s, a, OverlayChange::Cluster(Some(on)));
        assert_eq!(layer(&s, a).cluster, Some(on));
        assert!(layer(&s, b).cluster.is_none()); // hooks are per layer
        let changed = ClusterSettings {
            nn: 3,
            min_size: 40.0,
            unit: SizeUnit::Microliters,
            ..on
        };
        change(&mut s, a, OverlayChange::Cluster(Some(changed)));
        assert_eq!(layer(&s, a).cluster, Some(changed));
        change(&mut s, a, OverlayChange::Cluster(None));
        assert!(layer(&s, a).cluster.is_none());
    }

    #[test]
    fn clusterize_settings_that_make_no_sense_are_ignored() {
        let (mut s, id, _) = two_layers();
        let good = ClusterSettings::default();
        change(&mut s, id, OverlayChange::Cluster(Some(good)));
        for bad in [
            ClusterSettings { nn: 0, ..good },
            ClusterSettings { nn: 4, ..good },
            ClusterSettings {
                min_size: -1.0,
                ..good
            },
            ClusterSettings {
                min_size: f64::NAN,
                ..good
            },
        ] {
            change(&mut s, id, OverlayChange::Cluster(Some(bad)));
            assert_eq!(layer(&s, id).cluster, Some(good), "{bad:?}");
        }
    }

    #[test]
    fn a_layers_clusters_are_recomputed_for_the_selection_not_the_look() {
        let (mut s, id, _) = two_layers();
        let key = s.selection_key(id);
        change(&mut s, id, OverlayChange::Opacity(0.3));
        change(
            &mut s,
            id,
            OverlayChange::ColorScale(AfniColorScale::RedsAndBlues),
        );
        change(&mut s, id, OverlayChange::Boxed(true));
        change(&mut s, id, OverlayChange::Visible(false));
        assert_eq!(s.selection_key(id), key);
        change(&mut s, id, OverlayChange::Threshold(2.5));
        assert_ne!(s.selection_key(id), key);
    }

    #[test]
    fn a_masks_key_follows_the_layers_its_rule_reads() {
        let (mut s, a, b) = two_layers();
        change(&mut s, b, OverlayChange::MaskMode(true));
        change(
            &mut s,
            b,
            OverlayChange::MaskRule(MaskRule::Expression("c".into())),
        );
        change(
            &mut s,
            b,
            OverlayChange::Bind('c', Some(Binding::LayerMask(a))),
        );
        let before = s.selection_key(b);
        change(&mut s, a, OverlayChange::Threshold(1.5));
        assert_ne!(s.selection_key(b), before);
    }

    #[test]
    fn only_restricting_to_clusters_changes_the_picture() {
        let (mut s, id, _) = two_layers();
        let key = layer(&s, id).display_key();
        let on = ClusterSettings::default();
        change(&mut s, id, OverlayChange::Cluster(Some(on)));
        assert_eq!(layer(&s, id).display_key(), key); // attached, nothing hidden
        change(
            &mut s,
            id,
            OverlayChange::Cluster(Some(ClusterSettings {
                only_clusters: true,
                ..on
            })),
        );
        assert_ne!(layer(&s, id).display_key(), key);
    }

    // ---- Memory ----

    #[test]
    fn a_replaced_underlay_and_a_removed_overlay_are_freed_but_used_ones_stay() {
        let (mut s, over, layer_id) = with_overlay();
        let first = s.controller().underlay.unwrap();
        let second = s.store.add(synthetic::phantom());
        s.apply(Action::SetUnderlay(second));
        assert!(s.store.get(first).is_none(), "the old underlay is freed");
        assert!(s.store.get(over).is_some(), "the overlay's dataset stays");
        // A dataset used as both underlay and overlay survives either change.
        s.apply(Action::AddOverlay(second));
        let third = s.store.add(synthetic::phantom());
        s.apply(Action::SetUnderlay(third));
        assert!(s.store.get(second).is_some(), "still a layer");
        s.apply(Action::RemoveOverlay(layer_id));
        assert!(
            s.store.get(over).is_none(),
            "the removed layer's dataset is freed"
        );
        // Asking for a freed dataset is refused, not a crash.
        s.apply(Action::AddOverlay(over));
        assert_eq!(s.controller().overlays.len(), 1);
    }

    #[test]
    fn a_dataset_read_by_a_rule_or_plotted_is_not_freed() {
        let (mut s, over, id) = with_overlay();
        let other = s.store.add(synthetic::tmap());
        s.apply(Action::Layer(
            id,
            OverlayChange::Bind(
                'c',
                Some(Binding::Sub {
                    dataset: other,
                    sub: 0,
                }),
            ),
        ));
        s.apply(Action::Layer(id, OverlayChange::Dataset(other)));
        assert!(
            s.store.get(over).is_none(),
            "the replaced layer dataset is freed"
        );
        assert!(s.store.get(other).is_some());
    }
}
