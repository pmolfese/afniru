//! Image analysis specific to afniru. The shared algorithms (statistics, colors,
//! thresholds, clustering) are in `afni-core` and used from there; these
//! modules are placeholders for afniru-side adapters.
pub mod cluster;
pub mod color;
pub mod threshold;
