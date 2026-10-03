//! `Action`: everything a tool or the shell can ask the session to do.
//!
//! Plain data, so it can later be parsed from text (AFNI `-com`-style
//! scripting). [`Session::apply`](super::Session::apply) carries them out.

use super::overlay::{LayerId, OverlayChange};
use super::store::DatasetId;

/// A request to change the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Use this dataset as the underlay.
    SetUnderlay(DatasetId),
    /// Show this sub-brick of the underlay.
    SetUnderlaySubBrick(usize),
    /// Move the crosshair to this voxel.
    MoveCrosshair([usize; 3]),
    /// Move the crosshair to the voxel nearest this RAS+ position in mm.
    JumpToRas([f64; 3]),
    /// Add an overlay layer for this dataset, on top of the others.
    AddOverlay(DatasetId),
    /// Remove an overlay layer.
    RemoveOverlay(LayerId),
    /// Move a layer to position `to` of the stack (0 is the bottom layer).
    MoveOverlay {
        /// The layer to move.
        id: LayerId,
        /// Its new position, counted from the bottom.
        to: usize,
    },
    /// Change one overlay layer.
    Layer(LayerId, OverlayChange),
}
