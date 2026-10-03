//! Turning an overlay layer into colors for one slice.
//!
//! The work is `afni_core::overlay::evaluate_rows`, fed the OLay and Thr
//! values of the slice. This module builds the `OverlaySpec` from the layer
//! (`AFNI` conventions: ± or positive-only, colors over `[-R, R]` or `[0, R]`,
//! zeros not drawn, optional alpha fade "A") and adds the boxed outline "B".

use std::sync::Arc;

use afni_core::color::Rgba;
use afni_core::column::ColumnRange;
use afni_core::overlay::{
    ClipMode, FadeStyle, FailedThreshold, OverlayColors, OverlayInputs, OverlaySpec,
    RangeSelection, evaluate_rows,
};
use afni_core::threshold::{FadeCurve, FadeModel, MissingThreshold, Threshold};

#[cfg(test)]
use super::slice::Slice;
use crate::session::OverlayLayer;

/// Number of entries of the continuous color map built from an AFNI scale.
const SCALE_ENTRIES: usize = 256;

/// An overlay's sub-bricks on the underlay's grid, with the facts the
/// controls need. Built once per overlay choice (resampling is the costly
/// part), not per slice.
#[derive(Debug, Clone)]
pub struct OverlayFrames {
    /// The OLay values on the underlay grid (NaN outside the overlay).
    pub olay: Arc<Vec<f32>>,
    /// The Thr values on the underlay grid.
    pub thr: Arc<Vec<f32>>,
    /// Largest absolute finite value of the OLay sub-brick: the automatic top
    /// of the color range.
    pub auto_range: f64,
    /// Largest absolute finite value of the Thr sub-brick: the top of the
    /// threshold slider.
    pub thr_max: f64,
    /// Voxels of the underlay grid inside the layer's surviving clusters,
    /// when the layer is restricted to them (`None`: draw everything).
    pub keep: Option<Arc<Vec<bool>>>,
}

/// Largest absolute finite value, 0 if there is none.
pub fn max_abs(values: &[f32]) -> f64 {
    values
        .iter()
        .filter(|v| v.is_finite())
        .fold(0.0_f64, |m, v| m.max(f64::from(v.abs())))
}

/// The top of the color range in use: the user's, else the data's, else 1.
pub fn range_top(layer: &OverlayLayer, auto_range: f64) -> f64 {
    layer
        .range
        .unwrap_or(if auto_range > 0.0 { auto_range } else { 1.0 })
}

/// The `OverlaySpec` for a layer.
pub fn spec(layer: &OverlayLayer, auto_range: f64) -> afni_core::Result<OverlaySpec> {
    let top = range_top(layer, auto_range);
    let bottom = if layer.signed { -top } else { 0.0 };
    let mut spec = OverlaySpec::new(OverlayColors::Continuous(
        layer.colorscale.to_color_map(SCALE_ENTRIES)?,
    ));
    spec.intensity_range = RangeSelection::Manual(ColumnRange::new(bottom, top)?);
    spec.clip = ClipMode::Clamp;
    spec.threshold = if layer.signed {
        Threshold::AbsoluteAbove(layer.threshold)
    } else {
        Threshold::Above(layer.threshold)
    };
    spec.failed = if layer.fade {
        FailedThreshold::Fade {
            model: FadeModel::Afni {
                curve: FadeCurve::Linear,
                floor: 0.0,
            },
            style: FadeStyle::default(),
        }
    } else {
        FailedThreshold::Hide
    };
    spec.missing_threshold = MissingThreshold::Hide;
    spec.show_zero = false; // AFNI_OVERLAY_ZERO = NO
    spec.opacity = layer.opacity;
    spec.validate()?;
    Ok(spec)
}

/// Colors for the pixels of one slice, plus which pixels pass the threshold.
/// `olay` and `thr` are slices of the same plane and index. (The views go
/// through `render::layers`; this is the whole-slice form the tests use.)
#[cfg(test)]
pub fn colors_for_slice(
    layer: &OverlayLayer,
    auto_range: f64,
    olay: &Slice,
    thr: &Slice,
) -> afni_core::Result<(Vec<Rgba>, Vec<bool>)> {
    let intensity: Vec<f64> = olay.data.iter().map(|&v| f64::from(v)).collect();
    let threshold: Vec<f64> = thr.data.iter().map(|&v| f64::from(v)).collect();
    let (mut colors, passed) = colors_for_values(layer, auto_range, &intensity, &threshold)?;
    if layer.boxed {
        outline_only(&mut colors, &passed, olay.width, olay.height);
    }
    Ok((colors, passed))
}

/// Colors for a list of voxels given their OLay and Thr values (one entry per
/// voxel in both slices), plus which of them pass the threshold. This is the
/// color-map path; mask layers use `render::layers`.
pub fn colors_for_values(
    layer: &OverlayLayer,
    auto_range: f64,
    intensity: &[f64],
    threshold: &[f64],
) -> afni_core::Result<(Vec<Rgba>, Vec<bool>)> {
    let evaluation = evaluate_rows(
        &spec(layer, auto_range)?,
        &OverlayInputs {
            intensity,
            threshold: Some(threshold),
            ..OverlayInputs::default()
        },
    )?;
    Ok((evaluation.colors, evaluation.passed))
}

/// AFNI's "B": keep only the pixels on the edge of the suprathreshold
/// regions. A pixel is on the edge when one of its four neighbors inside the
/// image does not pass; everything else becomes transparent.
pub(crate) fn outline_only(colors: &mut [Rgba], passed: &[bool], width: usize, height: usize) {
    let idx = |x: usize, y: usize| y * width + x;
    for y in 0..height {
        for x in 0..width {
            let i = idx(x, y);
            let edge = passed[i]
                && [
                    (x > 0).then(|| idx(x - 1, y)),
                    (x + 1 < width).then(|| idx(x + 1, y)),
                    (y > 0).then(|| idx(x, y - 1)),
                    (y + 1 < height).then(|| idx(x, y + 1)),
                ]
                .into_iter()
                .flatten()
                .any(|n| !passed[n]);
            if !edge {
                colors[i] = Rgba::TRANSPARENT;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use afni_core::afni_colors::AfniColorScale;

    use super::*;
    use crate::session::store::DatasetId;

    fn layer() -> OverlayLayer {
        OverlayLayer {
            threshold: 2.0,
            ..OverlayLayer::new(DatasetId(1), AfniColorScale::RedsAndBlues)
        }
    }

    fn slice(data: Vec<f32>, width: usize) -> Slice {
        Slice {
            width,
            height: data.len() / width,
            data,
            pixel_mm: [1.0; 2],
            left: 'R',
            right: 'L',
            top: 'A',
            bottom: 'P',
        }
    }

    fn eval(layer: &OverlayLayer, values: &[f32]) -> (Vec<Rgba>, Vec<bool>) {
        let s = slice(values.to_vec(), values.len());
        colors_for_slice(layer, 5.0, &s, &s).unwrap()
    }

    #[test]
    fn signed_overlay_colors_extremes_and_hides_below_threshold() {
        let (colors, passed) = eval(&layer(), &[-5.0, 0.0, 1.9, 2.0, 5.0]);
        assert_eq!(passed, [true, false, false, true, true]);
        let map = AfniColorScale::RedsAndBlues.to_color_map(256).unwrap();
        // Range is [-5, 5]: -5 is the first color of the map, 5 the last.
        assert_eq!(colors[0], map.sample(0.0));
        assert_eq!(colors[4], map.sample(1.0));
        assert_eq!(colors[3], map.sample(0.7)); // (2 + 5) / 10
        assert_eq!(colors[1].a, 0.0); // zero is not drawn
        assert_eq!(colors[2].a, 0.0); // below threshold is hidden
        assert!(colors[0].a == 1.0 && colors[4].a == 1.0);
    }

    #[test]
    fn positive_only_hides_negatives_and_uses_zero_to_top() {
        let l = OverlayLayer {
            signed: false,
            ..layer()
        };
        let (colors, passed) = eval(&l, &[-5.0, 2.0, 5.0]);
        assert_eq!(passed, [false, true, true]);
        let map = AfniColorScale::RedsAndBlues.to_color_map(256).unwrap();
        assert_eq!(colors[2], map.sample(1.0));
        assert_eq!(colors[1], map.sample(0.4)); // 2 / 5
        assert_eq!(colors[0].a, 0.0);
    }

    #[test]
    fn a_fixed_range_replaces_the_automatic_one() {
        let l = OverlayLayer {
            range: Some(10.0),
            ..layer()
        };
        let (colors, _) = eval(&l, &[5.0]);
        let map = AfniColorScale::RedsAndBlues.to_color_map(256).unwrap();
        assert_eq!(colors[0], map.sample(0.75)); // (5 + 10) / 20
    }

    #[test]
    fn opacity_scales_alpha_and_visible_voxels_keep_it() {
        let l = OverlayLayer {
            opacity: 0.5,
            ..layer()
        };
        let (colors, _) = eval(&l, &[5.0]);
        assert!((colors[0].a - 0.5).abs() < 1e-6);
    }

    #[test]
    fn the_a_button_fades_instead_of_hiding() {
        let l = OverlayLayer {
            fade: true,
            ..layer()
        };
        let (colors, passed) = eval(&l, &[1.0, 2.0]);
        assert_eq!(passed, [false, true]);
        // AFNI linear ramp: |v| / t = 0.5 of 255 = 127.5 -> 128 (ties to even).
        assert!(
            (colors[0].a - 128.0 / 255.0).abs() < 1e-3,
            "{}",
            colors[0].a
        );
        assert_eq!(colors[1].a, 1.0);
    }

    #[test]
    fn the_thr_sub_brick_can_differ_from_the_olay_sub_brick() {
        let olay = slice(vec![3.0, 3.0], 2);
        let thr = slice(vec![1.0, 9.0], 2);
        let (_, passed) = colors_for_slice(&layer(), 5.0, &olay, &thr).unwrap();
        assert_eq!(passed, [false, true]);
    }

    #[test]
    fn nan_voxels_are_not_drawn() {
        let (colors, passed) = eval(&layer(), &[f32::NAN, 4.0]);
        assert_eq!(passed, [false, true]);
        assert_eq!(colors[0].a, 0.0);
    }

    #[test]
    fn boxed_keeps_only_the_outline_of_regions() {
        // 5x5, a 3x3 block passes: only its ring of 8 pixels stays.
        let mut v = vec![0.0_f32; 25];
        for y in 1..4 {
            for x in 1..4 {
                v[y * 5 + x] = 4.0;
            }
        }
        let l = OverlayLayer {
            boxed: true,
            ..layer()
        };
        let s = slice(v, 5);
        let (colors, passed) = colors_for_slice(&l, 5.0, &s, &s).unwrap();
        assert_eq!(passed.iter().filter(|p| **p).count(), 9);
        let drawn: Vec<usize> = (0..25).filter(|&i| colors[i].a > 0.0).collect();
        assert_eq!(drawn.len(), 8);
        assert!(!drawn.contains(&12)); // the center is inside, not on the edge
    }

    #[test]
    fn the_image_border_is_not_an_edge() {
        let mut colors = vec![Rgba::WHITE; 4];
        outline_only(&mut colors, &[true, true, true, true], 2, 2);
        // A region that fills the slice has no edge inside it.
        assert!(colors.iter().all(|c| c.a == 0.0));
    }

    #[test]
    fn range_top_prefers_the_user_then_the_data_then_one() {
        let l = layer();
        assert_eq!(range_top(&l, 7.0), 7.0);
        assert_eq!(range_top(&l, 0.0), 1.0);
        assert_eq!(
            range_top(
                &OverlayLayer {
                    range: Some(3.0),
                    ..l
                },
                7.0
            ),
            3.0
        );
    }

    #[test]
    fn max_abs_skips_non_finite_values() {
        assert_eq!(max_abs(&[1.0, -4.0, f32::NAN, f32::INFINITY, 3.0]), 4.0);
        assert_eq!(max_abs(&[]), 0.0);
    }
}
