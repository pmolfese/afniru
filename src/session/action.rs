//! `Action`: everything a tool or the shell can ask the session to do.
//!
//! Plain data, so it can later be parsed from text (AFNI `-com`-style
//! scripting). [`Session::apply`](super::Session::apply) carries them out.

use super::Links;
use super::overlay::{LayerId, OverlayChange};
use super::series::SeriesChange;
use super::store::DatasetId;
use crate::geom::Plane;
use crate::render::export::ExportWhat;

/// What a dataset being opened should become.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LoadRole {
    /// The underlay.
    Underlay,
    /// A new overlay layer on top.
    Overlay,
    /// The dataset of an existing overlay layer (replacing its dataset).
    Layer(LayerId),
    /// The dataset the Graph plots (it need not be the underlay or an overlay).
    GraphSource,
    /// The Graph's fit (any dataset with as many time points).
    GraphFit,
}

/// A request to change the session.
#[derive(Debug, Clone, PartialEq)]
pub enum Action {
    /// Show controller `index` (A is 0, B is 1, ...) in the controller panel.
    SelectController(usize),
    /// Copy controller `from` (its underlay, every overlay layer and all its
    /// settings) into controller `to`; `to` is the next new controller to add
    /// one. The copy becomes the active controller. The datasets are shared,
    /// not copied.
    CloneController {
        /// The controller to copy.
        from: usize,
        /// The controller to write (an existing one, or the next new one).
        to: usize,
    },
    /// Remove controller `index` (the last one cannot be removed).
    RemoveController(usize),
    /// Choose what is linked between controllers.
    SetLinks(Links),
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
    /// Open a dataset from disk (in the background) as the underlay or as a
    /// new overlay layer. The session does nothing with it: the app loads it.
    LoadDataset(std::path::PathBuf, LoadRole),
    /// List (or list again) the datasets of a folder. Handled by the app.
    ScanFolder(std::path::PathBuf),
    /// Stop listing a folder. Handled by the app.
    CloseFolder(std::path::PathBuf),
    /// Stop waiting for load `id` (an id from the loading list).
    CancelLoad(u64),
    /// Save images of the views (a slice, the three views, a montage). The
    /// session does nothing with it: the app renders and writes the files.
    Export(ExportWhat),
    /// Like [`Action::Export`], with the Graph added under the views.
    ExportWithGraph(ExportWhat),
    /// Open the export dialog, with this plane preselected.
    ExportDialog(Plane),
    /// Change the Graph view's settings.
    Series(SeriesChange),
    /// Ask for a stimulus file and load it into the Graph view. The session
    /// does nothing with it: the app opens the dialog and reads the file.
    LoadStim,
    /// Save a layer's cluster table to a file. The session does nothing with
    /// it: the app asks for a file name and writes (a card cannot).
    SaveClusters(LayerId),
}
