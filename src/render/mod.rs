//! Turning volumes into pixels: slice extraction and compositing.
//!
//! CPU only and independent of egui, so everything here is unit-testable.

pub mod compose;
pub mod export;
pub mod graph_image;
pub mod label;
pub mod layers;
pub mod mask;
pub mod overlay;
pub mod resample;
pub mod slice;
pub mod text;
