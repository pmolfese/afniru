//! Datasets: what afniru knows about a volume on disk (or a synthetic one).
//!
//! [`Dataset`] holds a cheap summary (name, grid, voxel size, sub-brick labels,
//! TR) next to the voxel data, so the UI never has to ask `afni-io` for it
//! again. Voxel order is `i + nx * (j + ny * k)`, as in `afni-io`.

pub mod load;
pub mod synthetic;

use std::path::PathBuf;

use afni_core::curve::ThresholdCurve;
use afni_core::stat::StatSpec;
use afni_io::geometry::Mat44;
use afni_io::volume::Volume;

use crate::geom::GridOrient;

/// Where a dataset came from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Source {
    /// A file on disk (AFNI `.HEAD`/`.BRIK` or NIfTI).
    File(PathBuf),
    /// Generated in memory (`--demo`, tests).
    Synthetic,
}

/// The voxel data behind a [`Dataset`].
#[derive(Debug, Clone)]
pub enum Data {
    /// Read by `afni-io`.
    Loaded(Box<Volume>),
    /// One `f32` vector per sub-brick.
    Synthetic(Vec<Vec<f32>>),
}

/// A volume plus the facts the UI shows about it.
#[derive(Debug, Clone)]
pub struct Dataset {
    /// Short display name (prefix, honoring `AFNI_SESSTRAIL`).
    pub name: String,
    /// Where it came from.
    pub source: Source,
    /// Grid size `[nx, ny, nz]`.
    pub dims: [usize; 3],
    /// Voxel size in mm along i, j, k.
    pub voxel_mm: [f64; 3],
    /// Number of sub-bricks.
    pub nvols: usize,
    /// TR in seconds, for time series.
    pub tr: Option<f64>,
    /// One label per sub-brick.
    pub labels: Vec<String>,
    /// The statistic each sub-brick holds (`None` for plain data).
    pub stats: Vec<Option<StatSpec>>,
    /// The FDR curve of each sub-brick, when the file has one.
    pub fdr_curves: Vec<Option<ThresholdCurve>>,
    /// Voxel-to-RAS matrix of the display grid. For AFNI datasets this is
    /// the cardinal grid (`ORIGIN`/`DELTA`), which is what AFNI shows and
    /// `3dmaskdump -xyz` reports; for NIfTI it is the file's affine.
    pub ijk_to_ras: Mat44,
    /// For an oblique AFNI dataset, the true scanner-space matrix
    /// (`IJK_TO_DICOM_REAL`, `3dinfo -aform_real`), as RAS.
    pub ijk_to_ras_real: Option<Mat44>,
    /// Which voxel axis is which anatomical axis.
    pub orient: GridOrient,
    /// The voxels.
    pub data: Data,
}

impl Dataset {
    /// A synthetic dataset on the phantom grid (1 mm voxels, RAI order).
    pub(crate) fn synthetic(name: &str, frames: Vec<Vec<f32>>, labels: Vec<String>) -> Self {
        let [nx, ny, nz] = [
            synthetic::NX as f64,
            synthetic::NY as f64,
            synthetic::NZ as f64,
        ];
        // RAI: +i goes toward Left, +j toward Posterior, +k toward Superior;
        // the RAS matrix therefore flips x and y. The origin centers the grid.
        let ijk_to_ras = [
            [-1.0, 0.0, 0.0, nx / 2.0],
            [0.0, -1.0, 0.0, ny / 2.0],
            [0.0, 0.0, 1.0, -nz / 2.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        Self {
            name: name.to_string(),
            source: Source::Synthetic,
            dims: [synthetic::NX, synthetic::NY, synthetic::NZ],
            voxel_mm: [1.0; 3],
            nvols: frames.len(),
            tr: None,
            stats: vec![None; labels.len()],
            fdr_curves: vec![None; labels.len()],
            labels,
            orient: GridOrient::from_ijk_to_ras(&ijk_to_ras),
            ijk_to_ras,
            ijk_to_ras_real: None,
            data: Data::Synthetic(frames),
        }
    }

    /// Sub-brick `t` as `f32`, in `i + nx * (j + ny * k)` order.
    pub fn frame(&self, t: usize) -> Option<Vec<f32>> {
        match &self.data {
            Data::Loaded(v) => v.frame_f32(t),
            Data::Synthetic(f) => f.get(t).cloned(),
        }
    }

    /// One-line summary for the status bar, e.g. `anat+orig  256×256×170  1×1×1 mm  1 sub-brick`.
    pub fn summary(&self) -> String {
        let [nx, ny, nz] = self.dims;
        let [dx, dy, dz] = self.voxel_mm.map(trim_float);
        let plural = if self.nvols == 1 { "" } else { "s" };
        let mut s = format!(
            "{}  {nx}×{ny}×{nz}  {dx}×{dy}×{dz} mm  {} sub-brick{plural}",
            self.name, self.nvols
        );
        if let Some(tr) = self.tr {
            s.push_str(&format!("  TR {}s", trim_float(tr)));
        }
        if self.ijk_to_ras_real.is_some() {
            s.push_str("  oblique");
        }
        s
    }
}

/// Format with at most two decimals and no trailing zeros (`2`, `1.5`, `0.97`).
fn trim_float(v: f64) -> String {
    let s = format!("{v:.2}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn trim_float_drops_zeros() {
        assert_eq!(trim_float(2.0), "2");
        assert_eq!(trim_float(1.5), "1.5");
        assert_eq!(trim_float(0.973), "0.97");
    }

    #[test]
    fn phantom_summary_and_frame() {
        let d = synthetic::phantom();
        assert_eq!(d.nvols, 1);
        assert_eq!(d.summary(), "phantom  150×180×150  1×1×1 mm  1 sub-brick");
        assert_eq!(d.frame(0).unwrap().len(), 150 * 180 * 150);
        assert!(d.frame(1).is_none());
    }
}
