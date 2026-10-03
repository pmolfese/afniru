//! `ControllerState`: one AFNI controller's underlay and crosshair. Overlay
//! layers, tool states and links join it in later milestones. Cloning one
//! (`derive(Clone)`) is how Clone-to-compare will work (Milestone 8).

use super::overlay::OverlayLayer;
use super::series::SeriesSettings;
use super::store::DatasetId;
use crate::geom::Plane;

/// The crosshair position and which card the keyboard acts on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cursor {
    /// Voxel `[i, j, k]` under the crosshair. Every card's slice follows it.
    pub ijk: [usize; 3],
    /// The card last touched or hovered: arrow keys and Page Up/Down act on
    /// its plane.
    pub active: Plane,
}

impl Default for Cursor {
    fn default() -> Self {
        Self {
            ijk: [0; 3],
            active: Plane::Axial,
        }
    }
}

/// One controller's state.
#[derive(Debug, Clone, Default)]
pub struct ControllerState {
    /// The dataset shown in gray.
    pub underlay: Option<DatasetId>,
    /// Which sub-brick of the underlay is shown.
    pub underlay_sub_brick: usize,
    /// The crosshair.
    pub cursor: Cursor,
    /// The overlay layers, bottom first: later layers are drawn over earlier
    /// ones.
    pub overlays: Vec<OverlayLayer>,
    /// What the Graph view plots.
    pub series: SeriesSettings,
    /// Changes whenever this controller's underlay or sub-brick does: the cache
    /// key of its views (the value comes from the session's counter, so two
    /// controllers never share one).
    pub generation: u64,
}
