//! State shared by the cards of the view area: layout, options, the cursor.

use crate::prefs::Prefs;

/// How the cards are arranged in the view area.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Layout {
    /// 1×3: three cards side by side.
    Row,
    /// 3×1: three cards stacked.
    Column,
    /// 2×2: three planes and the Graph.
    #[default]
    Grid,
}

/// Display options the user can flip while running.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ViewOptions {
    /// How the cards are arranged.
    pub layout: Layout,
    /// Draw the crosshair lines.
    pub crosshair: bool,
    /// Neurological display (subject's left on screen left) instead of
    /// radiological.
    pub left_is_left: bool,
}

impl ViewOptions {
    /// Starting options from the preferences.
    pub fn from_prefs(prefs: &Prefs) -> Self {
        Self {
            layout: Layout::default(),
            crosshair: true,
            left_is_left: prefs.left_is_left,
        }
    }
}
