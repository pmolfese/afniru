//! Clustering an overlay layer: the part of Clusterize with no interface.
//!
//! The work is `afni_core::volume_cluster::cluster_volume`, which matches
//! `3dClusterize`. This module decides what to feed it from an overlay layer:
//!
//! * a **color-map layer** is clustered on the overlay dataset's *own* grid,
//!   as AFNI's Clusterize does: the Thr sub-brick is thresholded (`|thr| >= T`
//!   for ±, `thr >= T` for positive only) and the OLay sub-brick supplies the
//!   values reported (peak, mean);
//! * a **mask layer** is clustered on the underlay grid, where its rule is
//!   evaluated: the voxels that are on are thresholded at one half, and no
//!   values are reported.
//!
//! The result also carries the surviving voxels on the underlay grid, so a
//! layer can be drawn restricted to its clusters.

use std::sync::Arc;

use afni_core::domain::VolumeDomain;
use afni_core::volume_cluster::{
    Tails, VolumeClusterInput, VolumeClusterParams, VoxelConnectivity, VoxelThreshold,
    cluster_volume,
};

use crate::geom::coords::ras_to_ijk;
use crate::render::resample::{self, Grid};
use crate::session::{ClusterSettings, SizeUnit};

/// One cluster, as a row of the table.
#[derive(Debug, Clone, PartialEq)]
pub struct ClusterRow {
    /// Rank by size, starting at 1 (the value in the cluster map).
    pub rank: u32,
    /// Number of voxels.
    pub voxels: usize,
    /// Volume in microliters (cubic millimeters).
    pub volume_ul: f64,
    /// The value of largest magnitude (signed). Meaningless for a mask.
    pub peak: f64,
    /// Where the peak is, RAS+ mm.
    pub peak_ras: [f64; 3],
    /// The center of mass, RAS+ mm.
    pub center_ras: [f64; 3],
    /// Mean of the (signed) values. Meaningless for a mask.
    pub mean: f64,
}

/// The clusters of one layer.
#[derive(Debug, Clone)]
pub struct ClusterOutcome {
    /// The surviving clusters, largest first.
    pub rows: Vec<ClusterRow>,
    /// Do the peak and mean mean anything? (Not for a mask.)
    pub has_values: bool,
    /// Voxels in surviving clusters, over all clusters.
    pub total_voxels: usize,
    /// The cluster rank of each voxel of the clustered grid (0: none).
    labels: Vec<u32>,
    /// The clustered grid.
    dims: [usize; 3],
    ijk_to_ras: afni_io::geometry::Mat44,
    /// Voxels of the underlay grid that lie in a surviving cluster.
    pub survivors: Arc<Vec<bool>>,
}

impl ClusterOutcome {
    /// The rank of the cluster at a RAS+ position, if it is inside one.
    pub fn rank_at(&self, ras: [f64; 3]) -> Option<u32> {
        let [i, j, k] = ras_to_ijk(&self.ijk_to_ras, self.dims, ras)?;
        let rank = *self.labels.get(i + self.dims[0] * (j + self.dims[1] * k))?;
        (rank != 0).then_some(rank)
    }
}

/// What is clustered.
#[derive(Debug, Clone, Copy)]
pub enum Input<'a> {
    /// A color-map layer's sub-bricks on its own grid.
    Values {
        /// The Thr sub-brick (what is thresholded).
        thr: &'a [f32],
        /// The OLay sub-brick (what is reported); `None` if it is the same
        /// sub-brick as Thr.
        olay: Option<&'a [f32]>,
    },
    /// A mask: the voxels that are on, on the underlay grid.
    Mask(&'a [bool]),
}

/// How the layer selects voxels.
#[derive(Debug, Clone, Copy)]
pub struct Selection {
    /// ± (`|thr| >= threshold`) or positive only (`thr >= threshold`).
    pub signed: bool,
    /// The layer's threshold (an absolute value).
    pub threshold: f64,
}

/// Cluster `input`, which lives on grid `source`; `under` is the underlay
/// grid the survivors are mapped onto.
pub fn run(
    input: Input,
    source: &Grid,
    select: Selection,
    settings: &ClusterSettings,
    under: &Grid,
) -> Result<ClusterOutcome, String> {
    let domain = VolumeDomain::new(None, source.dims, Some(*source.ijk_to_ras))
        .map_err(|e| e.to_string())?;
    let n = domain.voxel_count();
    let (thr, data, has_values): (Vec<f64>, Option<Vec<f64>>, bool) = match input {
        Input::Values { thr, olay } => {
            if thr.len() != n || olay.is_some_and(|o| o.len() != n) {
                return Err("the sub-bricks do not fit their grid".into());
            }
            (
                thr.iter().map(|&v| f64::from(v)).collect(),
                olay.map(|o| o.iter().map(|&v| f64::from(v)).collect()),
                true,
            )
        }
        Input::Mask(on) => {
            if on.len() != n {
                return Err("the mask does not fit its grid".into());
            }
            (
                on.iter().map(|&b| f64::from(u8::from(b))).collect(),
                None,
                false,
            )
        }
    };

    let t = select.threshold;
    let (threshold, tails) = match input {
        Input::Mask(_) => (VoxelThreshold::RightTail(0.5), Tails::Merged),
        Input::Values { .. } if select.signed => (
            VoxelThreshold::TwoSided {
                left_upper: -t,
                right_lower: t,
            },
            if settings.bisided {
                Tails::Separate
            } else {
                Tails::Merged
            },
        ),
        Input::Values { .. } => (VoxelThreshold::RightTail(t), Tails::Merged),
    };
    let mut params = VolumeClusterParams::new(
        VoxelConnectivity::from_nn(settings.nn).map_err(|e| e.to_string())?,
        threshold,
    );
    params.tails = tails;
    match settings.unit {
        SizeUnit::Voxels => {
            params.min_voxels =
                (settings.min_size >= 1.0).then(|| settings.min_size.ceil() as usize);
        }
        SizeUnit::Microliters => {
            params.min_volume = (settings.min_size > 0.0).then_some(settings.min_size);
        }
    }
    let found = cluster_volume(
        &VolumeClusterInput {
            domain: &domain,
            threshold_values: &thr,
            data_values: data.as_deref(),
            mask: None,
        },
        &params,
    )
    .map_err(|e| e.to_string())?;

    let rows: Vec<ClusterRow> = found
        .clusters
        .iter()
        .map(|c| ClusterRow {
            rank: c.label,
            voxels: c.voxel_count,
            volume_ul: c.volume.unwrap_or(0.0),
            peak: c.peak.1,
            peak_ras: c.peak_world.unwrap_or_default(),
            center_ras: c.center_of_mass_world.unwrap_or_default(),
            mean: c.mean,
        })
        .collect();
    let total_voxels = rows.iter().map(|r| r.voxels).sum();

    // Survivors on the underlay grid.
    let survivors: Vec<bool> = if source.same_as(under) {
        found.labels.iter().map(|&l| l != 0).collect()
    } else {
        let flags: Vec<f32> = found.labels.iter().map(|&l| f32::from(l != 0)).collect();
        resample::nearest(&flags, source, under)
            .into_iter()
            .map(|v| v == 1.0)
            .collect()
    };
    Ok(ClusterOutcome {
        rows,
        has_values,
        total_voxels,
        labels: found.labels,
        dims: source.dims,
        ijk_to_ras: *source.ijk_to_ras,
        survivors: Arc::new(survivors),
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::*;
    use crate::data::{Dataset, load::load};

    fn fixture(name: &str) -> Dataset {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures");
        load(&path.join(name), 0).unwrap()
    }

    fn grid(ds: &Dataset) -> Grid<'_> {
        Grid {
            dims: ds.dims,
            ijk_to_ras: &ds.ijk_to_ras,
        }
    }

    fn cluster(ds: &Dataset, signed: bool, threshold: f64, s: &ClusterSettings) -> ClusterOutcome {
        let frame = ds.frame(0).unwrap();
        run(
            Input::Values {
                thr: &frame,
                olay: None,
            },
            &grid(ds),
            Selection { signed, threshold },
            s,
            &grid(ds),
        )
        .unwrap()
    }

    // ---- 3dClusterize's own reports (tests/fixtures/clusterize/*.1D) ----

    /// A row of a 3dClusterize report: voxels, center (RAI), mean, peak and
    /// where it is (RAI).
    struct Reported {
        voxels: usize,
        center: [f64; 3],
        mean: f64,
        peak: f64,
        peak_at: [f64; 3],
    }

    fn report(name: &str) -> Vec<Reported> {
        let path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures/clusterize")
            .join(format!("{name}.1D"));
        let text = std::fs::read_to_string(path).unwrap();
        text.lines()
            .filter(|l| !l.trim_start().starts_with('#') && !l.trim().is_empty())
            .map(|l| {
                let v: Vec<f64> = l.split_whitespace().map(|x| x.parse().unwrap()).collect();
                assert_eq!(v.len(), 16, "{l}");
                Reported {
                    voxels: v[0] as usize,
                    center: [v[1], v[2], v[3]],
                    mean: v[10],
                    peak: v[12],
                    peak_at: [v[13], v[14], v[15]],
                }
            })
            .collect()
    }

    /// RAS+ to the RAI (DICOM) coordinates 3dClusterize prints.
    fn rai([x, y, z]: [f64; 3]) -> [f64; 3] {
        [-x, -y, z]
    }

    fn assert_matches_afni(name: &str, ours: &ClusterOutcome) {
        let theirs = report(name);
        assert_eq!(ours.rows.len(), theirs.len(), "{name}: number of clusters");
        for (n, (o, t)) in ours.rows.iter().zip(&theirs).enumerate() {
            let at = format!("{name}, cluster {}", n + 1);
            assert_eq!(o.voxels, t.voxels, "{at}: voxels");
            for a in 0..3 {
                // The report prints one decimal.
                assert!(
                    (rai(o.center_ras)[a] - t.center[a]).abs() < 0.051,
                    "{at}: center"
                );
                assert!(
                    (rai(o.peak_ras)[a] - t.peak_at[a]).abs() < 0.051,
                    "{at}: peak at"
                );
            }
            assert!(
                (o.peak - t.peak).abs() <= 1e-4 * t.peak.abs().max(1.0),
                "{at}: peak"
            );
            assert!(
                (o.mean - t.mean).abs() <= 1e-4 * t.mean.abs().max(1.0),
                "{at}: mean"
            );
        }
    }

    fn settings(nn: u8, min: f64, bisided: bool) -> ClusterSettings {
        ClusterSettings {
            nn,
            min_size: min,
            bisided,
            ..ClusterSettings::default()
        }
    }

    #[test]
    fn bisided_clusters_match_3dclusterize_at_every_nn_level() {
        let ds = fixture("clust+orig");
        for (name, nn, min) in [
            ("nn1_bisided_1.5_min2", 1, 2.0),
            ("nn2_bisided_1.5_min2", 2, 2.0),
            ("nn3_bisided_1.5_min2", 3, 2.0),
            ("nn1_bisided_1.5_min5", 1, 5.0),
            ("nn1_bisided_1.5_min3", 1, 3.0),
        ] {
            assert_matches_afni(name, &cluster(&ds, true, 1.5, &settings(nn, min, true)));
        }
    }

    #[test]
    fn positive_only_and_two_sided_clusters_match_3dclusterize() {
        let ds = fixture("clust+orig");
        assert_matches_afni(
            "nn2_right_2.0_min1",
            &cluster(&ds, false, 2.0, &settings(2, 1.0, true)),
        );
        // Not bisided: one cluster may join both signs.
        assert_matches_afni(
            "nn2_twosided_1.5_min2",
            &cluster(&ds, true, 1.5, &settings(2, 2.0, false)),
        );
    }

    #[test]
    fn a_minimum_in_microliters_is_a_real_volume() {
        // 12 µL per voxel: at least 30 µL is at least 3 voxels, which
        // 3dClusterize -clust_vol would read as 30 voxels (and find nothing).
        let ds = fixture("clust+orig");
        let ul = ClusterSettings {
            unit: SizeUnit::Microliters,
            ..settings(1, 30.0, true)
        };
        let by_volume = cluster(&ds, true, 1.5, &ul);
        assert_matches_afni("nn1_bisided_1.5_min3", &by_volume);
        assert!(by_volume.rows.iter().all(|r| r.volume_ul >= 30.0));
        assert!(
            (by_volume.rows[0].volume_ul - 12.0 * by_volume.rows[0].voxels as f64).abs() < 1e-9
        );
    }

    #[test]
    fn nn_levels_join_more_voxels() {
        let ds = fixture("clust+orig");
        let count = |nn| cluster(&ds, true, 1.5, &settings(nn, 1.0, true)).rows.len();
        assert!(count(1) > count(2));
        assert!(count(2) >= count(3));
    }

    #[test]
    fn survivors_are_the_voxels_of_the_kept_clusters() {
        let ds = fixture("clust+orig");
        let out = cluster(&ds, true, 1.5, &settings(2, 2.0, true));
        assert_eq!(out.survivors.len(), 4 * 5 * 6);
        assert_eq!(
            out.survivors.iter().filter(|s| **s).count(),
            out.total_voxels
        );
        // 3dClusterize: 23 + 22 + 9 + 2 voxels.
        assert_eq!(out.total_voxels, 56);
    }

    #[test]
    fn the_cluster_under_a_position_is_found() {
        let ds = fixture("clust+orig");
        let out = cluster(&ds, true, 1.5, &settings(2, 2.0, true));
        for row in &out.rows {
            assert_eq!(out.rank_at(row.peak_ras), Some(row.rank));
        }
        assert_eq!(out.rank_at([1000.0, 0.0, 0.0]), None);
    }

    #[test]
    fn a_threshold_above_every_value_leaves_no_clusters() {
        let ds = fixture("clust+orig");
        let out = cluster(&ds, true, 50.0, &settings(1, 1.0, true));
        assert!(out.rows.is_empty() && out.total_voxels == 0);
        assert!(out.survivors.iter().all(|s| !*s));
    }

    #[test]
    fn a_mask_is_clustered_without_values() {
        let ds = fixture("clust+orig");
        let on: Vec<bool> = ds
            .frame(0)
            .unwrap()
            .iter()
            .map(|v| v.abs() >= 1.5)
            .collect();
        let out = run(
            Input::Mask(&on),
            &grid(&ds),
            Selection {
                signed: true,
                threshold: 0.0,
            },
            &settings(2, 2.0, true),
            &grid(&ds),
        )
        .unwrap();
        assert!(!out.has_values);
        // NN2 on the thresholded mask joins both signs where they touch, so it
        // can only have fewer, larger clusters than the bisided report.
        assert!(out.rows.len() <= report("nn2_bisided_1.5_min2").len());
        assert!(out.total_voxels >= 56);
    }

    #[test]
    fn survivors_are_resampled_onto_a_different_underlay_grid() {
        let over = fixture("clust+orig"); // 4x5x6, 2x2x3 mm
        let mut under = over.clone();
        // An underlay with half the voxel size along i only.
        under.dims = [8, 5, 6];
        under.ijk_to_ras[0][0] /= 2.0;
        let frame = over.frame(0).unwrap();
        let out = run(
            Input::Values {
                thr: &frame,
                olay: None,
            },
            &grid(&over),
            Selection {
                signed: true,
                threshold: 1.5,
            },
            &settings(2, 2.0, true),
            &grid(&under),
        )
        .unwrap();
        assert_eq!(out.survivors.len(), 8 * 5 * 6);
        let kept = out.survivors.iter().filter(|s| **s).count();
        assert!(kept > out.total_voxels, "{kept} vs {}", out.total_voxels);
    }

    #[test]
    fn a_grid_that_does_not_fit_the_data_is_an_error() {
        let ds = fixture("clust+orig");
        let r = run(
            Input::Mask(&[true; 7]),
            &grid(&ds),
            Selection {
                signed: false,
                threshold: 0.0,
            },
            &ClusterSettings::default(),
            &grid(&ds),
        );
        assert!(r.is_err());
    }
}
