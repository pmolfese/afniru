//! Reading datasets from disk through `afni-io`.

use std::path::Path;

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
    summarize(vol, path, sess_trail)
}

fn summarize(vol: Volume, path: &Path, sess_trail: usize) -> Result<Dataset> {
    let ijk_to_ras = vol.ijk_to_ras()?;
    let voxel_mm = voxel_size(&ijk_to_ras);
    let tr = vol
        .afni_header()?
        .and_then(|h| h.time_axis())
        .and_then(|t| t.tr_seconds())
        .filter(|tr| *tr > 0.0);
    Ok(Dataset {
        name: display_name(path, sess_trail),
        source: Source::File(path.to_path_buf()),
        dims: vol.dimensions(),
        voxel_mm,
        nvols: vol.nvols(),
        tr,
        labels: vol.labels()?,
        orient: GridOrient::from_ijk_to_ras(&ijk_to_ras),
        ijk_to_ras,
        data: Data::Loaded(Box::new(vol)),
    })
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
}
