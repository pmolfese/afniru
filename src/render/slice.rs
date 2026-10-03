//! Extracting one 2D slice from a volume, already in screen orientation.

use crate::geom::{GridOrient, Plane, letter};

/// How a plane's pixels map to voxels: the single place that knows which
/// voxel axis runs across, which runs down, and which way.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PlaneMap {
    /// Pixels across.
    pub width: usize,
    /// Pixels down.
    pub height: usize,
    /// Voxel axis running across the screen.
    h_axis: usize,
    /// Voxel axis running down the screen.
    v_axis: usize,
    /// Voxel axis that is constant in the plane (the slice axis).
    pub fixed_axis: usize,
    /// Does a rising column also raise the voxel index?
    h_same: bool,
    /// Does a rising row also raise the voxel index?
    v_same: bool,
}

impl PlaneMap {
    /// The mapping for `plane` on a grid of size `dims`.
    pub fn new(dims: [usize; 3], orient: &GridOrient, plane: Plane, left_is_left: bool) -> Self {
        let (h, v) = plane.screen_axes(left_is_left);
        let (h_map, v_map) = (orient.axes[h.ras_axis], orient.axes[v.ras_axis]);
        Self {
            width: dims[h_map.voxel_axis],
            height: dims[v_map.voxel_axis],
            h_axis: h_map.voxel_axis,
            v_axis: v_map.voxel_axis,
            fixed_axis: orient.slice_axis(plane),
            h_same: (h.dir > 0) == h_map.positive,
            v_same: (v.dir > 0) == v_map.positive,
        }
    }

    /// The voxel shown at pixel (`col`, `row`) of slice `index`.
    pub fn voxel(&self, col: usize, row: usize, index: usize) -> [usize; 3] {
        let mut ijk = [0; 3];
        ijk[self.fixed_axis] = index;
        ijk[self.h_axis] = if self.h_same {
            col
        } else {
            self.width - 1 - col
        };
        ijk[self.v_axis] = if self.v_same {
            row
        } else {
            self.height - 1 - row
        };
        ijk
    }

    /// The pixel (`col`, `row`) where voxel `ijk` appears in its slice.
    pub fn pixel(&self, ijk: [usize; 3]) -> (usize, usize) {
        let (h, v) = (ijk[self.h_axis], ijk[self.v_axis]);
        (
            if self.h_same { h } else { self.width - 1 - h },
            if self.v_same { v } else { self.height - 1 - v },
        )
    }
}

/// A slice ready for display: row-major, top row first, left pixel first.
#[derive(Debug, Clone, PartialEq)]
pub struct Slice {
    /// Pixels across.
    pub width: usize,
    /// Pixels down.
    pub height: usize,
    /// `width * height` values.
    pub data: Vec<f32>,
    /// Physical size of one pixel `[across, down]` in mm.
    pub pixel_mm: [f64; 2],
    /// Anatomical letter at the left edge.
    pub left: char,
    /// Anatomical letter at the right edge.
    pub right: char,
    /// Anatomical letter at the top edge.
    pub top: char,
    /// Anatomical letter at the bottom edge.
    pub bottom: char,
}

/// Number of slices of `plane` in a grid of size `dims`.
pub fn slice_count(dims: [usize; 3], orient: &GridOrient, plane: Plane) -> usize {
    dims[orient.slice_axis(plane)]
}

/// Extract slice `index` (a voxel index along the plane's slice axis).
///
/// `frame` is one sub-brick in `i + nx * (j + ny * k)` order. Returns `None`
/// if `frame` does not match `dims` or `index` is out of range.
pub fn extract(
    frame: &[f32],
    dims: [usize; 3],
    voxel_mm: [f64; 3],
    orient: &GridOrient,
    plane: Plane,
    index: usize,
    left_is_left: bool,
) -> Option<Slice> {
    let [nx, ny, nz] = dims;
    if frame.len() != nx * ny * nz || index >= slice_count(dims, orient, plane) {
        return None;
    }
    let map = PlaneMap::new(dims, orient, plane, left_is_left);
    let mut data = Vec::with_capacity(map.width * map.height);
    for row in 0..map.height {
        for col in 0..map.width {
            let [i, j, k] = map.voxel(col, row, index);
            data.push(frame[i + nx * (j + ny * k)]);
        }
    }
    let (h, v) = plane.screen_axes(left_is_left);
    Some(Slice {
        width: map.width,
        height: map.height,
        data,
        pixel_mm: [voxel_mm[map.h_axis], voxel_mm[map.v_axis]],
        // The left edge is where the screen coordinate is smallest, i.e. the
        // end opposite to its direction of increase.
        left: letter(h.ras_axis, h.dir < 0),
        right: letter(h.ras_axis, h.dir > 0),
        top: letter(v.ras_axis, v.dir < 0),
        bottom: letter(v.ras_axis, v.dir > 0),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::orient::GridOrient;

    const RAI: [[f64; 4]; 4] = [
        [-1.0, 0.0, 0.0, 0.0],
        [0.0, -1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];

    /// 3×4×5 volume where value = i + 10 j + 100 k.
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

    const DIMS: [usize; 3] = [3, 4, 5];
    const VOX: [f64; 3] = [2.0, 3.0, 4.0];

    fn get(plane: Plane, index: usize, lil: bool) -> Slice {
        let o = GridOrient::from_ijk_to_ras(&RAI);
        extract(&ramp(DIMS), DIMS, VOX, &o, plane, index, lil).unwrap()
    }

    #[test]
    fn axial_radiological_rai() {
        // RAI: i toward Left, j toward Posterior. Radiological axial puts
        // Right on the left and Anterior at the top: pixel (col,row) = (i, j).
        let s = get(Plane::Axial, 2, false);
        assert_eq!((s.width, s.height), (3, 4));
        assert_eq!(s.pixel_mm, [2.0, 3.0]);
        assert_eq!(s.data[0], 200.0); // i=0 j=0
        assert_eq!(s.data[2], 202.0); // i=2 j=0 (top right)
        assert_eq!(s.data[3 * 3], 230.0); // i=0 j=3 (bottom left)
        assert_eq!((s.left, s.right, s.top, s.bottom), ('R', 'L', 'A', 'P'));
    }

    #[test]
    fn axial_neurological_flips_horizontally() {
        let s = get(Plane::Axial, 2, true);
        assert_eq!(s.data[0], 202.0);
        assert_eq!(s.data[2], 200.0);
        assert_eq!((s.left, s.right), ('L', 'R'));
    }

    #[test]
    fn coronal_has_superior_on_top() {
        // Fixed axis is j; horizontal is i, vertical is k, flipped (top = high k).
        let s = get(Plane::Coronal, 1, false);
        assert_eq!((s.width, s.height), (3, 5));
        assert_eq!(s.pixel_mm, [2.0, 4.0]);
        assert_eq!(s.data[0], 410.0 + 0.0); // top row = k=4, i=0, j=1
        assert_eq!(s.data[3 * 4], 10.0); // bottom row = k=0
        assert_eq!((s.top, s.bottom), ('S', 'I'));
    }

    #[test]
    fn sagittal_has_anterior_on_left() {
        // Fixed axis is i; horizontal is j with anterior (low j) on the left.
        let s = get(Plane::Sagittal, 1, false);
        assert_eq!((s.width, s.height), (4, 5));
        assert_eq!(s.data[0], 401.0); // left = j=0, top = k=4
        assert_eq!(s.data[3], 431.0); // right = j=3
        assert_eq!((s.left, s.right, s.top, s.bottom), ('A', 'P', 'S', 'I'));
    }

    #[test]
    fn pixel_and_voxel_are_inverses() {
        let m = [
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let dims = [2, 3, 4];
        for orient in [
            GridOrient::from_ijk_to_ras(&RAI),
            GridOrient::from_ijk_to_ras(&m),
        ] {
            for plane in Plane::ALL {
                for lil in [false, true] {
                    let map = PlaneMap::new(dims, &orient, plane, lil);
                    for row in 0..map.height {
                        for col in 0..map.width {
                            let ijk = map.voxel(col, row, 1);
                            assert_eq!(ijk[map.fixed_axis], 1);
                            assert_eq!(map.pixel(ijk), (col, row));
                        }
                    }
                }
            }
        }
    }

    #[test]
    fn bad_index_or_length_is_none() {
        let o = GridOrient::from_ijk_to_ras(&RAI);
        let f = ramp(DIMS);
        assert!(extract(&f, DIMS, VOX, &o, Plane::Axial, 5, false).is_none());
        assert!(extract(&f[1..], DIMS, VOX, &o, Plane::Axial, 0, false).is_none());
        assert_eq!(slice_count(DIMS, &o, Plane::Sagittal), 3);
    }

    #[test]
    fn permuted_grid_still_shows_standard_orientation() {
        // i → +z (S), j → +x (R), k → −y (P). Axial slice is fixed i.
        let m = [
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, -1.0, 0.0],
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let o = GridOrient::from_ijk_to_ras(&m);
        let dims = [2, 3, 4];
        let s = extract(&ramp(dims), dims, [1.0; 3], &o, Plane::Axial, 1, false).unwrap();
        // Horizontal is x (Right on the left): x grows with j, so the left
        // pixel is the highest j. Vertical is y (anterior on top): y grows as
        // k falls, so the top row is k = 0. Top-left is i=1, j=2, k=0.
        assert_eq!(s.data[0], 1.0 + 20.0);
        assert_eq!((s.width, s.height), (3, 4));
        assert_eq!((s.left, s.top), ('R', 'A'));
    }
}

#[cfg(test)]
mod fixture_tests {
    use std::path::Path;

    use super::*;
    use crate::data::load;

    fn fixture(name: &str) -> std::path::PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    /// `tiny2_maskdump.txt` is `3dmaskdump -noijk tiny2+orig`: AFNI's own
    /// voxel order. The loaded sub-bricks must match it value for value.
    #[test]
    fn loaded_voxel_order_matches_3dmaskdump() {
        let d = load::load(&fixture("tiny2+orig.HEAD"), 0).unwrap();
        let dump = std::fs::read_to_string(fixture("tiny2_maskdump.txt")).unwrap();
        let rows: Vec<Vec<f32>> = dump
            .lines()
            .map(|l| l.split_whitespace().map(|v| v.parse().unwrap()).collect())
            .collect();
        assert_eq!(rows.len(), d.dims.iter().product::<usize>());
        for t in 0..d.nvols {
            let frame = d.frame(t).unwrap();
            for (n, row) in rows.iter().enumerate() {
                assert_eq!(frame[n], row[t], "sub-brick {t}, voxel {n}");
            }
        }
    }

    /// The fixture is RAI with value = i + 10 j + 100 k − 327, so slices can
    /// be checked against a formula.
    #[test]
    fn fixture_slices_follow_the_orientation() {
        let d = load::load(&fixture("tiny2+orig.HEAD"), 0).unwrap();
        assert_eq!(d.orient.code(), "RAI");
        let frame = d.frame(0).unwrap();
        let value = |i: usize, j: usize, k: usize| (i + 10 * j + 100 * k) as f32 - 327.0;
        let [nx, ny, nz] = d.dims;
        let get = |plane, index| {
            extract(&frame, d.dims, d.voxel_mm, &d.orient, plane, index, false).unwrap()
        };

        // Axial (radiological): pixel (col, row) = voxel (i, j).
        let s = get(Plane::Axial, 3);
        assert_eq!((s.width, s.height, s.pixel_mm), (nx, ny, [2.0, 2.0]));
        for row in 0..ny {
            for col in 0..nx {
                assert_eq!(s.data[col + nx * row], value(col, row, 3));
            }
        }
        // Coronal: pixel (col, row) = voxel (i, nz-1-row).
        let s = get(Plane::Coronal, 2);
        assert_eq!((s.width, s.height, s.pixel_mm), (nx, nz, [2.0, 3.0]));
        for row in 0..nz {
            for col in 0..nx {
                assert_eq!(s.data[col + nx * row], value(col, 2, nz - 1 - row));
            }
        }
        // Sagittal: pixel (col, row) = voxel (index, col, nz-1-row).
        let s = get(Plane::Sagittal, 1);
        assert_eq!((s.width, s.height, s.pixel_mm), (ny, nz, [2.0, 3.0]));
        for row in 0..nz {
            for col in 0..ny {
                assert_eq!(s.data[col + ny * row], value(1, col, nz - 1 - row));
            }
        }
    }
}
