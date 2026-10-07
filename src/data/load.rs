//! Reading datasets from disk through `afni-io`.

use std::path::Path;

use afni_core::curve::ThresholdCurve;
use afni_io::geometry::{Mat44, dicom_to_ras};
use afni_io::volume::{self, Volume};
use anyhow::{Context, Result};

use super::{Data, Dataset, Source};
use crate::geom::GridOrient;

/// Read an AFNI dataset or NIfTI volume.
///
/// `sess_trail` is AFNI's `AFNI_SESSTRAIL`: how many directory levels to keep
/// in the display name (0 shows the file name alone).
pub fn load(path: &Path, sess_trail: usize) -> Result<Dataset> {
    let vol = volume::read_any(path).with_context(|| format!("reading {}", path.display()))?;
    dataset_from_volume(vol, path, sess_trail)
}

/// Adapt a decoded file volume into afniru's display-oriented dataset model.
fn dataset_from_volume(vol: Volume, path: &Path, sess_trail: usize) -> Result<Dataset> {
    let (ijk_to_ras, ijk_to_ras_real) = grids(&vol)?;
    let voxel_mm = voxel_size(&ijk_to_ras);
    let tr = vol
        .afni_header()?
        .and_then(|h| h.time_axis())
        .and_then(|t| t.tr_seconds())
        .filter(|tr| *tr > 0.0);
    let nvols = vol.nvols();
    // A malformed statistic is not fatal for viewing: treat it as plain data.
    let stats = vol.stats().unwrap_or_else(|_| vec![None; nvols]);
    let header = vol.afni_header().ok().flatten();
    let fdr_curves = (0..nvols)
        .map(|t| {
            let raw = header.as_ref()?.fdr_curve(t)?;
            ThresholdCurve::new(raw.x0, raw.dx, raw.values).ok()
        })
        .collect();
    Ok(Dataset {
        name: display_name(path, sess_trail),
        source: Source::File(path.to_path_buf()),
        dims: vol.dimensions(),
        voxel_mm,
        nvols: vol.nvols(),
        tr,
        labels: vol.labels()?,
        stats,
        fdr_curves,
        orient: GridOrient::from_ijk_to_ras(&ijk_to_ras),
        ijk_to_ras,
        ijk_to_ras_real,
        data: Data::Loaded(Box::new(vol)),
    })
}

/// The display matrix and, for an oblique AFNI dataset, the real one.
fn grids(vol: &Volume) -> Result<(Mat44, Option<Mat44>)> {
    let Volume::Afni(brik) = vol else {
        return Ok((vol.ijk_to_ras()?, None));
    };
    let h = &brik.header;
    let cardinal = dicom_to_ras(&h.ijk_to_dicom_cardinal()?);
    let real = h.ijk_to_dicom_real().map(|m| dicom_to_ras(&m));
    Ok((cardinal, real.filter(|_| h.is_oblique())))
}

/// Voxel size: the length of each column of the 3×3 part of the matrix.
pub(crate) fn voxel_size(m: &afni_io::geometry::Mat44) -> [f64; 3] {
    std::array::from_fn(|c| (0..3).map(|r| m[r][c] * m[r][c]).sum::<f64>().sqrt())
}

/// Name for display: the file name without `.BRIK`/`.nii.gz`-style suffixes
/// that AFNI hides, preceded by up to `sess_trail` parent directories.
pub(crate) fn display_name(path: &Path, sess_trail: usize) -> String {
    let file = path.file_name().map(|f| f.to_string_lossy().into_owned());
    let mut name = file.unwrap_or_else(|| path.display().to_string());
    for suffix in [".HEAD", ".BRIK.gz", ".BRIK", ".nii.gz", ".nii"] {
        if let Some(stripped) = name.strip_suffix(suffix) {
            name = stripped.to_string();
            break;
        }
    }
    let parents: Vec<String> = path
        .parent()
        .map(|p| {
            p.components()
                .filter_map(|c| match c {
                    std::path::Component::Normal(s) => Some(s.to_string_lossy().into_owned()),
                    _ => None,
                })
                .collect()
        })
        .unwrap_or_default();
    let keep = sess_trail.min(parents.len());
    let mut parts: Vec<String> = parents[parents.len() - keep..].to_vec();
    parts.push(name);
    parts.join("/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn display_name_trims_suffixes_and_keeps_trail() {
        let p = Path::new("/data/sub-01/anat/anat+orig.HEAD");
        assert_eq!(display_name(p, 0), "anat+orig");
        assert_eq!(display_name(p, 1), "anat/anat+orig");
        assert_eq!(display_name(p, 9), "data/sub-01/anat/anat+orig");
        assert_eq!(display_name(Path::new("T1.nii.gz"), 1), "T1");
    }

    #[test]
    fn voxel_size_from_matrix_columns() {
        let m = [
            [-2.0, 0.0, 0.0, 0.0],
            [0.0, -2.0, 0.0, 0.0],
            [0.0, 0.0, 3.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert_eq!(voxel_size(&m), [2.0, 2.0, 3.0]);
    }

    #[test]
    fn missing_file_is_an_error_naming_it() {
        let e = load(Path::new("/nonexistent/x+orig"), 0).unwrap_err();
        assert!(format!("{e:#}").contains("x+orig"));
    }
}

#[cfg(test)]
mod fixture_tests {
    use super::*;

    /// `tests/fixtures/tiny2`: 4×5×6, 2×2×3 mm, 2 sub-bricks, TR 2 s
    /// (made with `3dUndump`, `3dcalc`, `3dTcat`, `3drefit`).
    #[test]
    fn loads_afni_fixture() {
        let path = Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/tiny2+orig.HEAD");
        let d = load(&path, 0).unwrap();
        assert_eq!(d.name, "tiny2+orig");
        assert_eq!(d.dims, [4, 5, 6]);
        assert_eq!(d.voxel_mm, [2.0, 2.0, 3.0]);
        assert_eq!(d.nvols, 2);
        assert_eq!(d.tr, Some(2.0));
        assert_eq!(d.frame(1).unwrap().len(), 4 * 5 * 6);
        assert_eq!(
            d.summary(),
            "tiny2+orig  4×5×6  2×2×3 mm  2 sub-bricks  TR 2s"
        );
    }

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// Rows of `3dmaskdump -xyz -noijk`: x y z then one value per sub-brick.
    fn xyz_dump(name: &str) -> Vec<[f64; 3]> {
        std::fs::read_to_string(fixture(name))
            .unwrap()
            .lines()
            .map(|l| {
                let v: Vec<f64> = l.split_whitespace().map(|t| t.parse().unwrap()).collect();
                [v[0], v[1], v[2]]
            })
            .collect()
    }

    fn assert_close(a: [f64; 3], b: [f64; 3], what: &str) {
        for axis in 0..3 {
            assert!((a[axis] - b[axis]).abs() < 1e-4, "{what}: {a:?} vs {b:?}");
        }
    }

    /// Our RAI coordinates of every voxel equal `3dmaskdump -xyz`.
    fn check_against_dump(d: &Dataset, dump: &[[f64; 3]]) {
        let [nx, ny, nz] = d.dims;
        assert_eq!(dump.len(), nx * ny * nz);
        for (n, expected) in dump.iter().enumerate() {
            let ijk = [n % nx, (n / nx) % ny, n / (nx * ny)];
            let ras = crate::geom::coords::ijk_to_ras(&d.ijk_to_ras, ijk);
            let rai = crate::geom::CoordOrient::Rai.ras_to_coords(ras);
            assert_close(rai, *expected, &format!("voxel {ijk:?}"));
        }
    }

    #[test]
    fn xyz_matches_3dmaskdump_for_cardinal_dataset() {
        let d = load(&fixture("tiny2+orig.HEAD"), 0).unwrap();
        assert!(d.ijk_to_ras_real.is_none());
        check_against_dump(&d, &xyz_dump("tiny2_maskdump_xyz.txt"));
    }

    /// `obl+orig` carries an `IJK_TO_DICOM_REAL` rotated 15° about z and
    /// 10° about x. `3dmaskdump -xyz` reports the cardinal grid; the real
    /// matrix is `3dinfo -aform_real`.
    #[test]
    fn oblique_dataset_keeps_cardinal_grid_and_real_matrix() {
        let d = load(&fixture("obl+orig.HEAD"), 0).unwrap();
        check_against_dump(&d, &xyz_dump("obl_maskdump_xyz.txt"));

        let real = d.ijk_to_ras_real.expect("oblique");
        let rai = afni_io::geometry::dicom_to_ras(&real); // RAS ↔ RAI flip
        let aform_real = [
            [1.931852, -0.509774, 0.134830, -3.5],
            [0.517638, 1.902502, -0.503194, 4.25],
            [0.0, 0.347296, 2.954423, -6.0],
        ];
        for r in 0..3 {
            for c in 0..4 {
                assert!((rai[r][c] - aform_real[r][c]).abs() < 1e-5, "[{r}][{c}]");
            }
        }
    }

    #[test]
    fn statistics_and_fdr_curves_come_from_the_header() {
        let d = load(&fixture("stat+orig.HEAD"), 0).unwrap();
        let spec = d.stats[0].as_ref().expect("fitt(118)");
        assert_eq!(spec.kind, afni_core::stat::StatKind::Ttest);
        assert_eq!(spec.params, [118.0]);
        assert!(d.fdr_curves[0].is_some());
        assert_eq!(d.labels, ["Tstat"]);
        // Plain data has neither.
        let plain = load(&fixture("tiny2+orig.HEAD"), 0).unwrap();
        assert!(plain.stats.iter().all(Option::is_none));
        assert!(plain.fdr_curves.iter().all(Option::is_none));
    }
}
