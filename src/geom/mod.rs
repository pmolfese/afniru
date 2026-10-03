//! Geometry: how a dataset's voxel grid sits in the world, and how slices are
//! oriented on screen.
//!
//! World coordinates are RAS+ (x grows toward the subject's Right, y toward
//! Anterior, z toward Superior), as `afni-io` reports them. Slices are shown
//! on the dataset's own voxel grid (no rotation or resampling), as AFNI does.

pub mod orient;

pub use orient::{GridOrient, Plane, letter};
