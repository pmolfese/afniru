//! Evaluating the whole overlay stack at a list of voxels.
//!
//! A layer is either a **color map** (values become colors through the color
//! scale and the threshold) or a **mask** (every voxel is on or off, and "on"
//! is one color). A mask is on where its **rule** says so: the layer's own
//! threshold, or a `3dcalc` expression whose letters are bound to sub-bricks,
//! coordinates, or *other layers* (where they are drawn, or their values).
//! Layers may read each other in any order, hidden ones included, so
//! "where A is", "where B is" and "where both are" are three layers.
//!
//! Voxels are given as `[i, j, k]` triples of the underlay grid, so the same
//! code serves a whole slice and a single crosshair voxel.

use std::collections::HashMap;

use afni_core::calc::Expr;
use afni_core::color::Rgba;
use afni_core::threshold::Threshold;

use super::mask::evaluate_rule;
use super::overlay::{OverlayFrames, colors_for_values};
use crate::data::Dataset;
use crate::geom::coords::ijk_to_ras;
use crate::session::overlay::{Binding, Coord, LayerId, MaskRule, OverlayLayer};
use crate::session::store::DatasetId;

/// One layer with its data on the underlay grid.
pub struct LayerInput<'a> {
    /// What to draw and how.
    pub layer: &'a OverlayLayer,
    /// Its OLay and Thr values on the underlay grid.
    pub frames: &'a OverlayFrames,
}

/// Everything the evaluation reads.
pub struct Context<'a> {
    /// The underlay: its grid sets the voxel numbering and the coordinates.
    pub under: &'a Dataset,
    /// The layers, bottom first.
    pub layers: &'a [LayerInput<'a>],
    /// A dataset sub-brick on the underlay grid, for `Binding::Sub`.
    pub sub_frame: &'a dyn Fn(DatasetId, usize) -> Option<&'a [f32]>,
    /// Hide the voxels outside a layer's surviving clusters (for layers
    /// restricted to them)? Off when clustering the layer itself.
    pub apply_keep: bool,
}

/// What one layer looks like at the voxels.
#[derive(Debug, Clone, PartialEq)]
pub struct LayerResult {
    /// The layer.
    pub id: LayerId,
    /// One color per voxel (transparent where nothing is drawn).
    pub colors: Vec<Rgba>,
    /// Is the voxel "on": passing the threshold, or inside the mask.
    pub passed: Vec<bool>,
    /// Why the layer shows nothing, when its rule cannot be evaluated: a
    /// parse error, an unbound letter, a reference to a missing layer.
    pub problem: Option<String>,
}

/// Evaluate every layer at `voxels`, in layer order.
pub fn evaluate(cx: &Context, voxels: &[[usize; 3]]) -> Vec<LayerResult> {
    let mut done: HashMap<LayerId, LayerResult> = HashMap::new();
    for input in cx.layers {
        evaluate_layer(cx, voxels, input.layer.id, &mut done, &mut Vec::new());
    }
    cx.layers
        .iter()
        .filter_map(|l| done.remove(&l.layer.id))
        .collect()
}

/// Evaluate one layer (once), evaluating the layers it reads first.
/// `active` guards against a cycle that should have been refused earlier.
fn evaluate_layer<'r>(
    cx: &Context,
    voxels: &[[usize; 3]],
    id: LayerId,
    done: &'r mut HashMap<LayerId, LayerResult>,
    active: &mut Vec<LayerId>,
) -> Option<&'r LayerResult> {
    if done.contains_key(&id) {
        return done.get(&id);
    }
    let input = cx.layers.iter().find(|l| l.layer.id == id)?;
    if active.contains(&id) {
        return None;
    }
    active.push(id);
    let mut result = layer_result(cx, voxels, input, done, active);
    active.pop();
    if cx.apply_keep
        && let Some(keep) = &input.frames.keep
    {
        restrict(&mut result, keep, cx.under, voxels);
    }
    done.insert(id, result);
    done.get(&id)
}

/// Hide the voxels that are not in `keep` (a flag per voxel of the underlay).
fn restrict(result: &mut LayerResult, keep: &[bool], under: &Dataset, voxels: &[[usize; 3]]) {
    let [nx, ny, _] = under.dims;
    for (n, [i, j, k]) in voxels.iter().enumerate() {
        if !keep.get(i + nx * (j + ny * k)).copied().unwrap_or(false) {
            result.passed[n] = false;
            result.colors[n] = Rgba::TRANSPARENT;
        }
    }
}

fn nothing(id: LayerId, n: usize, problem: impl Into<String>) -> LayerResult {
    LayerResult {
        id,
        colors: vec![Rgba::TRANSPARENT; n],
        passed: vec![false; n],
        problem: Some(problem.into()),
    }
}

/// Values of a frame at the voxels; NaN (outside the field of view) reads as 0.
fn gather(frame: &[f32], under: &Dataset, voxels: &[[usize; 3]]) -> Vec<f64> {
    let [nx, ny, _] = under.dims;
    voxels
        .iter()
        .map(|[i, j, k]| {
            let v = f64::from(frame.get(i + nx * (j + ny * k)).copied().unwrap_or(0.0));
            if v.is_nan() { 0.0 } else { v }
        })
        .collect()
}

fn layer_result(
    cx: &Context,
    voxels: &[[usize; 3]],
    input: &LayerInput,
    done: &mut HashMap<LayerId, LayerResult>,
    active: &mut Vec<LayerId>,
) -> LayerResult {
    let (layer, n) = (input.layer, voxels.len());
    let olay = gather(&input.frames.olay, cx.under, voxels);
    let thr = gather(&input.frames.thr, cx.under, voxels);

    if !layer.as_mask {
        return match colors_for_values(layer, input.frames.auto_range, &olay, &thr) {
            Ok((colors, passed)) => LayerResult {
                id: layer.id,
                colors,
                passed,
                problem: None,
            },
            Err(e) => nothing(layer.id, n, e.to_string()),
        };
    }

    let passed = match &layer.mask.rule {
        MaskRule::Threshold => {
            let t = if layer.signed {
                Threshold::AbsoluteAbove(layer.threshold)
            } else {
                Threshold::Above(layer.threshold)
            };
            // A zero is never inside, as AFNI never colors zeros.
            thr.iter().map(|v| *v != 0.0 && t.passes(*v)).collect()
        }
        MaskRule::Expression(text) => {
            let expr = match Expr::parse(text) {
                Ok(e) => e,
                Err(e) => return nothing(layer.id, n, e.to_string()),
            };
            let mut columns = Vec::new();
            for letter in expr.variables() {
                let column = match layer.binding_for(letter) {
                    None => {
                        return nothing(layer.id, n, format!("{letter} is not bound to anything"));
                    }
                    Some(Binding::Olay) => olay.clone(),
                    Some(Binding::Thr) => thr.clone(),
                    Some(Binding::Sub { dataset, sub }) => match (cx.sub_frame)(dataset, sub) {
                        Some(f) => gather(f, cx.under, voxels),
                        None => {
                            return nothing(
                                layer.id,
                                n,
                                format!("{letter}: dataset not available"),
                            );
                        }
                    },
                    Some(Binding::Coord(c)) => coordinates(cx.under, voxels, c),
                    Some(Binding::LayerMask(other)) => {
                        match evaluate_layer(cx, voxels, other, done, active) {
                            Some(r) => r.passed.iter().map(|p| f64::from(u8::from(*p))).collect(),
                            None => {
                                return nothing(
                                    layer.id,
                                    n,
                                    format!("{letter}: layer {} is not available", other.0),
                                );
                            }
                        }
                    }
                    Some(Binding::LayerValue(other)) => {
                        match cx.layers.iter().find(|l| l.layer.id == other) {
                            Some(o) => gather(&o.frames.olay, cx.under, voxels),
                            None => {
                                return nothing(
                                    layer.id,
                                    n,
                                    format!("{letter}: layer {} is not available", other.0),
                                );
                            }
                        }
                    }
                };
                columns.push((letter, column));
            }
            evaluate_rule(&expr, &columns, n)
        }
    };

    let [r, g, b] = layer.mask.color;
    let on = Rgba::from_u8(r, g, b, 255).with_alpha(layer.opacity);
    let colors = passed
        .iter()
        .map(|p| if *p { on } else { Rgba::TRANSPARENT })
        .collect();
    LayerResult {
        id: layer.id,
        colors,
        passed,
        problem: None,
    }
}

/// A coordinate or index at each voxel, with `3dcalc`'s meanings: x, y, z in
/// mm in DICOM (RAI) order, and the voxel indices i, j, k.
fn coordinates(under: &Dataset, voxels: &[[usize; 3]], c: Coord) -> Vec<f64> {
    voxels
        .iter()
        .map(|&v| match c {
            Coord::I => v[0] as f64,
            Coord::J => v[1] as f64,
            Coord::K => v[2] as f64,
            Coord::X => -ijk_to_ras(&under.ijk_to_ras, v)[0],
            Coord::Y => -ijk_to_ras(&under.ijk_to_ras, v)[1],
            Coord::Z => ijk_to_ras(&under.ijk_to_ras, v)[2],
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use afni_core::afni_colors::AfniColorScale;

    use super::*;
    use crate::data::synthetic;
    use crate::session::overlay::MaskSettings;

    /// A 1-voxel-wide "volume" is enough: the evaluation only needs the
    /// grid for numbering and coordinates, so use the phantom and a few voxels.
    fn under() -> Dataset {
        synthetic::phantom()
    }

    fn frames(under: &Dataset, f: impl Fn(usize) -> f32) -> OverlayFrames {
        let n = under.dims.iter().product::<usize>();
        let v: Arc<Vec<f32>> = Arc::new((0..n).map(&f).collect());
        OverlayFrames {
            olay: v.clone(),
            thr: v,
            auto_range: 5.0,
            thr_max: 5.0,
            keep: None,
        }
    }

    fn layer(id: u64) -> OverlayLayer {
        let mut l = OverlayLayer::new(DatasetId(1), AfniColorScale::RedsAndBlues);
        l.id = LayerId(id);
        l
    }

    fn mask(mut l: OverlayLayer, rule: MaskRule) -> OverlayLayer {
        l.as_mask = true;
        l.mask = MaskSettings {
            rule,
            color: [10, 20, 30],
        };
        l
    }

    /// Voxels (0,0,0), (1,0,0), (2,0,0), (3,0,0): indices 0..4.
    fn row() -> Vec<[usize; 3]> {
        (0..4).map(|i| [i, 0, 0]).collect()
    }

    fn run(under: &Dataset, inputs: &[LayerInput]) -> Vec<LayerResult> {
        let none = |_: DatasetId, _: usize| None;
        evaluate(
            &Context {
                under,
                layers: inputs,
                sub_frame: &none,
                apply_keep: true,
            },
            &row(),
        )
    }

    #[test]
    fn a_color_map_layer_draws_colors_above_the_threshold() {
        let u = under();
        let f = frames(&u, |n| [0.0, 1.0, 3.0, 5.0].get(n).copied().unwrap_or(0.0));
        let l = OverlayLayer {
            threshold: 2.0,
            ..layer(1)
        };
        let r = &run(
            &u,
            &[LayerInput {
                layer: &l,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [false, false, true, true]);
        assert!(r.colors[2].a > 0.0 && r.colors[0].a == 0.0);
        assert!(r.problem.is_none());
    }

    #[test]
    fn a_threshold_mask_is_one_color_where_the_threshold_passes() {
        let u = under();
        let f = frames(&u, |n| [0.0, 1.0, 3.0, 5.0].get(n).copied().unwrap_or(0.0));
        let l = mask(
            OverlayLayer {
                threshold: 2.0,
                ..layer(1)
            },
            MaskRule::Threshold,
        );
        let r = &run(
            &u,
            &[LayerInput {
                layer: &l,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [false, false, true, true]);
        let on = Rgba::from_u8(10, 20, 30, 255);
        assert_eq!(r.colors[2], on);
        assert_eq!(r.colors[3], on); // 3 and 5 are the same color: no longer a gradient
        assert_eq!(r.colors[1], Rgba::TRANSPARENT);
    }

    #[test]
    fn a_threshold_mask_never_counts_zeros_even_at_threshold_zero() {
        let u = under();
        let f = frames(&u, |n| {
            [0.0, -1.0, 3.0, f32::NAN].get(n).copied().unwrap_or(0.0)
        });
        let l = mask(
            OverlayLayer {
                threshold: 0.0,
                ..layer(1)
            },
            MaskRule::Threshold,
        );
        let r = &run(
            &u,
            &[LayerInput {
                layer: &l,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [false, true, true, false]); // ±: |-1| counts; 0 and NaN do not
        let pos = OverlayLayer {
            signed: false,
            ..l.clone()
        };
        let r = &run(
            &u,
            &[LayerInput {
                layer: &pos,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [false, false, true, false]);
    }

    #[test]
    fn mask_opacity_scales_the_on_color() {
        let u = under();
        let f = frames(&u, |_| 4.0);
        let l = mask(
            OverlayLayer {
                threshold: 1.0,
                opacity: 0.5,
                ..layer(1)
            },
            MaskRule::Threshold,
        );
        let r = &run(
            &u,
            &[LayerInput {
                layer: &l,
                frames: &f,
            }],
        )[0];
        assert!((r.colors[0].a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn an_expression_rule_uses_the_layers_own_olay_and_thr() {
        let u = under();
        let f = frames(&u, |n| (n as f32) * 2.0); // 0, 2, 4, 6
        let l = mask(layer(1), MaskRule::Expression("step(a-3)".into()));
        let r = &run(
            &u,
            &[LayerInput {
                layer: &l,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [false, false, true, true]);
        let within = mask(layer(1), MaskRule::Expression("within(a,2,4)".into()));
        let r = &run(
            &u,
            &[LayerInput {
                layer: &within,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [false, true, true, false]);
    }

    #[test]
    fn where_a_is_where_b_is_and_where_both_are() {
        let u = under();
        let fa = frames(&u, |n| [5.0, 5.0, 0.0, 0.0].get(n).copied().unwrap_or(0.0));
        let fb = frames(&u, |n| [5.0, 0.0, 5.0, 0.0].get(n).copied().unwrap_or(0.0));
        let fc = frames(&u, |_| 0.0);
        let a = mask(
            OverlayLayer {
                threshold: 2.0,
                ..layer(1)
            },
            MaskRule::Threshold,
        );
        let b = mask(
            OverlayLayer {
                threshold: 2.0,
                ..layer(2)
            },
            MaskRule::Threshold,
        );
        let mut both = mask(layer(3), MaskRule::Expression("a*b".into()));
        both.bindings.insert('a', Binding::LayerMask(LayerId(1)));
        both.bindings.insert('b', Binding::LayerMask(LayerId(2)));
        let r = run(
            &u,
            &[
                LayerInput {
                    layer: &a,
                    frames: &fa,
                },
                LayerInput {
                    layer: &b,
                    frames: &fb,
                },
                LayerInput {
                    layer: &both,
                    frames: &fc,
                },
            ],
        );
        assert_eq!(r[0].passed, [true, true, false, false]); // where A is
        assert_eq!(r[1].passed, [true, false, true, false]); // where B is
        assert_eq!(r[2].passed, [true, false, false, false]); // where both are
    }

    #[test]
    fn a_hidden_layer_still_provides_its_mask_to_others() {
        let u = under();
        let fa = frames(&u, |n| [5.0, 0.0, 5.0, 0.0].get(n).copied().unwrap_or(0.0));
        let fc = frames(&u, |_| 0.0);
        let mut a = mask(
            OverlayLayer {
                threshold: 2.0,
                ..layer(1)
            },
            MaskRule::Threshold,
        );
        a.visible = false;
        let mut not_a = mask(layer(2), MaskRule::Expression("not(a)".into()));
        not_a.bindings.insert('a', Binding::LayerMask(LayerId(1)));
        let r = run(
            &u,
            &[
                LayerInput {
                    layer: &a,
                    frames: &fa,
                },
                LayerInput {
                    layer: &not_a,
                    frames: &fc,
                },
            ],
        );
        assert_eq!(r[1].passed, [false, true, false, true]);
    }

    #[test]
    fn a_layer_can_read_a_layer_above_it_in_the_stack() {
        let u = under();
        let fa = frames(&u, |_| 0.0);
        let fb = frames(&u, |n| (n as f32) * 3.0);
        let mut reader = mask(layer(1), MaskRule::Expression("a".into()));
        reader.bindings.insert('a', Binding::LayerMask(LayerId(2)));
        let source = mask(
            OverlayLayer {
                threshold: 2.0,
                ..layer(2)
            },
            MaskRule::Threshold,
        );
        let r = run(
            &u,
            &[
                LayerInput {
                    layer: &reader,
                    frames: &fa,
                },
                LayerInput {
                    layer: &source,
                    frames: &fb,
                },
            ],
        );
        assert_eq!(r[0].passed, r[1].passed);
        assert_eq!(r[0].passed, [false, true, true, true]);
    }

    #[test]
    fn a_rule_can_read_another_layers_values_not_just_its_mask() {
        let u = under();
        let fa = frames(&u, |_| 0.0);
        let fb = frames(&u, |n| n as f32);
        let mut reader = mask(layer(1), MaskRule::Expression("step(a-1.5)".into()));
        reader.bindings.insert('a', Binding::LayerValue(LayerId(2)));
        let other = layer(2);
        let r = run(
            &u,
            &[
                LayerInput {
                    layer: &reader,
                    frames: &fa,
                },
                LayerInput {
                    layer: &other,
                    frames: &fb,
                },
            ],
        );
        assert_eq!(r[0].passed, [false, false, true, true]);
    }

    #[test]
    fn coordinates_and_indices_are_built_in() {
        let u = under(); // phantom: RAS x = 75 - i, so DICOM x = i - 75
        let f = frames(&u, |_| 0.0);
        let left = mask(layer(1), MaskRule::Expression("step(x+73.5)".into())); // i - 75 > -73.5
        let r = &run(
            &u,
            &[LayerInput {
                layer: &left,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [false, false, true, true]); // i = 2, 3
        let index = mask(layer(1), MaskRule::Expression("equals(i,1)".into()));
        let r = &run(
            &u,
            &[LayerInput {
                layer: &index,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [false, true, false, false]);
        let z = mask(layer(1), MaskRule::Expression("step(z)".into())); // z = k - 75 = -75 here
        assert!(
            run(
                &u,
                &[LayerInput {
                    layer: &z,
                    frames: &f
                }]
            )[0]
            .passed
            .iter()
            .all(|p| !p)
        );
    }

    #[test]
    fn a_letter_can_be_a_sub_brick_of_any_dataset() {
        let u = under();
        let f = frames(&u, |_| 0.0);
        let other: Vec<f32> = (0..u.dims.iter().product::<usize>())
            .map(|n| n as f32)
            .collect();
        let mut l = mask(layer(1), MaskRule::Expression("step(c-1.5)".into()));
        l.bindings.insert(
            'c',
            Binding::Sub {
                dataset: DatasetId(7),
                sub: 0,
            },
        );
        let lookup =
            |d: DatasetId, s: usize| (d == DatasetId(7) && s == 0).then_some(other.as_slice());
        let r = evaluate(
            &Context {
                under: &u,
                layers: &[LayerInput {
                    layer: &l,
                    frames: &f,
                }],
                sub_frame: &lookup,
                apply_keep: true,
            },
            &row(),
        );
        assert_eq!(r[0].passed, [false, false, true, true]);
        // The same layer when the dataset is gone shows nothing and says why.
        let none = |_: DatasetId, _: usize| None;
        let r = evaluate(
            &Context {
                under: &u,
                layers: &[LayerInput {
                    layer: &l,
                    frames: &f,
                }],
                sub_frame: &none,
                apply_keep: true,
            },
            &row(),
        );
        assert!(r[0].problem.as_deref().unwrap().contains("not available"));
        assert!(r[0].passed.iter().all(|p| !p));
    }

    #[test]
    fn problems_are_reported_and_draw_nothing() {
        let u = under();
        let f = frames(&u, |_| 5.0);
        let problem = |rule: &str| {
            let l = mask(layer(1), MaskRule::Expression(rule.into()));
            let r = run(
                &u,
                &[LayerInput {
                    layer: &l,
                    frames: &f,
                }],
            )
            .remove(0);
            assert!(r.passed.iter().all(|p| !p) && r.colors.iter().all(|c| c.a == 0.0));
            r.problem.unwrap()
        };
        assert!(problem("step(").contains("end"));
        assert!(problem("a%3").contains("cannot interpret"));
        assert!(problem("step(c)").contains("c is not bound"));
        assert!(problem("gran(1,2)").contains("not implemented"));
        assert!(problem("").contains("empty"));
    }

    #[test]
    fn a_reference_to_a_missing_layer_is_a_problem_not_a_crash() {
        let u = under();
        let f = frames(&u, |_| 5.0);
        let mut l = mask(layer(1), MaskRule::Expression("a".into()));
        l.bindings.insert('a', Binding::LayerMask(LayerId(42)));
        let r = &run(
            &u,
            &[LayerInput {
                layer: &l,
                frames: &f,
            }],
        )[0];
        assert!(r.problem.as_deref().unwrap().contains("layer 42"));
    }

    #[test]
    fn a_cycle_that_slipped_through_draws_nothing_instead_of_looping() {
        let u = under();
        let f = frames(&u, |_| 5.0);
        let mut a = mask(layer(1), MaskRule::Expression("a".into()));
        let mut b = mask(layer(2), MaskRule::Expression("a".into()));
        a.bindings.insert('a', Binding::LayerMask(LayerId(2)));
        b.bindings.insert('a', Binding::LayerMask(LayerId(1)));
        let r = run(
            &u,
            &[
                LayerInput {
                    layer: &a,
                    frames: &f,
                },
                LayerInput {
                    layer: &b,
                    frames: &f,
                },
            ],
        );
        assert!(r.iter().all(|x| x.passed.iter().all(|p| !p)));
    }

    #[test]
    fn nan_data_is_off_for_masks_not_on() {
        // AFNI's step(NaN) is 1; the overlay binds a missing voxel as 0.
        let u = under();
        let f = frames(&u, |n| if n == 1 { f32::NAN } else { 5.0 });
        let l = mask(layer(1), MaskRule::Expression("step(a)".into()));
        let r = &run(
            &u,
            &[LayerInput {
                layer: &l,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [true, false, true, true]);
    }

    #[test]
    fn a_single_voxel_works_like_a_slice() {
        let u = under();
        let f = frames(&u, |n| n as f32);
        let l = mask(layer(1), MaskRule::Expression("step(a-1.5)".into()));
        let none = |_: DatasetId, _: usize| None;
        let cx = Context {
            under: &u,
            layers: &[LayerInput {
                layer: &l,
                frames: &f,
            }],
            sub_frame: &none,
            apply_keep: true,
        };
        assert_eq!(evaluate(&cx, &[[3, 0, 0]])[0].passed, [true]);
        assert_eq!(evaluate(&cx, &[[1, 0, 0]])[0].passed, [false]);
    }

    #[test]
    fn a_layer_restricted_to_clusters_shows_only_the_kept_voxels() {
        let u = under();
        let mut f = frames(&u, |n| [0.0, 4.0, 5.0, 5.0].get(n).copied().unwrap_or(0.0));
        // Voxels 1 and 2 pass the threshold; only voxel 2 is in a cluster.
        let mut keep = vec![false; u.dims.iter().product()];
        keep[2] = true;
        f.keep = Some(Arc::new(keep));
        let l = OverlayLayer {
            threshold: 2.0,
            ..layer(1)
        };
        let r = &run(
            &u,
            &[LayerInput {
                layer: &l,
                frames: &f,
            }],
        )[0];
        assert_eq!(r.passed, [false, false, true, false]);
        assert!(r.colors[1].a == 0.0 && r.colors[2].a > 0.0);
    }

    #[test]
    fn clustering_a_layer_sees_it_unrestricted_and_others_see_the_restriction() {
        let u = under();
        let mut f = frames(&u, |n| [0.0, 4.0, 5.0, 5.0].get(n).copied().unwrap_or(0.0));
        let mut keep = vec![false; u.dims.iter().product()];
        keep[2] = true;
        f.keep = Some(Arc::new(keep));
        let a = layer(1);
        let a = OverlayLayer {
            threshold: 2.0,
            ..a
        };
        let mut b = mask(layer(2), MaskRule::Expression("c".into()));
        b.bindings
            .insert('c', crate::session::overlay::Binding::LayerMask(LayerId(1)));
        let g = frames(&u, |_| 0.0);
        let inputs = [
            LayerInput {
                layer: &a,
                frames: &f,
            },
            LayerInput {
                layer: &b,
                frames: &g,
            },
        ];
        let none = |_: DatasetId, _: usize| None;
        let run_with = |apply_keep| {
            evaluate(
                &Context {
                    under: &u,
                    layers: &inputs,
                    sub_frame: &none,
                    apply_keep,
                },
                &row(),
            )
        };
        let off = run_with(false);
        assert_eq!(off[0].passed, [false, true, true, true]);
        let on = run_with(true);
        assert_eq!(on[0].passed, [false, false, true, false]);
        // A mask reading the layer's mask sees where it is drawn.
        assert_eq!(on[1].passed, [false, false, true, false]);
    }
}
