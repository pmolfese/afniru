//! Sampling an overlay dataset onto the underlay's grid, nearest neighbor.
//!
//! AFNI draws everything on the underlay's grid: each overlay voxel that a
//! display voxel falls in supplies its value (`AFNI_dataset_slice` with NN
//! resampling, the default). Display voxels outside the overlay's field of
//! view get NaN, which the overlay code treats as "nothing to draw".

use afni_io::geometry::Mat44;

use crate::geom::coords::invert_affine;

/// A voxel grid: its size and where it sits in the world.
#[derive(Debug, Clone, Copy)]
pub struct Grid<'a> {
    /// Voxel counts `[nx, ny, nz]`.
    pub dims: [usize; 3],
    /// Voxel-to-RAS matrix.
    pub ijk_to_ras: &'a Mat44,
}

impl Grid<'_> {
    /// Same size and (within a micrometer) the same placement?
    pub fn same_as(&self, other: &Grid) -> bool {
        self.dims == other.dims
            && (0..4).all(|r| {
                (0..4).all(|c| (self.ijk_to_ras[r][c] - other.ijk_to_ras[r][c]).abs() < 1e-6)
            })
    }
}

/// `a * b` for 4×4 matrices.
fn mul(a: &Mat44, b: &Mat44) -> Mat44 {
    let mut out = [[0.0; 4]; 4];
    for (r, row) in out.iter_mut().enumerate() {
        for (c, cell) in row.iter_mut().enumerate() {
            *cell = (0..4).map(|k| a[r][k] * b[k][c]).sum();
        }
    }
    out
}

/// The values of `src` (on grid `from`, voxel order `i + nx (j + ny k)`) at
/// every voxel of grid `onto`, nearest neighbor; NaN where `onto` is outside
/// `from`.
///
/// # Panics
/// If `src` does not have `from.dims` voxels.
pub fn nearest(src: &[f32], from: &Grid, onto: &Grid) -> Vec<f32> {
    let [sx, sy, sz] = from.dims;
    assert_eq!(src.len(), sx * sy * sz, "source does not match its grid");
    let [nx, ny, nz] = onto.dims;
    let Some(inverse) = invert_affine(from.ijk_to_ras) else {
        return vec![f32::NAN; nx * ny * nz];
    };
    // Destination voxel -> source voxel coordinates, in one matrix.
    let m = mul(&inverse, onto.ijk_to_ras);
    let mut out = Vec::with_capacity(nx * ny * nz);
    for k in 0..nz {
        for j in 0..ny {
            // Source coordinates of voxel (0, j, k); they change by column 0
            // of the matrix for each step in i.
            let base = |r: usize| m[r][1] * j as f64 + m[r][2] * k as f64 + m[r][3];
            let (bx, by, bz) = (base(0), base(1), base(2));
            for i in 0..nx {
                let i = i as f64;
                let x = (m[0][0] * i + bx + 0.5).floor();
                let y = (m[1][0] * i + by + 0.5).floor();
                let z = (m[2][0] * i + bz + 0.5).floor();
                let inside = x >= 0.0
                    && y >= 0.0
                    && z >= 0.0
                    && x < sx as f64
                    && y < sy as f64
                    && z < sz as f64;
                out.push(if inside {
                    src[x as usize + sx * (y as usize + sy * z as usize)]
                } else {
                    f32::NAN
                });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn eye(origin: [f64; 3], voxel: [f64; 3]) -> Mat44 {
        [
            [voxel[0], 0.0, 0.0, origin[0]],
            [0.0, voxel[1], 0.0, origin[1]],
            [0.0, 0.0, voxel[2], origin[2]],
            [0.0, 0.0, 0.0, 1.0],
        ]
    }

    /// value = i + 10 j + 100 k.
    fn ramp(dims: [usize; 3]) -> Vec<f32> {
        let mut v = Vec::new();
        for k in 0..dims[2] {
            for j in 0..dims[1] {
                for i in 0..dims[0] {
                    v.push((i + 10 * j + 100 * k) as f32);
                }
            }
        }
        v
    }

    #[test]
    fn the_same_grid_is_the_identity() {
        let m = eye([0.0; 3], [2.0, 2.0, 3.0]);
        let g = Grid {
            dims: [3, 4, 5],
            ijk_to_ras: &m,
        };
        assert!(g.same_as(&g));
        let src = ramp([3, 4, 5]);
        assert_eq!(nearest(&src, &g, &g), src);
    }

    #[test]
    fn a_shifted_grid_reads_the_voxel_it_lands_in() {
        let src_m = eye([0.0; 3], [1.0; 3]);
        let dst_m = eye([2.0, 0.0, 0.0], [1.0; 3]); // dst voxel i is at x = i + 2
        let src = Grid {
            dims: [4, 2, 2],
            ijk_to_ras: &src_m,
        };
        let dst = Grid {
            dims: [4, 2, 2],
            ijk_to_ras: &dst_m,
        };
        let out = nearest(&ramp([4, 2, 2]), &src, &dst);
        assert_eq!(out[0], 2.0); // x=2 -> source i=2
        assert_eq!(out[1], 3.0);
        assert!(out[2].is_nan() && out[3].is_nan()); // beyond the source's end
        assert_eq!(out[4], 12.0); // j=1
    }

    #[test]
    fn a_finer_destination_repeats_coarse_voxels() {
        let src_m = eye([0.0; 3], [2.0, 2.0, 2.0]); // centers at 0, 2, 4
        let dst_m = eye([-1.0, 0.0, 0.0], [1.0, 2.0, 2.0]); // x = -1, 0, 1, 2, 3, 4
        let src = Grid {
            dims: [3, 1, 1],
            ijk_to_ras: &src_m,
        };
        let dst = Grid {
            dims: [6, 1, 1],
            ijk_to_ras: &dst_m,
        };
        let out = nearest(&[10.0, 20.0, 30.0], &src, &dst);
        // Source voxel i covers x in [2i - 1, 2i + 1): a point exactly on an
        // edge goes to the higher voxel (x = -1 -> voxel 0, x = 1 -> voxel 1).
        assert_eq!(out, [10.0, 10.0, 20.0, 20.0, 30.0, 30.0]);
    }

    #[test]
    fn a_flipped_grid_is_read_backwards() {
        let src_m = eye([0.0; 3], [1.0; 3]);
        let dst_m = [
            [-1.0, 0.0, 0.0, 3.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let src = Grid {
            dims: [4, 1, 1],
            ijk_to_ras: &src_m,
        };
        let dst = Grid {
            dims: [4, 1, 1],
            ijk_to_ras: &dst_m,
        };
        assert_eq!(
            nearest(&[0.0, 1.0, 2.0, 3.0], &src, &dst),
            [3.0, 2.0, 1.0, 0.0]
        );
    }

    #[test]
    fn grids_that_differ_are_not_the_same() {
        let a = eye([0.0; 3], [1.0; 3]);
        let b = eye([0.5, 0.0, 0.0], [1.0; 3]);
        let ga = Grid {
            dims: [2, 2, 2],
            ijk_to_ras: &a,
        };
        assert!(!ga.same_as(&Grid {
            dims: [2, 2, 2],
            ijk_to_ras: &b
        }));
        assert!(!ga.same_as(&Grid {
            dims: [2, 2, 3],
            ijk_to_ras: &a
        }));
    }

    #[test]
    fn a_singular_source_gives_nothing() {
        let bad = [[0.0; 4]; 4];
        let m = eye([0.0; 3], [1.0; 3]);
        let out = nearest(
            &[1.0],
            &Grid {
                dims: [1, 1, 1],
                ijk_to_ras: &bad,
            },
            &Grid {
                dims: [2, 1, 1],
                ijk_to_ras: &m,
            },
        );
        assert!(out.iter().all(|v| v.is_nan()));
    }
}
