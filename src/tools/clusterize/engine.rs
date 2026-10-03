//! Keeping every attached layer's clusters up to date.
//!
//! Clustering a big volume takes a moment, so it runs only when what the layer
//! *selects* changes (dataset, sub-bricks, threshold, mask rule, a layer it
//! reads, the clustering settings, the underlay), never for colors or
//! opacity, and not while the mouse button is down: dragging the threshold
//! shows the previous clusters, marked stale, and on release the new ones are
//! computed **on a background thread** and swapped in when ready (the
//! interface never waits). The one thing done on the interface thread is
//! evaluating a mask layer's rule over the underlay, which needs the views'
//! caches.

use std::collections::HashMap;
use std::hash::{Hash, Hasher};
use std::sync::Arc;
use std::sync::mpsc::{self, Receiver, TryRecvError};

use afni_io::geometry::Mat44;

use super::compute::{self, ClusterOutcome, Input, Selection};
use crate::data::Dataset;
use crate::render::resample::Grid;
use crate::session::{ClusterSettings, LayerId, OverlayLayer, Session};

/// Called from the worker thread when a result is ready, to wake the
/// interface.
pub type Wake = Arc<dyn Fn() + Send + Sync>;

type Outcome = Result<ClusterOutcome, String>;

/// The clusters of one layer, as last computed.
#[derive(Debug, Clone)]
pub struct Entry {
    key: u64,
    /// The clusters, or why there are none.
    pub result: Result<Arc<ClusterOutcome>, String>,
    /// Is the layer's selection newer than these clusters?
    pub stale: bool,
}

/// A computation in flight.
#[derive(Debug)]
struct Pending {
    key: u64,
    rx: Receiver<Outcome>,
}

/// Per-layer cluster results.
#[derive(Debug)]
pub struct Engine {
    entries: HashMap<LayerId, Entry>,
    pending: HashMap<LayerId, Pending>,
    /// Compute on a worker thread (the app), or on the caller's (tests).
    background: bool,
}

impl Default for Engine {
    /// A background engine; in tests a synchronous one, so that a frame's
    /// result is there when the frame ends.
    fn default() -> Self {
        Self::new(!cfg!(test))
    }
}

/// Where a layer is on, over the whole underlay, found in the views' caches.
pub type PassedFn<'a> = &'a dyn Fn(&[OverlayLayer], LayerId) -> Option<Vec<bool>>;

/// What a worker needs, owned.
struct Job {
    source: Source,
    select: Selection,
    settings: ClusterSettings,
    under_dims: [usize; 3],
    under_ras: Mat44,
}

enum Source {
    ColorMap {
        ds: Arc<Dataset>,
        olay_sub: usize,
        thr_sub: usize,
    },
    Mask(Vec<bool>),
}

impl Engine {
    /// An engine computing on a worker thread (`true`) or the caller's.
    pub fn new(background: bool) -> Self {
        Self {
            entries: HashMap::new(),
            pending: HashMap::new(),
            background,
        }
    }

    /// The clusters of layer `id`.
    pub fn get(&self, id: LayerId) -> Option<&Entry> {
        self.entries.get(&id)
    }

    /// Is a computation running?
    pub fn busy(&self) -> bool {
        !self.pending.is_empty()
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
        ctl: usize,
        under: &Dataset,
        settled: bool,
        passed: PassedFn,
        wake: &Wake,
    ) -> bool {
        let layers = &session.controllers[ctl].overlays;
        let hooked = |id: &LayerId| layers.iter().any(|l| l.id == *id && l.cluster.is_some());
        self.entries.retain(|id, _| hooked(id));
        self.pending.retain(|id, _| hooked(id));
        self.collect();

        let mut waiting = false;
        for layer in layers {
            let Some(settings) = layer.cluster else {
                continue;
            };
            let key = key(session, ctl, layer);
            if let Some(e) = self.entries.get_mut(&layer.id)
                && e.key == key
            {
                e.stale = false;
                self.pending.remove(&layer.id);
                continue;
            }
            let in_flight = self.pending.get(&layer.id).is_some_and(|p| p.key == key);
            if in_flight || (!settled && self.entries.contains_key(&layer.id)) {
                if let Some(e) = self.entries.get_mut(&layer.id) {
                    e.stale = true;
                }
                continue;
            }
            let source = if layer.as_mask {
                match passed(layers, layer.id) {
                    Some(on) => Source::Mask(on),
                    None => {
                        waiting = true;
                        continue;
                    }
                }
            } else {
                match session.store.get(layer.dataset) {
                    Some(ds) => Source::ColorMap {
                        ds: ds.clone(),
                        olay_sub: layer.olay_sub,
                        thr_sub: layer.thr_sub,
                    },
                    None => {
                        self.entries.insert(
                            layer.id,
                            Entry {
                                key,
                                result: Err("the overlay dataset is gone".into()),
                                stale: false,
                            },
                        );
                        continue;
                    }
                }
            };
            let job = Job {
                source,
                select: selection(layer),
                settings,
                under_dims: under.dims,
                under_ras: under.ijk_to_ras,
            };
            if self.background {
                let (tx, rx) = mpsc::channel();
                let wake = wake.clone();
                std::thread::spawn(move || {
                    let _ = tx.send(run_job(job));
                    wake();
                });
                self.pending.insert(layer.id, Pending { key, rx });
                if let Some(e) = self.entries.get_mut(&layer.id) {
                    e.stale = true;
                }
            } else {
                self.entries.insert(
                    layer.id,
                    Entry {
                        key,
                        result: run_job(job).map(Arc::new),
                        stale: false,
                    },
                );
            }
        }
        waiting
    }

    /// Take in the results that have arrived.
    fn collect(&mut self) {
        let ids: Vec<LayerId> = self.pending.keys().copied().collect();
        for id in ids {
            let Some(p) = self.pending.get(&id) else {
                continue;
            };
            match p.rx.try_recv() {
                Ok(result) => {
                    let key = p.key;
                    self.pending.remove(&id);
                    self.entries.insert(
                        id,
                        Entry {
                            key,
                            result: result.map(Arc::new),
                            stale: false,
                        },
                    );
                }
                Err(TryRecvError::Empty) => {}
                Err(TryRecvError::Disconnected) => {
                    self.pending.remove(&id);
                }
            }
        }
    }
}

fn selection(layer: &OverlayLayer) -> Selection {
    Selection {
        signed: layer.signed,
        threshold: layer.threshold,
    }
}

/// Everything the clusters of `layer` depend on.
fn key(session: &Session, ctl: usize, layer: &OverlayLayer) -> u64 {
    let mut h = std::collections::hash_map::DefaultHasher::new();
    session.selection_key_in(ctl, layer.id).hash(&mut h);
    session.controllers[ctl].generation.hash(&mut h);
    if let Some(c) = layer.cluster {
        // Whether the layer is restricted to its clusters does not change them.
        (c.nn, c.min_size.to_bits(), c.unit, c.bisided).hash(&mut h);
    }
    h.finish()
}

/// Do the work of one job.
fn run_job(job: Job) -> Outcome {
    let under = Grid {
        dims: job.under_dims,
        ijk_to_ras: &job.under_ras,
    };
    match &job.source {
        Source::Mask(on) => {
            compute::run(Input::Mask(on), &under, job.select, &job.settings, &under)
        }
        Source::ColorMap {
            ds,
            olay_sub,
            thr_sub,
        } => {
            let thr = ds
                .frame(*thr_sub)
                .ok_or("the Thr sub-brick cannot be read")?;
            let olay = if olay_sub == thr_sub {
                None
            } else {
                Some(
                    ds.frame(*olay_sub)
                        .ok_or("the OLay sub-brick cannot be read")?,
                )
            };
            let source = Grid {
                dims: ds.dims,
                ijk_to_ras: &ds.ijk_to_ras,
            };
            compute::run(
                Input::Values {
                    thr: &thr,
                    olay: olay.as_deref(),
                },
                &source,
                job.select,
                &job.settings,
                &under,
            )
        }
    }
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

    fn wake() -> Wake {
        Arc::new(|| {})
    }

    #[test]
    fn an_attached_layer_gets_clusters_and_a_detached_one_loses_them() {
        let (mut s, id) = session();
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        assert!(!e.update(&s, s.active, &under, true, &no_views, &wake()));
        let out = e.get(id).unwrap().result.as_ref().unwrap();
        assert!(!out.rows.is_empty(), "the t-map has clusters above 3.1");
        assert_eq!(out.survivors.len(), under.dims.iter().product::<usize>());
        s.apply(Action::Layer(id, OverlayChange::Cluster(None)));
        e.update(&s, s.active, &under, true, &no_views, &wake());
        assert!(e.get(id).is_none());
    }

    #[test]
    fn colors_and_opacity_do_not_recompute_but_the_threshold_does() {
        let (mut s, id) = session();
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        e.update(&s, s.active, &under, true, &no_views, &wake());
        let first = e.get(id).unwrap().result.as_ref().unwrap().clone();
        s.apply(Action::Layer(id, OverlayChange::Opacity(0.4)));
        e.update(&s, s.active, &under, true, &no_views, &wake());
        assert!(Arc::ptr_eq(
            &first,
            e.get(id).unwrap().result.as_ref().unwrap()
        ));
        s.apply(Action::Layer(id, OverlayChange::Threshold(4.5)));
        e.update(&s, s.active, &under, true, &no_views, &wake());
        let again = e.get(id).unwrap().result.as_ref().unwrap();
        assert!(!Arc::ptr_eq(&first, again));
        assert!(again.total_voxels < first.total_voxels);
    }

    #[test]
    fn while_the_mouse_is_down_the_old_clusters_stay_and_are_stale() {
        let (mut s, id) = session();
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        e.update(&s, s.active, &under, true, &no_views, &wake());
        let first = e.get(id).unwrap().result.as_ref().unwrap().clone();
        s.apply(Action::Layer(id, OverlayChange::Threshold(4.5)));
        e.update(&s, s.active, &under, false, &no_views, &wake());
        let held = e.get(id).unwrap();
        assert!(held.stale);
        assert!(Arc::ptr_eq(&first, held.result.as_ref().unwrap()));
        e.update(&s, s.active, &under, true, &no_views, &wake()); // released
        assert!(!e.get(id).unwrap().stale);
    }

    #[test]
    fn restricting_a_layer_hands_its_survivors_to_the_views() {
        let (mut s, id) = session();
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        e.update(&s, s.active, &under, true, &no_views, &wake());
        assert!(e.keep(s.layer(id).unwrap()).is_none());
        let on = ClusterSettings {
            only_clusters: true,
            min_size: 1.0,
            ..ClusterSettings::default()
        };
        s.apply(Action::Layer(id, OverlayChange::Cluster(Some(on))));
        e.update(&s, s.active, &under, true, &no_views, &wake());
        let keep = e.keep(s.layer(id).unwrap()).unwrap();
        assert!(keep.iter().any(|k| *k));
    }

    #[test]
    fn a_mask_layer_waits_for_the_views_then_clusters_where_it_is_on() {
        let (mut s, id) = session();
        s.apply(Action::Layer(id, OverlayChange::MaskMode(true)));
        let under = s.underlay().unwrap().clone();
        let mut e = Engine::default();
        assert!(
            e.update(&s, s.active, &under, true, &no_views, &wake()),
            "waiting for frames"
        );
        assert!(e.get(id).is_none());
        let n = under.dims.iter().product::<usize>();
        let on: Vec<bool> = (0..n).map(|v| v % 150 < 3 && v < 150 * 3).collect();
        let supply = |_: &[OverlayLayer], _: LayerId| Some(on.clone());
        assert!(!e.update(&s, s.active, &under, true, &supply, &wake()));
        let out = e.get(id).unwrap().result.as_ref().unwrap();
        assert!(!out.has_values);
        assert!(out.total_voxels > 0);
    }

    /// Run `update` until the background work is done (or fail after 20 s).
    fn settle(e: &mut Engine, s: &Session, under: &Dataset, wake: &Wake) {
        let start = std::time::Instant::now();
        loop {
            e.update(s, s.active, under, true, &no_views, wake);
            if !e.busy() {
                return;
            }
            assert!(start.elapsed().as_secs() < 20, "the worker never finished");
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    #[test]
    fn a_background_engine_returns_at_once_and_delivers_later_with_a_wake() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let (s, id) = session();
        let under = s.underlay().unwrap().clone();
        let woken = Arc::new(AtomicUsize::new(0));
        let w: Wake = {
            let woken = woken.clone();
            Arc::new(move || {
                woken.fetch_add(1, Ordering::SeqCst);
            })
        };
        let mut e = Engine::new(true);
        e.update(&s, s.active, &under, true, &no_views, &w);
        // Nothing yet to show unless the worker was very quick.
        assert!(e.busy() || e.get(id).is_some());
        settle(&mut e, &s, &under, &w);
        let out = e.get(id).unwrap().result.as_ref().unwrap();
        assert!(!out.rows.is_empty());
        assert!(woken.load(Ordering::SeqCst) >= 1);
    }

    #[test]
    fn a_background_result_equals_the_synchronous_one() {
        let (s, id) = session();
        let under = s.underlay().unwrap().clone();
        let w = wake();
        let mut sync = Engine::new(false);
        sync.update(&s, s.active, &under, true, &no_views, &w);
        let mut bg = Engine::new(true);
        settle(&mut bg, &s, &under, &w);
        let rows = |e: &Engine| e.get(id).unwrap().result.as_ref().unwrap().rows.clone();
        assert_eq!(rows(&sync), rows(&bg));
    }

    #[test]
    fn a_threshold_changed_during_a_computation_wins() {
        let (mut s, id) = session();
        let under = s.underlay().unwrap().clone();
        let w = wake();
        let mut e = Engine::new(true);
        e.update(&s, s.active, &under, true, &no_views, &w);
        s.apply(Action::Layer(id, OverlayChange::Threshold(4.5)));
        settle(&mut e, &s, &under, &w);
        settle(&mut e, &s, &under, &w);
        let mut sync = Engine::new(false);
        sync.update(&s, s.active, &under, true, &no_views, &w);
        let total = |e: &Engine| e.get(id).unwrap().result.as_ref().unwrap().total_voxels;
        assert_eq!(total(&e), total(&sync));
        assert!(!e.get(id).unwrap().stale);
    }
}
