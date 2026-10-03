//! Voxel ↔ world coordinates and the two coordinate conventions.
//!
//! Matrices map `(i, j, k, 1)` to RAS+ millimeters (x toward Right, y toward
//! Anterior, z toward Superior), as `afni-io` gives them. AFNI itself shows
//! coordinates as **RAI** (DICOM: x toward Left, y toward Posterior), so that
//! is the default display convention; **LPI** (= RAS+) is the option.

use afni_io::geometry::{Mat44, transform_point};

use super::orient::letter;

/// How coordinates are written for the user (`AFNI_ORIENT`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CoordOrient {
    /// AFNI's default: x grows toward Left, y toward Posterior, z toward
    /// Superior (DICOM / `3dmaskdump -xyz`).
    #[default]
    Rai,
    /// x grows toward Right, y toward Anterior, z toward Superior (= RAS+).
    Lpi,
}

impl CoordOrient {
    /// AFNI's name for the convention.
    pub fn name(self) -> &'static str {
        match self {
            CoordOrient::Rai => "RAI",
            CoordOrient::Lpi => "LPI",
        }
    }

    /// RAS+ millimeters → signed coordinates in this convention.
    pub fn ras_to_coords(self, [x, y, z]: [f64; 3]) -> [f64; 3] {
        match self {
            CoordOrient::Rai => [-x, -y, z],
            CoordOrient::Lpi => [x, y, z],
        }
    }

    /// Signed coordinates in this convention → RAS+ millimeters.
    pub fn coords_to_ras(self, c: [f64; 3]) -> [f64; 3] {
        // Flipping x and y is its own inverse.
        self.ras_to_coords(c)
    }
}

/// The voxel-to-world position of voxel `ijk` (RAS+ mm).
pub fn ijk_to_ras(m: &Mat44, ijk: [usize; 3]) -> [f64; 3] {
    transform_point(m, ijk.map(|v| v as f64))
}

/// The nearest voxel to a RAS+ point, or `None` outside the grid.
pub fn ras_to_ijk(m: &Mat44, dims: [usize; 3], ras: [f64; 3]) -> Option<[usize; 3]> {
    let inv = invert_affine(m)?;
    let f = transform_point(&inv, ras);
    let mut ijk = [0usize; 3];
    for a in 0..3 {
        let r = f[a].round();
        if r < 0.0 || r >= dims[a] as f64 {
            return None;
        }
        ijk[a] = r as usize;
    }
    Some(ijk)
}

/// Inverse of an affine matrix (bottom row `0 0 0 1`); `None` if singular.
pub fn invert_affine(m: &Mat44) -> Option<Mat44> {
    let a = |r: usize, c: usize| m[r][c];
    let cof =
        |r0: usize, r1: usize, c0: usize, c1: usize| a(r0, c0) * a(r1, c1) - a(r0, c1) * a(r1, c0);
    let det = a(0, 0) * cof(1, 2, 1, 2) - a(0, 1) * cof(1, 2, 0, 2) + a(0, 2) * cof(1, 2, 0, 1);
    if det.abs() < 1e-12 {
        return None;
    }
    let d = 1.0 / det;
    let l = [
        [
            cof(1, 2, 1, 2) * d,
            -cof(0, 2, 1, 2) * d,
            cof(0, 1, 1, 2) * d,
        ],
        [
            -cof(1, 2, 0, 2) * d,
            cof(0, 2, 0, 2) * d,
            -cof(0, 1, 0, 2) * d,
        ],
        [
            cof(1, 2, 0, 1) * d,
            -cof(0, 2, 0, 1) * d,
            cof(0, 1, 0, 1) * d,
        ],
    ];
    let t = [m[0][3], m[1][3], m[2][3]];
    let mut out = [[0.0; 4]; 4];
    for r in 0..3 {
        for c in 0..3 {
            out[r][c] = l[r][c];
        }
        out[r][3] = -(0..3).map(|c| l[r][c] * t[c]).sum::<f64>();
    }
    out[3][3] = 1.0;
    Some(out)
}

/// One coordinate for display: magnitude and the letter of the side it is on,
/// e.g. `(14.0, 'R')`. Values within 0.05 mm of zero show as positive.
pub fn magnitude_and_letter(ras: [f64; 3]) -> [(f64, char); 3] {
    std::array::from_fn(|axis| {
        let v = ras[axis];
        let near_zero = v.abs() < 0.05;
        (
            if near_zero { 0.0 } else { v.abs() },
            letter(axis, v >= 0.0 || near_zero),
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn oblique() -> Mat44 {
        [
            [1.9, -0.5, 0.1, -3.5],
            [0.5, 1.9, -0.5, 4.25],
            [0.0, 0.3, 2.9, -6.0],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    #[test]
    fn convention_flips_round_trip() {
        let ras = [12.0, -7.5, 3.0];
        assert_eq!(CoordOrient::Rai.ras_to_coords(ras), [-12.0, 7.5, 3.0]);
        assert_eq!(CoordOrient::Lpi.ras_to_coords(ras), ras);
        for c in [CoordOrient::Rai, CoordOrient::Lpi] {
            assert_eq!(c.coords_to_ras(c.ras_to_coords(ras)), ras);
        }
    }

    #[test]
    fn ijk_xyz_round_trip_for_oblique_grid() {
        let m = oblique();
        let dims = [4, 5, 6];
        for k in 0..6 {
            for j in 0..5 {
                for i in 0..4 {
                    let ras = ijk_to_ras(&m, [i, j, k]);
                    assert_eq!(ras_to_ijk(&m, dims, ras), Some([i, j, k]));
                }
            }
        }
    }

    #[test]
    fn point_outside_grid_is_none() {
        let m = oblique();
        assert_eq!(ras_to_ijk(&m, [4, 5, 6], [500.0, 0.0, 0.0]), None);
        assert_eq!(
            ras_to_ijk(&m, [4, 5, 6], ijk_to_ras(&m, [0, 0, 0]).map(|v| v - 3.0)),
            None
        );
    }

    #[test]
    fn singular_matrix_has_no_inverse() {
        let mut m = oblique();
        m[2] = m[1];
        assert!(invert_affine(&m).is_none());
    }

    #[test]
    fn letters_and_magnitudes() {
        let r = magnitude_and_letter([-14.0, 15.0, -0.01]);
        assert_eq!(r[0], (14.0, 'L'));
        assert_eq!(r[1], (15.0, 'A'));
        assert_eq!(r[2], (0.0, 'S'));
    }
}
