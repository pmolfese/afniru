//! Tools: one folder each, the extension point of afniru.
//!
//! A tool implements the small [`Tool`] trait and is added with one line in
//! [`tool`]; nothing in `app.rs` or `ui::controller` changes. A tool's logic
//! lives in its `mod.rs`, free of egui and unit tested; its card is drawn by
//! `card_ui`. A card reads the session and returns [`Action`]s; it never
//! changes the session itself. See `docs/ADDING_A_TOOL.md`.
//!
//! [`ToolId`] names every tool on the shelf, including ones not built yet
//! (shown dimmed). Atlas, Draw ROI, Montage and plugins come later.

pub mod clusterize;
pub mod crosshair;
pub mod datasets;
pub mod graph;
pub mod instacorr;
pub mod overlay;

use egui::{Painter, Ui};
use egui_phosphor::regular as icon;
use serde::{Deserialize, Serialize};

use crate::data::Dataset;
use crate::geom::{CoordOrient, Plane};
use crate::loader::{FolderListing, LoadingInfo};
use crate::render::overlay::OverlayFrames;
use crate::session::{Action, ControllerState, LayerId, OverlayLayer, Session};
use crate::ui::theme::Theme;

/// A stable identifier for a tool, used in workspaces and saved sessions.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ToolId {
    /// Datasets: pick the underlay (and later the overlay).
    Datasets,
    /// Define Overlay: colormap, range, threshold.
    Overlay,
    /// Clusterize.
    Clusterize,
    /// InstaCorr.
    InstaCorr,
    /// Graph settings.
    Graph,
    /// Atlas / Whereami.
    Atlas,
    /// Draw ROI.
    DrawRoi,
    /// Montage.
    Montage,
    /// Crosshair: coordinates and values.
    Crosshair,
    /// Plugins.
    Plugins,
}

impl ToolId {
    /// Every tool, in shelf order (two rows of five).
    pub const SHELF: [ToolId; 10] = [
        ToolId::Datasets,
        ToolId::Overlay,
        ToolId::Clusterize,
        ToolId::InstaCorr,
        ToolId::Graph,
        ToolId::Atlas,
        ToolId::DrawRoi,
        ToolId::Montage,
        ToolId::Crosshair,
        ToolId::Plugins,
    ];

    /// Short label on the shelf tile.
    pub fn label(self) -> &'static str {
        match self {
            ToolId::Datasets => "Data",
            ToolId::Overlay => "Overlay",
            ToolId::Clusterize => "Cluster",
            ToolId::InstaCorr => "InstaCorr",
            ToolId::Graph => "Graph",
            ToolId::Atlas => "Atlas",
            ToolId::DrawRoi => "Draw ROI",
            ToolId::Montage => "Montage",
            ToolId::Crosshair => "Xhair",
            ToolId::Plugins => "Plugins",
        }
    }

    /// Title on the card.
    pub fn title(self) -> &'static str {
        match self {
            ToolId::Datasets => "Datasets",
            ToolId::Overlay => "Define Overlay",
            ToolId::Clusterize => "Clusterize",
            ToolId::InstaCorr => "InstaCorr",
            ToolId::Graph => "Graph",
            ToolId::Atlas => "Atlas",
            ToolId::DrawRoi => "Draw ROI",
            ToolId::Montage => "Montage",
            ToolId::Crosshair => "Crosshair",
            ToolId::Plugins => "Plugins",
        }
    }

    /// The icon (a Phosphor glyph).
    pub fn icon(self) -> &'static str {
        match self {
            ToolId::Datasets => icon::DATABASE,
            ToolId::Overlay => icon::PALETTE,
            ToolId::Clusterize => icon::PLUS_SQUARE,
            ToolId::InstaCorr => icon::LIGHTNING,
            ToolId::Graph => icon::CHART_LINE,
            ToolId::Atlas => icon::MAP_TRIFOLD,
            ToolId::DrawRoi => icon::PENCIL_SIMPLE,
            ToolId::Montage => icon::FILM_STRIP,
            ToolId::Crosshair => icon::CROSSHAIR,
            ToolId::Plugins => icon::PLUGS,
        }
    }

    /// Where this tool is planned, for the tooltip on a dimmed tile.
    pub fn planned_in(self) -> &'static str {
        match self {
            ToolId::Overlay => "Milestone 4",
            ToolId::Clusterize => "Milestone 6",
            ToolId::Graph => "Milestone 7",
            ToolId::InstaCorr => "Milestone 9",
            _ => "a later milestone",
        }
    }

    /// The implementation, if the tool exists yet.
    pub fn tool(self) -> Option<&'static dyn Tool> {
        tool(self)
    }
}

/// The implementation of `id`, or `None` while it is only a tile on the shelf.
/// Adding a tool means adding its line here.
pub fn tool(id: ToolId) -> Option<&'static dyn Tool> {
    match id {
        ToolId::Datasets => Some(&datasets::DatasetsTool),
        ToolId::Overlay => Some(&overlay::OverlayTool),
        ToolId::Clusterize => Some(&clusterize::ClusterizeTool),
        ToolId::Crosshair => Some(&crosshair::CrosshairTool),
        ToolId::Graph => Some(&graph::GraphTool),
        _ => None,
    }
}

/// What a tool may read while drawing its card or painting on a view.
pub struct ToolContext<'a> {
    /// Colors.
    pub theme: &'a Theme,
    /// The whole session (read only).
    pub session: &'a Session,
    /// The active controller.
    pub controller: &'a ControllerState,
    /// The active controller's underlay.
    pub dataset: Option<&'a Dataset>,
    /// How coordinates are written.
    pub coord_orient: CoordOrient,
    /// The underlay's value at the crosshair.
    pub value: Option<f32>,
    /// The datasets being read in the background.
    pub loading: &'a [LoadingInfo],
    /// The folders whose datasets can be picked.
    pub folders: &'a [FolderListing],
    /// The overlay layers, bottom first.
    pub overlays: Vec<OverlayContext<'a>>,
}

impl ToolContext<'_> {
    /// The overlay layer with this id.
    pub fn overlay(&self, id: LayerId) -> Option<&OverlayContext<'_>> {
        self.overlays.iter().find(|o| o.layer.id == id)
    }
}

/// What a tool may read about one overlay layer.
pub struct OverlayContext<'a> {
    /// The layer's settings.
    pub layer: &'a OverlayLayer,
    /// The overlay dataset.
    pub dataset: &'a Dataset,
    /// Its sub-bricks on the underlay grid (absent until the views built them).
    pub frames: Option<&'a OverlayFrames>,
    /// The OLay and Thr values at the crosshair.
    pub values: Option<(f32, f32)>,
    /// The color the layer is drawn in at the crosshair (`None` if not drawn).
    pub drawn: Option<afni_core::color::Rgba>,
    /// Why the layer shows nothing, when its rule cannot be evaluated.
    pub problem: Option<String>,
    /// The layer's clusters, when Clusterize is hooked under it.
    pub cluster: Option<&'a clusterize::engine::Entry>,
}

/// One card of a tool. Most tools have a single card; a tool can have
/// several, such as Define Overlay with one per layer.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Instance {
    /// Distinguishes the cards of one tool (the layer number); 0 for a tool's
    /// only card.
    pub id: u64,
    /// The card's title, when it differs from the tool's.
    pub title: Option<String>,
}

impl Instance {
    /// The only card of a one-card tool.
    pub fn single() -> Self {
        Self { id: 0, title: None }
    }
}

/// One tool in the controller (Clusterize, InstaCorr, Atlas, ...).
pub trait Tool: Sync {
    /// Pinned tools are always present: their card has a pin instead of a
    /// close button and their tile cannot be turned off.
    fn pinned(&self) -> bool {
        false
    }

    /// The tool this one hooks under, if any (Clusterize → Overlay). Its
    /// cards then sit under the parent's card of the same instance, linked by
    /// a spine.
    fn attaches_to(&self) -> Option<ToolId> {
        None
    }

    /// What passes along the link to the parent, as the label on the spine's
    /// socket ("clusters the threshold").
    fn link_label(&self) -> &'static str {
        ""
    }

    /// Is this tool hooked under instance `parent` of its parent tool (for a
    /// parent of several cards, the layer number)? Drives the parent's
    /// attach chips.
    fn is_hooked(&self, _cx: &ToolContext, _parent: u64) -> bool {
        false
    }

    /// The action that hooks this tool under instance `parent` of its parent
    /// tool, or unhooks it.
    fn hook_action(&self, _parent: u64, _hooked: bool) -> Option<Action> {
        None
    }

    /// Actions to take when the user turns the tool's tile on (Clusterize
    /// hooks itself under the top overlay when nothing is hooked yet).
    fn opened(&self, _cx: &ToolContext) -> Vec<Action> {
        Vec::new()
    }

    /// The tool's cards, in the order shown. One card unless the tool says
    /// otherwise.
    fn instances(&self, _cx: &ToolContext) -> Vec<Instance> {
        vec![Instance::single()]
    }

    /// The body of one card. Reads state, returns actions; never mutates
    /// directly.
    fn card_ui(&self, ui: &mut Ui, cx: &ToolContext, instance: &Instance) -> Vec<Action>;

    /// One line shown when the card is collapsed.
    fn summary(&self, cx: &ToolContext, instance: &Instance) -> String;

    /// Optional drawing on top of a slice view (seed ring, brush, outlines).
    #[expect(
        dead_code,
        reason = "tools that draw on the views, e.g. the InstaCorr seed (Milestone 9)"
    )]
    fn paint_view(&self, _plane: Plane, _painter: &Painter, _cx: &ToolContext) {}
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shelf_lists_every_tool_once() {
        let mut seen = ToolId::SHELF.to_vec();
        seen.sort_by_key(|t| t.label());
        seen.dedup();
        assert_eq!(seen.len(), 10);
    }

    #[test]
    fn datasets_is_pinned_and_crosshair_is_not() {
        assert!(ToolId::Datasets.tool().unwrap().pinned());
        assert!(!ToolId::Crosshair.tool().unwrap().pinned());
        assert!(ToolId::Overlay.tool().is_some());
        assert!(ToolId::Clusterize.tool().is_some());
        assert!(ToolId::InstaCorr.tool().is_none());
    }
}
