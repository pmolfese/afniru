//! Keeping every attached layer's clusters up to date.
//!
//! Clustering a big volume takes a moment, so it runs only when what the layer
//! *selects* changes (dataset, sub-bricks, threshold, mask rule, a layer it
//! reads, the clustering settings, the underlay), never for colors or
//! opacity, and not while the mouse button is down: dragging the threshold
//! shows the previous clusters, marked stale, and the new ones appear on
//! release.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;

use super::compute::{self, ClusterOutcome, Input, Selection};
use crate::data::Dataset;
use crate::render::resample::Grid;
use crate::session::{LayerId, OverlayLayer, Session};

/// The clusters of one layer, as last computed.
#[derive(Debug, Clone)]
pub struct Entry {
    key: u64,
    /// The clusters, or why there are none.
    pub result: Result<Arc<ClusterOutcome>, String>,
    /// Is the layer's selection newer than these clusters?
    pub stale: bool,
}

/// Per-layer cluster results.
#[derive(Debug, Default)]
pub struct Engine {
    entries: HashMap<LayerId, Entry>,
}

/// The layers a layer's clusters depend on, found in the views' caches. Gives
/// where `id` is on, over the whole underlay.
pub type PassedFn<'a> = &'a dyn Fn(&[OverlayLayer], LayerId) -> Option<Vec<bool>>;

impl Engine {
    /// The clusters of layer `id`.
    pub fn get(&self, id: LayerId) -> Option<&Entry> {
        self.entries.get(&id)
    }

    /// The voxels of the underlay to keep for layer `id`, if it is attached
    /// and restricted to its clusters.
    pub fn keep(&self, layer: &OverlayLayer) -> Option<Arc<Vec<bool>>> {
        if !layer.cluster?.only_clusters {
            return None;
        }
        self.entries
            .get(&layer.id)?
            .result
            .as_ref()
            .ok()
            .map(|o| o.survivors.clone())
    }

    /// Bring every attached layer's clusters up to date. With `settled` false
    /// (the mouse is down) a layer that already has clusters keeps them,
    /// marked stale. Returns `true` when a layer is still waiting for data
    /// the views have not built yet, so the caller should try again.
    pub fn update(
        &mut self,
        session: &Session,
        under: &Dataset,
        settled: bool,
        passed: PassedFn,
    ) -> bool {
        let layers = &session.controller().overlays;
        self.entries
            .retain(|id, _| layers.iter().any(|l| l.id == *id && l.cluster.is_some()));
        let mut waiting = false;
        for layer in layers {
            let Some(settings) = layer.cluster else {
                continue;
            };
            let key = key(session, layer);
            match self.entries.get_mut(&layer.id) {
                Some(e) if e.key == key => {
                    e.stale = false;
                    continue;
                }
                Some(e) if !settled => {
                    e.stale = true;
                    continue;
                }
                _ => {}
            }
            let result = if layer.as_mask {
                match passed(layers, layer.id) {
                    Some(on) => compute::run(
                        Input::Mask(&on),
                        &grid(under),
                        selection(layer),
                        &settings,
                        &grid(under),
                    ),
                    None => {
                        waiting = true;
                        continue;
                    }
                }
            } else {
                cluster_color_map(session, layer, under)
            };
            self.entries.insert(
                layer.id,
                Entry {
                    key,
                    result: result.map(Arc::new),
                    stale: false,
                },
            );
        }
        waiting
    }
}

fn grid(ds: &Dataset) -> Grid<'_> {
    Grid {
        dims: ds.dims,
        ijk_to_ras: &ds.ijk_to_ras,
    }
}

fn selection(layer: &OverlayLayer) -> Selection {
    Selection {
        signed: layer.signed,
        threshold: layer.threshold,
    }
}

/// Everything the clusters of `layer` depend on.
fn key(session: &Session, layer: &OverlayLayer) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    session.selection_key(layer.id).hash(&mut h);
    session.generation.hash(&mut h);
    if let Some(c) = layer.cluster {
        // Whether the layer is restricted to its clusters does not change them.
        (c.nn, c.min_size.to_bits(), c.unit, c.bisided).hash(&mut h);
    }
    h.finish()
}

/// Cluster a color-map layer on its own dataset's grid.
fn cluster_color_map(
    session: &Session,
    layer: &OverlayLayer,
    under: &Dataset,
) -> Result<ClusterOutcome, String> {
    let ds = session
        .store
        .get(layer.dataset)
        .ok_or("the overlay dataset is gone")?;
    let thr = ds
        .frame(layer.thr_sub)
        .ok_or("the Thr sub-brick cannot be read")?;
    let olay = if layer.olay_sub == layer.thr_sub {
        None
    } else {
        Some(
            ds.frame(layer.olay_sub)
                .ok_or("the OLay sub-brick cannot be read")?,
        )
    };
    compute::run(
        Input::Values {
            thr: &thr,
            olay: olay.as_deref(),
        },
        &grid(ds),
        selection(layer),
        &layer.cluster.unwrap_or_default(),
        &grid(under),
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::synthetic;
    use crate::session::{Action, ClusterSettings, OverlayChange};

    fn session() -> (Session, LayerId) {
        let mut s = Session::new();
        s.add_dataset(synthetic::phantom());
        let over = s.store.add(synthetic::tmap());
        s.apply(Action::AddOverlay(over));
        let id = s.controller().overlays[0].id;
        s.apply(Action::Layer(id, OverlayChange::Threshold(3.1)));
        s.apply(Action::Layer(
            id,
            OverlayChange::Cluster(Some(ClusterSettings {
                min_size: 1.0,
                ..ClusterSettings::default()
            })),
        ));
        (s, id)
    }

    fn no_views(_: &[OverlayLayer], _: LayerId) -> Option<Vec<bool>> {
        None
    }

    #[test]
    fn an_attached_layer_gets_clusters_and_a_detached_one_loses_them() {
        let (mut s, id) = session();
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        assert!(!e.update(&s, &under, true, &no_views));
        let out = e.get(id).unwrap().result.as_ref().unwrap();
        assert!(!out.rows.is_empty(), "the t-map has clusters above 3.1");
        assert_eq!(out.survivors.len(), under.dims.iter().product::<usize>());
        s.apply(Action::Layer(id, OverlayChange::Cluster(None)));
        e.update(&s, &under, true, &no_views);
        assert!(e.get(id).is_none());
    }

    #[test]
    fn colors_and_opacity_do_not_recompute_but_the_threshold_does() {
        let (mut s, id) = session();
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        e.update(&s, &under, true, &no_views);
        let first = e.get(id).unwrap().result.as_ref().unwrap().clone();
        s.apply(Action::Layer(id, OverlayChange::Opacity(0.4)));
        e.update(&s, &under, true, &no_views);
        assert!(Arc::ptr_eq(
            &first,
            e.get(id).unwrap().result.as_ref().unwrap()
        ));
        s.apply(Action::Layer(id, OverlayChange::Threshold(4.5)));
        e.update(&s, &under, true, &no_views);
        let again = e.get(id).unwrap().result.as_ref().unwrap();
        assert!(!Arc::ptr_eq(&first, again));
        assert!(again.total_voxels < first.total_voxels);
    }

    #[test]
    fn while_the_mouse_is_down_the_old_clusters_stay_and_are_stale() {
        let (mut s, id) = session();
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        e.update(&s, &under, true, &no_views);
        let first = e.get(id).unwrap().result.as_ref().unwrap().clone();
        s.apply(Action::Layer(id, OverlayChange::Threshold(4.5)));
        e.update(&s, &under, false, &no_views);
        let held = e.get(id).unwrap();
        assert!(held.stale);
        assert!(Arc::ptr_eq(&first, held.result.as_ref().unwrap()));
        e.update(&s, &under, true, &no_views); // released
        assert!(!e.get(id).unwrap().stale);
    }

    #[test]
    fn restricting_a_layer_hands_its_survivors_to_the_views() {
        let (mut s, id) = session();
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        e.update(&s, &under, true, &no_views);
        assert!(e.keep(s.layer(id).unwrap()).is_none());
        let on = ClusterSettings {
            only_clusters: true,
            min_size: 1.0,
            ..ClusterSettings::default()
        };
        s.apply(Action::Layer(id, OverlayChange::Cluster(Some(on))));
        e.update(&s, &under, true, &no_views);
        let keep = e.keep(s.layer(id).unwrap()).unwrap();
        assert!(keep.iter().any(|k| *k));
    }

    #[test]
    fn a_mask_layer_waits_for_the_views_then_clusters_where_it_is_on() {
        let (mut s, id) = session();
        s.apply(Action::Layer(id, OverlayChange::MaskMode(true)));
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        assert!(e.update(&s, &under, true, &no_views), "waiting for frames");
        assert!(e.get(id).is_none());
        let n = under.dims.iter().product::<usize>();
        let on: Vec<bool> = (0..n).map(|v| v % 150 < 3 && v < 150 * 3).collect();
        let supply = |_: &[OverlayLayer], _: LayerId| Some(on.clone());
        assert!(!e.update(&s, &under, true, &supply));
        let out = e.get(id).unwrap().result.as_ref().unwrap();
        assert!(!out.has_values);
        assert!(out.total_voxels > 0);
    }
}
