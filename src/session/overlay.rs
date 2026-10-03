//! `OverlayLayer`: which dataset is drawn in color over the underlay, how its
//! values become colors, and where the threshold is. One layer for now;
//! several arrive in Milestone 5.
//!
//! This is *intent* (plain data). Turning it into pixels is `render::overlay`.

use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

use afni_core::afni_colors::AfniColorScale;
use afni_core::fdr::{afni_tail, q_value_for_threshold};
use afni_core::stat::StatSpec;
use afni_core::stats::Tail;

use super::store::DatasetId;
use crate::data::Dataset;

/// Identifies an overlay layer for the life of the session. The number is
/// also the layer's name in the interface ("Overlay 3").
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct LayerId(pub u64);

/// What a letter in a mask rule stands for.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Binding {
    /// This layer's own OLay sub-brick.
    Olay,
    /// This layer's own Thr sub-brick.
    Thr,
    /// Any dataset's sub-brick (resampled onto the underlay grid), as
    /// `3dcalc -a dset[sub]`.
    Sub {
        /// The dataset.
        dataset: DatasetId,
        /// The sub-brick.
        sub: usize,
    },
    /// Another layer's on/off result: 1 where that layer is drawn (its colors
    /// pass its threshold, or its mask is on), else 0. Works for hidden layers.
    LayerMask(LayerId),
    /// Another layer's OLay value.
    LayerValue(LayerId),
    /// A voxel coordinate or index.
    Coord(Coord),
}

/// Built-in variables, with `3dcalc`'s meanings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Coord {
    /// x in mm, DICOM (RAI) order: grows toward the subject's left.
    X,
    /// y in mm, DICOM order: grows toward posterior.
    Y,
    /// z in mm: grows toward superior.
    Z,
    /// Voxel index along the first axis of the underlay grid.
    I,
    /// Voxel index along the second axis.
    J,
    /// Voxel index along the third axis.
    K,
}

impl Coord {
    /// The variable letter that means this by default.
    pub fn letter(self) -> char {
        match self {
            Coord::X => 'x',
            Coord::Y => 'y',
            Coord::Z => 'z',
            Coord::I => 'i',
            Coord::J => 'j',
            Coord::K => 'k',
        }
    }

    /// The built-in meaning of a letter, if it has one.
    pub fn from_letter(c: char) -> Option<Coord> {
        [Coord::X, Coord::Y, Coord::Z, Coord::I, Coord::J, Coord::K]
            .into_iter()
            .find(|k| k.letter() == c)
    }
}

/// How a mask layer decides which voxels are on.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum MaskRule {
    /// The layer's own threshold: on where `|Thr| >= T` (± mode) or `Thr >= T`.
    Threshold,
    /// A `3dcalc` expression; a voxel is on where it is not zero.
    Expression(String),
}

/// A layer drawn as a mask: on/off, every "on" voxel the same color.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct MaskSettings {
    /// The rule.
    pub rule: MaskRule,
    /// The color of the "on" voxels (RGB).
    pub color: [u8; 3],
}

/// Starting colors for mask layers, picked by layer number.
const MASK_COLORS: [[u8; 3]; 6] = [
    [255, 70, 70],
    [60, 200, 90],
    [80, 150, 255],
    [255, 200, 40],
    [200, 100, 255],
    [40, 210, 210],
];

/// The unit of a minimum cluster size.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SizeUnit {
    /// A number of voxels (AFNI's `-clust_nvox`).
    Voxels,
    /// Microliters, which are cubic millimeters (a real volume, unlike
    /// `3dClusterize -clust_vol`, which AFNI reads as voxels).
    Microliters,
}

/// Clusterize, hooked under one overlay layer: how voxels join, how small a
/// cluster may be, and whether the layer then shows only the survivors.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClusterSettings {
    /// AFNI's NN level: 1 faces, 2 faces and edges, 3 faces, edges and corners.
    pub nn: u8,
    /// Smallest cluster kept, in `unit`.
    pub min_size: f64,
    /// The unit of `min_size`.
    pub unit: SizeUnit,
    /// Cluster positive and negative values separately (AFNI's `-bisided`);
    /// when off, a cluster may join both signs (`-2sided`). Only matters for a
    /// ± layer.
    pub bisided: bool,
    /// Draw only the voxels inside surviving clusters.
    pub only_clusters: bool,
}

impl Default for ClusterSettings {
    /// NN1, at least 10 voxels, separate signs, everything still drawn.
    fn default() -> Self {
        Self {
            nn: 1,
            min_size: 10.0,
            unit: SizeUnit::Voxels,
            bisided: true,
            only_clusters: false,
        }
    }
}

impl Hash for ClusterSettings {
    fn hash<H: Hasher>(&self, h: &mut H) {
        (self.nn, self.min_size.to_bits(), self.unit, self.bisided).hash(h);
        self.only_clusters.hash(h);
    }
}

impl ClusterSettings {
    /// May a session accept these settings?
    pub fn is_valid(&self) -> bool {
        (1..=3).contains(&self.nn) && self.min_size.is_finite() && self.min_size >= 0.0
    }
}

/// One overlay layer.
#[derive(Debug, Clone, PartialEq)]
pub struct OverlayLayer {
    /// Clusterize attached under this layer (`None`: not attached).
    pub cluster: Option<ClusterSettings>,
    /// What mask rules can say about a letter, kept even when the layer is
    /// not (or not currently) a mask.
    pub bindings: BTreeMap<char, Binding>,
    /// Draw as an on/off mask instead of a color map?
    pub as_mask: bool,
    /// The mask's rule and color (kept while the layer is a color map).
    pub mask: MaskSettings,
    /// Its identity.
    pub id: LayerId,
    /// The dataset drawn in color.
    pub dataset: DatasetId,
    /// Sub-brick whose values give the colors ("OLay").
    pub olay_sub: usize,
    /// Sub-brick that is thresholded ("Thr"); may be the same one.
    pub thr_sub: usize,
    /// The color scale.
    pub colorscale: AfniColorScale,
    /// `true` for ± (positive and negative values colored), `false` for
    /// positive values only.
    pub signed: bool,
    /// The top of the color range; `None` follows the data (largest absolute
    /// value of the OLay sub-brick).
    pub range: Option<f64>,
    /// The threshold, in units of the Thr sub-brick (absolute value).
    pub threshold: f64,
    /// Overall opacity, 0 to 1.
    pub opacity: f32,
    /// Drawn at all?
    pub visible: bool,
    /// AFNI's "A": voxels below the threshold fade out instead of vanishing.
    pub fade: bool,
    /// AFNI's "B": draw only the outline of the suprathreshold regions.
    pub boxed: bool,
}

impl OverlayLayer {
    /// A layer for `dataset` with AFNI-like starting settings.
    pub fn new(dataset: DatasetId, colorscale: AfniColorScale) -> Self {
        Self {
            id: LayerId(0),
            dataset,
            olay_sub: 0,
            thr_sub: 0,
            colorscale,
            signed: true,
            range: None,
            threshold: 0.0,
            opacity: 1.0,
            visible: true,
            fade: false,
            boxed: false,
            bindings: BTreeMap::from([('a', Binding::Olay), ('b', Binding::Thr)]),
            cluster: None,
            as_mask: false,
            mask: MaskSettings {
                rule: MaskRule::Threshold,
                color: MASK_COLORS[0],
            },
        }
    }

    /// The starting mask color for layer number `id`.
    pub fn mask_color_for(id: LayerId) -> [u8; 3] {
        MASK_COLORS[(id.0.max(1) as usize - 1) % MASK_COLORS.len()]
    }

    /// What `letter` means in this layer's rule: its binding, else the
    /// built-in meaning of coordinate letters.
    pub fn binding_for(&self, letter: char) -> Option<Binding> {
        self.bindings
            .get(&letter)
            .copied()
            .or_else(|| Coord::from_letter(letter).map(Binding::Coord))
    }

    /// The layers this layer's rule reads.
    pub fn referenced_layers(&self) -> Vec<LayerId> {
        self.bindings
            .values()
            .filter_map(|b| match b {
                Binding::LayerMask(l) | Binding::LayerValue(l) => Some(*l),
                _ => None,
            })
            .collect()
    }

    /// A number that changes when anything that affects the picture does
    /// (cache key for textures).
    pub fn display_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            self.dataset,
            self.olay_sub,
            self.thr_sub,
            self.colorscale,
            self.signed,
        )
            .hash(&mut h);
        (
            self.range.map(f64::to_bits),
            self.threshold.to_bits(),
            self.opacity.to_bits(),
        )
            .hash(&mut h);
        (self.visible, self.fade, self.boxed).hash(&mut h);
        (self.as_mask, &self.mask).hash(&mut h);
        self.bindings.hash(&mut h);
        // Only a layer restricted to its clusters is drawn differently.
        self.cluster.filter(|c| c.only_clusters).hash(&mut h);
        h.finish()
    }

    /// A number that changes when what the layer *selects* changes (dataset,
    /// sub-bricks, threshold, mask rule and bindings), not how it looks. The
    /// cache key of its clusters.
    pub fn selection_key(&self) -> u64 {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        (
            self.dataset,
            self.olay_sub,
            self.thr_sub,
            self.signed,
            self.threshold.to_bits(),
        )
            .hash(&mut h);
        self.as_mask.hash(&mut h);
        if self.as_mask {
            self.mask.rule.hash(&mut h);
            self.bindings.hash(&mut h);
        }
        h.finish()
    }

    /// The tail used for p-values and for "threshold by p": two-sided for ±
    /// when the statistic allows it, otherwise AFNI's natural tail.
    pub fn tail(&self, spec: &StatSpec) -> Option<Tail> {
        if self.signed && spec.supports_two_sided() {
            Some(Tail::TwoSided)
        } else {
            afni_tail(spec)
                .map(|t| if t == Tail::TwoSided { Tail::Upper } else { t })
                .or(Some(Tail::Upper))
        }
    }

    /// The p-value of the current threshold, when the Thr sub-brick is a
    /// known statistic.
    pub fn p_value(&self, ds: &Dataset) -> Option<f64> {
        let spec = ds.stats.get(self.thr_sub)?.as_ref()?;
        let tail = self.tail(spec)?;
        spec.p_value(self.threshold, tail).ok().map(|p| p.p())
    }

    /// The FDR q-value of the current threshold, when the file has an FDR
    /// curve for the Thr sub-brick.
    pub fn q_value(&self, ds: &Dataset) -> Option<f64> {
        let curve = ds.fdr_curves.get(self.thr_sub)?.as_ref()?;
        q_value_for_threshold(curve, self.threshold)
            .ok()
            .map(|q| q.get())
    }

    /// The threshold that gives p-value `p`, for a known statistic.
    pub fn threshold_for_p(&self, ds: &Dataset, p: f64) -> Option<f64> {
        let spec = ds.stats.get(self.thr_sub)?.as_ref()?;
        spec.critical_value(p, self.tail(spec)?).ok()
    }
}

/// A change to the overlay, applied by `Session::apply`.
#[derive(Debug, Clone, PartialEq)]
pub enum OverlayChange {
    /// Use this dataset in the layer, keeping its display settings.
    Dataset(DatasetId),
    /// Choose the OLay and Thr sub-bricks.
    SubBricks {
        /// Colors come from this sub-brick.
        olay: usize,
        /// The threshold applies to this one.
        thr: usize,
    },
    /// Choose the color scale.
    ColorScale(AfniColorScale),
    /// ± (`true`) or positive only (`false`).
    Signed(bool),
    /// A fixed top of the color range, or `None` for automatic.
    Range(Option<f64>),
    /// Set the threshold.
    Threshold(f64),
    /// Set the threshold to the value with this p-value.
    ThresholdByP(f64),
    /// Set the opacity (0 to 1).
    Opacity(f32),
    /// Show or hide the overlay.
    Visible(bool),
    /// AFNI's "A".
    Fade(bool),
    /// AFNI's "B".
    Boxed(bool),
    /// Draw the layer as an on/off mask (`true`) or as a color map (`false`);
    /// the rule and color are kept.
    MaskMode(bool),
    /// Choose the mask rule.
    MaskRule(MaskRule),
    /// Choose the color of the "on" voxels.
    MaskColor([u8; 3]),
    /// Say what a letter in the rule stands for (`None` removes the binding).
    Bind(char, Option<Binding>),
    /// Attach Clusterize to the layer with these settings, change them, or
    /// (`None`) detach it.
    Cluster(Option<ClusterSettings>),
}

/// The first sub-brick that holds a statistic, for the default Thr choice.
pub fn first_stat_sub_brick(ds: &Dataset) -> Option<usize> {
    ds.stats.iter().position(Option::is_some)
}

#[cfg(test)]
mod tests {
    use afni_core::stat::StatKind;

    use super::*;
    use crate::data::synthetic;

    fn tmap() -> Dataset {
        synthetic::tmap()
    }

    fn layer() -> OverlayLayer {
        OverlayLayer::new(DatasetId(1), AfniColorScale::SpectrumRedToBlue)
    }

    #[test]
    fn p_value_of_a_t_threshold_matches_afnis_cdf() {
        // `cdf -t2p fitt 3.1 118` -> 0.00242029 (two-sided, as for ±).
        let l = OverlayLayer {
            threshold: 3.1,
            ..layer()
        };
        let p = l.p_value(&tmap()).unwrap();
        assert!((p - 0.00242029).abs() < 1e-7, "{p}");
        let l = OverlayLayer {
            threshold: 2.0,
            ..layer()
        };
        assert!((l.p_value(&tmap()).unwrap() - 0.0477969).abs() < 1e-6);
    }

    #[test]
    fn positive_only_uses_one_tail() {
        let two = OverlayLayer {
            threshold: 2.0,
            signed: true,
            ..layer()
        }
        .p_value(&tmap())
        .unwrap();
        let one = OverlayLayer {
            threshold: 2.0,
            signed: false,
            ..layer()
        }
        .p_value(&tmap())
        .unwrap();
        assert!((one - two / 2.0).abs() < 1e-9, "{one} vs {two}");
    }

    #[test]
    fn threshold_for_p_inverts_p_value_and_matches_cdf() {
        // `cdf -p2t fitt 0.01 118` -> 2.61814.
        let t = layer().threshold_for_p(&tmap(), 0.01).unwrap();
        assert!((t - 2.61814).abs() < 1e-4, "{t}");
        let back = OverlayLayer {
            threshold: t,
            ..layer()
        }
        .p_value(&tmap())
        .unwrap();
        assert!((back - 0.01).abs() < 1e-9);
    }

    #[test]
    fn plain_data_has_no_p_and_no_q() {
        let mut d = tmap();
        d.stats = vec![None];
        let l = OverlayLayer {
            threshold: 3.0,
            ..layer()
        };
        assert_eq!(l.p_value(&d), None);
        assert_eq!(l.q_value(&d), None);
    }

    #[test]
    fn f_statistics_are_one_sided_even_when_signed() {
        let mut d = tmap();
        d.stats = vec![Some(StatSpec::new(StatKind::Ftest, &[2.0, 40.0], 0.0))];
        let l = OverlayLayer {
            threshold: 3.0,
            signed: true,
            ..layer()
        };
        assert!(l.p_value(&d).is_some());
    }

    #[test]
    fn display_key_changes_with_every_visible_setting() {
        let base = layer();
        let k = base.display_key();
        assert_eq!(k, base.clone().display_key());
        for changed in [
            OverlayLayer {
                threshold: 1.0,
                ..base.clone()
            },
            OverlayLayer {
                opacity: 0.5,
                ..base.clone()
            },
            OverlayLayer {
                signed: false,
                ..base.clone()
            },
            OverlayLayer {
                range: Some(5.0),
                ..base.clone()
            },
            OverlayLayer {
                fade: true,
                ..base.clone()
            },
            OverlayLayer {
                boxed: true,
                ..base.clone()
            },
            OverlayLayer {
                visible: false,
                ..base.clone()
            },
            OverlayLayer {
                colorscale: AfniColorScale::RedsAndBlues,
                ..base.clone()
            },
            OverlayLayer {
                olay_sub: 1,
                ..base.clone()
            },
            OverlayLayer {
                thr_sub: 1,
                ..base.clone()
            },
            OverlayLayer {
                dataset: DatasetId(2),
                ..base.clone()
            },
        ] {
            assert_ne!(changed.display_key(), k);
        }
    }

    /// The committed `stat+orig` (a t sub-brick, 118 df, with an FDR curve)
    /// against AFNI's own `cdf` and `fdrval` (see tests/fixtures/README.md).
    #[test]
    fn p_and_q_match_afni_on_a_real_dataset() {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/stat+orig.HEAD");
        let ds = crate::data::load::load(&path, 0).unwrap();
        let at = |t: f64| OverlayLayer {
            threshold: t,
            ..layer()
        };
        // `cdf -t2p fitt T 118`
        for (t, p) in [
            (1.0, 0.319357),
            (2.0, 0.0477969),
            (3.1, 0.00242029),
            (3.5, 0.000657355),
        ] {
            let got = at(t).p_value(&ds).unwrap();
            assert!((got - p).abs() / p < 1e-4, "t={t}: {got} vs {p}");
        }
        // `fdrval stat+orig 0 T`
        for (t, q) in [
            (1.0, 0.3847),
            (2.0, 0.088899),
            (3.0, 0.0070091),
            (3.1, 0.0057026),
            (3.5, 0.0023667),
        ] {
            let got = at(t).q_value(&ds).unwrap();
            assert!((got - q).abs() / q < 2e-4, "t={t}: {got} vs {q}");
        }
    }
}
