//! Which voxel axis is which anatomical axis, and how a plane maps to screen.

use afni_io::geometry::Mat44;

/// The three standard viewing planes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Plane {
    /// Looking from below or above: left–right by anterior–posterior.
    Axial,
    /// Looking from the front: left–right by superior–inferior.
    Coronal,
    /// Looking from the side: anterior–posterior by superior–inferior.
    Sagittal,
}

impl Plane {
    /// All planes, in AFNI's usual order.
    pub const ALL: [Plane; 3] = [Plane::Axial, Plane::Coronal, Plane::Sagittal];

    /// Display name.
    pub fn name(self) -> &'static str {
        match self {
            Plane::Axial => "Axial",
            Plane::Coronal => "Coronal",
            Plane::Sagittal => "Sagittal",
        }
    }

    /// The RAS axis that is constant within the plane (0 = x, 1 = y, 2 = z).
    pub fn fixed_ras_axis(self) -> usize {
        match self {
            Plane::Axial => 2,
            Plane::Coronal => 1,
            Plane::Sagittal => 0,
        }
    }

    /// The horizontal and vertical screen axes.
    ///
    /// Conventions (AFNI's): anterior is up in axial; superior is up in
    /// coronal and sagittal; sagittal has anterior on the left; in coronal and
    /// axial the subject's right is on the screen's left unless
    /// `left_is_left` (neurological).
    pub fn screen_axes(self, left_is_left: bool) -> (ScreenAxis, ScreenAxis) {
        // Direction (in RAS) in which the screen coordinate increases.
        let lr = ScreenAxis {
            ras_axis: 0,
            dir: if left_is_left { 1 } else { -1 },
        };
        let down = |ras_axis| ScreenAxis { ras_axis, dir: -1 };
        match self {
            Plane::Axial => (lr, down(1)),
            Plane::Coronal => (lr, down(2)),
            Plane::Sagittal => (down(1), down(2)),
        }
    }
}

/// One screen axis: which RAS axis it follows and in which direction the
/// screen coordinate increases (left→right, or top→bottom).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ScreenAxis {
    /// 0 = x (R/L), 1 = y (A/P), 2 = z (S/I).
    pub ras_axis: usize,
    /// `+1` if the screen coordinate grows toward the positive RAS direction.
    pub dir: i8,
}

/// The anatomical letter for the positive (`true`) or negative end of a RAS
/// axis: x → R/L, y → A/P, z → S/I.
pub fn letter(ras_axis: usize, positive: bool) -> char {
    match (ras_axis, positive) {
        (0, true) => 'R',
        (0, false) => 'L',
        (1, true) => 'A',
        (1, false) => 'P',
        (2, true) => 'S',
        _ => 'I',
    }
}

/// Where one RAS axis lies in the voxel grid.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AxisMap {
    /// The voxel axis (0 = i, 1 = j, 2 = k) that follows this RAS axis.
    pub voxel_axis: usize,
    /// True if increasing voxel index moves toward the positive RAS end.
    pub positive: bool,
}

/// The assignment of voxel axes to RAS axes for one dataset.
///
/// Oblique grids are assigned to the nearest cardinal axes, one voxel axis
/// per RAS axis (largest matrix entries first).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct GridOrient {
    /// Indexed by RAS axis.
    pub axes: [AxisMap; 3],
}

impl GridOrient {
    /// Derive the orientation from the voxel-to-RAS matrix.
    pub fn from_ijk_to_ras(m: &Mat44) -> Self {
        let mut entries: Vec<(f64, usize, usize)> = (0..3)
            .flat_map(|r| (0..3).map(move |a| (m[r][a].abs(), r, a)))
            .collect();
        // Largest first; ties resolved by position so the result is stable.
        entries.sort_by(|x, y| y.0.total_cmp(&x.0).then(x.1.cmp(&y.1)).then(x.2.cmp(&y.2)));
        let mut axes = [AxisMap {
            voxel_axis: 0,
            positive: true,
        }; 3];
        let (mut ras_used, mut vox_used) = ([false; 3], [false; 3]);
        for (_, r, a) in entries {
            if ras_used[r] || vox_used[a] {
                continue;
            }
            ras_used[r] = true;
            vox_used[a] = true;
            axes[r] = AxisMap {
                voxel_axis: a,
                positive: m[r][a] >= 0.0,
            };
        }
        Self { axes }
    }

    /// The voxel axis that follows `ras_axis`.
    pub fn voxel_axis(&self, ras_axis: usize) -> usize {
        self.axes[ras_axis].voxel_axis
    }

    /// The voxel axis that is constant within `plane`; slices are indexed
    /// along it.
    pub fn slice_axis(&self, plane: Plane) -> usize {
        self.voxel_axis(plane.fixed_ras_axis())
    }

    /// AFNI-style three-letter orientation code, e.g. `RAI` for DICOM order:
    /// for each voxel axis, the letter of the end it *starts from* (so `RAI`
    /// means i runs from Right to Left).
    pub fn code(&self) -> String {
        (0..3)
            .map(|a| {
                let r = (0..3).find(|&r| self.axes[r].voxel_axis == a).unwrap_or(0);
                letter(r, !self.axes[r].positive)
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// AFNI RAI (DICOM) grid as RAS: i toward Left (−x), j toward Posterior
    /// (−y), k toward Superior (+z).
    pub(crate) fn rai() -> Mat44 {
        [
            [-1.0, 0.0, 0.0, 0.0],
            [0.0, -1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    #[test]
    fn rai_code_and_axes() {
        let o = GridOrient::from_ijk_to_ras(&rai());
        assert_eq!(o.code(), "RAI");
        assert_eq!(
            o.axes[0],
            AxisMap {
                voxel_axis: 0,
                positive: false
            }
        );
        assert_eq!(o.slice_axis(Plane::Axial), 2);
    }

    #[test]
    fn permuted_grid_ras_axes() {
        // LPS-ish axis permutation: i → +z, j → +x, k → −y.
        let m = [
            [0.0, 2.0, 0.0, 0.0],
            [0.0, 0.0, -2.0, 0.0],
            [3.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let o = GridOrient::from_ijk_to_ras(&m);
        assert_eq!(o.voxel_axis(0), 1);
        assert_eq!(o.voxel_axis(1), 2);
        assert!(!o.axes[1].positive);
        assert_eq!(o.voxel_axis(2), 0);
        assert_eq!(o.slice_axis(Plane::Sagittal), 1);
    }

    #[test]
    fn oblique_grid_still_gets_one_voxel_axis_per_ras_axis() {
        let c = 0.6_f64.cos();
        let s = 0.6_f64.sin();
        let m = [
            [-c, -s, 0.0, 0.0],
            [s, -c, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let o = GridOrient::from_ijk_to_ras(&m);
        let mut used: Vec<usize> = o.axes.iter().map(|a| a.voxel_axis).collect();
        used.sort();
        assert_eq!(used, [0, 1, 2]);
    }

    #[test]
    fn screen_conventions() {
        let (h, v) = Plane::Axial.screen_axes(false);
        assert_eq!((h.ras_axis, h.dir, v.ras_axis, v.dir), (0, -1, 1, -1));
        let (h, _) = Plane::Axial.screen_axes(true);
        assert_eq!(h.dir, 1);
        let (h, v) = Plane::Sagittal.screen_axes(false);
        assert_eq!((h.ras_axis, v.ras_axis), (1, 2));
    }
}
