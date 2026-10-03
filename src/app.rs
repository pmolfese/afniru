//! The eframe application: owns the session and the controller, and runs one
//! frame.

use std::path::{Path, PathBuf};

use crate::data::{Dataset, Source, synthetic};
use crate::export_dialog::{ExportDialog, Outcome as ExportOutcome};
use crate::loader::{FolderListing, Loader};
use crate::prefs::CanvasBackground;
use crate::prefs::Prefs;
use crate::processing::StepId;
use crate::processing::model::{ProcessingModel, ViewOption};
use crate::recent::{RecentKind, Recents};
use crate::render::export::{self, ExportOptions, ExportWhat, Rgba8Image};
use crate::render::label::SliceLabel;
use crate::session::action::LoadRole;
use crate::session::overlay::OverlayChange;
use crate::session::series::{SeriesChange, Stim};
use crate::session::{Action as SessionAction, Session};
use crate::tools::clusterize::engine::{Engine, Wake};
use crate::tools::{OverlayContext, ToolContext};
use crate::ui::controller::{self, ControllerUi};
use crate::ui::fonts;
use crate::ui::processing_rail::{ProcessingRail, RailEvent};
use crate::ui::shell::{self, Action};
use crate::ui::theme::Theme;
use crate::ui::view_area::{OverlayTarget, Target, ViewArea, ZoomState};
use crate::ui::view_state::ViewOptions;

/// Everything afniru holds while running.
pub struct App {
    prefs: Prefs,
    /// Datasets and controllers (no egui types).
    session: Session,
    /// Last load error, shown in the status bar until the next load.
    error: Option<String>,
    /// One view area (layout, window, texture caches) for each controller.
    views: Vec<ViewArea>,
    /// Layout, crosshair, orientation and slice-number options, shared by all
    /// the views (copied into each before it is drawn).
    options: ViewOptions,
    /// Show controllers A and B side by side.
    compare: bool,
    /// Each controller's crosshair as of the last frame (to see which moved).
    last_cursors: Vec<[usize; 3]>,
    /// The generation each view was last reset for.
    view_generations: Vec<u64>,
    /// Each view's zoom and pan as of the last frame (to see which changed).
    last_zooms: Vec<ZoomState>,
    /// The controller sidebar: workspaces, rail state.
    controller: ControllerUi,
    /// The clusters of the layers Clusterize is hooked under, per controller.
    clusters: Vec<Engine>,
    /// The look of saved images (size, letters, crosshair).
    export_options: ExportOptions,
    /// The "Save images" dialog, when open.
    export_dialog: Option<ExportDialog>,
    /// A message for the status bar that is not an error (what was saved).
    notice: Option<String>,
    /// List the subfolders of a folder too (`afniru -R`).
    recursive_listing: bool,
    /// The datasets chosen recently, for the dropdowns (kept between runs).
    recents: Recents,
    /// Datasets being read (and folders being listed) in the background.
    loader: Loader,
    /// Folders given on the command line or opened, with their datasets.
    folders: Vec<FolderListing>,
    /// The afni_proc.py run being inspected, if one was found or opened.
    processing: Option<ProcessingModel>,
    /// The Processing rail on the right.
    rail: ProcessingRail,
    /// "View step datasets" waiting for the user's choice.
    pending_view: Option<ViewRequest>,
    /// Time of the last check for changed files (egui time, seconds).
    last_poll: f64,
}

/// A step's datasets offered to the user; choosing one replaces the
/// underlay, so it is always asked first (unless there is no underlay yet and
/// only one dataset).
struct ViewRequest {
    step_label: String,
    options: Vec<ViewOption>,
}

/// How often the run is checked for changed files.
const POLL_SECONDS: f64 = 2.0;

impl App {
    /// Start with `paths` loaded (and the demo phantom if `demo`).
    pub fn new(prefs: Prefs, paths: &[PathBuf], demo: bool) -> Self {
        let mut session = Session::new();
        session.colorscale = prefs.colorscale;
        let mut app = Self {
            views: vec![ViewArea::new(&prefs)],
            options: ViewOptions::from_prefs(&prefs),
            compare: false,
            last_cursors: vec![[0; 3]],
            view_generations: vec![u64::MAX],
            last_zooms: vec![ZoomState::default()],
            prefs,
            session,
            error: None,
            controller: ControllerUi::default(),
            clusters: vec![Engine::default()],
            export_options: ExportOptions::default(),
            export_dialog: None,
            notice: None,
            recursive_listing: false,
            recents: Recents::default(),
            loader: Loader::default(),
            folders: Vec::new(),
            processing: None,
            rail: ProcessingRail::default(),
            pending_view: None,
            last_poll: 0.0,
        };
        if demo {
            app.add(synthetic::phantom());
            let tmap = app.session.store.add(synthetic::tmap());
            app.session.apply(SessionAction::AddOverlay(tmap));
            // Layers start at threshold 0, as in AFNI; the demo shows the map
            // thresholded so that it looks like a result.
            let id = app.session.controller().overlays[0].id;
            app.session
                .apply(SessionAction::Layer(id, OverlayChange::Threshold(3.1)));
            // A task time series and its fit for the Graph view.
            let (bold, fit) = synthetic::bold();
            let bold = app.session.store.add(bold);
            let fit = app.session.store.add(fit);
            for change in [
                SeriesChange::Source(Some(bold)),
                SeriesChange::Fit(Some(fit)),
                SeriesChange::Stim(Some(Stim {
                    name: "task blocks".into(),
                    on: synthetic::bold_stimulus(),
                })),
            ] {
                app.session.apply(SessionAction::Series(change));
            }
        }
        // The first dataset is the underlay; the later ones become layers, in
        // order, each over the one before (`afniru anat func1 func2`). They load
        // in the background and are applied in this order.
        for (n, p) in paths.iter().enumerate() {
            let role = if n == 0 {
                LoadRole::Underlay
            } else {
                LoadRole::Overlay
            };
            app.request_load(p, role);
        }
        app.poll_loads();
        app
    }

    /// Restore the controller's saved workspaces and the rails' state.
    pub fn restore(&mut self, storage: &dyn eframe::Storage) {
        if let Some(saved) = eframe::get_value::<ControllerUi>(storage, controller::STORAGE_KEY) {
            self.controller = saved.normalized();
        }
        if let Some(saved) = eframe::get_value::<ProcessingRail>(storage, RAIL_STORAGE_KEY) {
            self.rail = saved;
        }
        if let Some(saved) = eframe::get_value::<Recents>(storage, RECENTS_STORAGE_KEY) {
            self.recents = saved;
        }
    }

    /// Look for an `afni_proc.py` run: first in the `explicit` directories
    /// (errors are reported), then in the `implicit` ones (the working
    /// directory, the folders of opened files; failures are silent). Only
    /// those directories are looked at, never their parents' trees.
    pub fn detect_processing(&mut self, explicit: &[PathBuf], implicit: &[PathBuf]) {
        for dir in explicit {
            if let Err(e) = self.set_processing(dir) {
                self.error = Some(e);
            } else {
                return;
            }
        }
        for dir in implicit {
            if self.set_processing(dir).is_ok() {
                return;
            }
        }
    }

    /// Open the run in `dir` in the Processing rail.
    fn set_processing(&mut self, dir: &Path) -> Result<(), String> {
        let model = ProcessingModel::open(dir).map_err(|e| e.to_string())?;
        self.processing = Some(model);
        self.rail.collapsed = false;
        Ok(())
    }

    /// "View" was pressed on a step: ask which dataset to show (or show the
    /// only one when nothing would be replaced).
    fn request_view(&mut self, id: &StepId) {
        let Some(model) = &self.processing else {
            return;
        };
        let options = model.view_options(id);
        let label = model
            .run
            .step(id)
            .map_or_else(String::new, |s| s.label.clone());
        match (options.as_slice(), self.session.underlay()) {
            ([], _) => self.error = Some(format!("{label}: no dataset to view")),
            ([only], None) => {
                let path = only.path.clone();
                self.open(&path);
            }
            _ => {
                self.pending_view = Some(ViewRequest {
                    step_label: label,
                    options,
                })
            }
        }
    }

    fn handle_rail(&mut self, events: Vec<RailEvent>) {
        for e in events {
            match e {
                RailEvent::View(id) => self.request_view(&id),
                RailEvent::Refresh => {
                    if let Some(m) = &mut self.processing {
                        m.refresh();
                    }
                }
            }
        }
    }

    /// Check the run for changed files now and then. The selection stays and
    /// the displayed dataset is never touched.
    fn poll_processing(&mut self, ctx: &egui::Context) {
        let Some(model) = &mut self.processing else {
            return;
        };
        let now = ctx.input(|i| i.time);
        if now - self.last_poll >= POLL_SECONDS {
            self.last_poll = now;
            if model.changed_on_disk() {
                model.refresh();
            }
        }
        ctx.request_repaint_after(std::time::Duration::from_secs_f64(POLL_SECONDS));
    }

    /// The "which dataset?" dialog.
    fn view_dialog(&mut self, ctx: &egui::Context, theme: &Theme) {
        let Some(request) = &self.pending_view else {
            return;
        };
        let replaces = self.session.underlay().map(|d| d.name.clone());
        let mut chosen: Option<PathBuf> = None;
        let response = egui::Modal::new(egui::Id::new("view_step_datasets")).show(ctx, |ui| {
            ui.set_width(360.0);
            ui.heading(format!("View {}", request.step_label));
            match &replaces {
                Some(name) => ui.label(format!("This replaces the current underlay ({name}).")),
                None => ui.label("Choose the dataset to show as the underlay."),
            };
            ui.add_space(6.0);
            for o in &request.options {
                let text = format!("{}  {}", egui_phosphor::regular::EYE, o.label);
                if ui
                    .button(text)
                    .on_hover_text(o.path.display().to_string())
                    .clicked()
                {
                    chosen = Some(o.path.clone());
                }
                ui.label(egui::RichText::new(&o.note).small().color(theme.text_dim));
                ui.add_space(4.0);
            }
            ui.separator();
            if ui.button("Cancel").clicked() {
                ui.close();
            }
        });
        if let Some(path) = chosen {
            self.pending_view = None;
            self.open(&path);
        } else if response.should_close() {
            self.pending_view = None;
        }
    }

    /// Keep one view and one cluster engine per controller.
    fn ensure_controllers(&mut self) {
        let n = self.session.controllers.len();
        while self.views.len() < n {
            let k = self.views.len();
            self.views.push(ViewArea::new(&self.prefs));
            self.clusters.push(Engine::default());
            self.last_cursors
                .push(self.session.controllers[k].cursor.ijk);
            self.view_generations.push(u64::MAX);
            self.last_zooms.push(self.views[k].zoom_state());
        }
        self.views.truncate(n);
        self.clusters.truncate(n);
        self.last_cursors.truncate(n);
        self.view_generations.truncate(n);
        self.last_zooms.truncate(n);
        self.reset_changed_views();
    }

    /// Reset the views of the controllers whose underlay changed.
    fn reset_changed_views(&mut self) {
        for (k, c) in self.session.controllers.iter().enumerate() {
            if let Some(seen) = self.view_generations.get_mut(k)
                && *seen != c.generation
            {
                *seen = c.generation;
                if let Some(v) = self.views.get_mut(k) {
                    v.reset();
                }
            }
        }
    }

    /// The two controllers shown side by side: A and, next to it, B (or the
    /// active one when it is a later controller).
    fn compared_pair(&self) -> (usize, usize) {
        let a = self.session.active;
        (0, if a == 0 { 1 } else { a })
    }

    /// Everything the views need of controller `k`, owned.
    fn target_data(&self, k: usize) -> Option<TargetData> {
        let c = self.session.controllers.get(k)?;
        let ds = self.session.store.get(c.underlay?)?.clone();
        let layers = self.session.overlay_layers_of(k);
        let keeps = layers
            .iter()
            .map(|(l, _)| self.clusters.get(k).and_then(|e| e.keep(l)))
            .collect();
        Some(TargetData {
            ds,
            sub_brick: c.underlay_sub_brick,
            generation: c.generation,
            layers,
            keeps,
            series: c.series.clone(),
        })
    }

    /// After the views: if a controller's crosshair moved, move the others'
    /// to the same place (when linked); remember where each now is.
    fn sync_links(&mut self) {
        let n = self.session.controllers.len().min(self.last_cursors.len());
        if let Some(k) =
            (0..n).find(|&k| self.session.controllers[k].cursor.ijk != self.last_cursors[k])
        {
            self.session.sync_crosshair(k);
        }
        for k in 0..n {
            self.last_cursors[k] = self.session.controllers[k].cursor.ijk;
        }
        // Zoom and pan follow the same way.
        let n = n.min(self.views.len()).min(self.last_zooms.len());
        if self.session.links.zoom
            && let Some(k) = (0..n).find(|&k| self.views[k].zoom_state() != self.last_zooms[k])
        {
            let state = self.views[k].zoom_state();
            for j in (0..n).filter(|&j| j != k) {
                self.views[j].set_zoom_state(state);
            }
        }
        for k in 0..n {
            self.last_zooms[k] = self.views[k].zoom_state();
        }
    }

    /// Make `d` the underlay.
    fn add(&mut self, d: Dataset) {
        self.error = None;
        self.session.add_dataset(d);
        self.reset_changed_views();
    }

    /// Open `path` as the underlay (in the background).
    fn open(&mut self, path: &Path) {
        self.request_load(path, LoadRole::Underlay);
        self.poll_loads();
    }

    /// Start reading `path` on a worker thread; the dataset is applied by
    /// [`App::poll_loads`] when it is ready.
    fn request_load(&mut self, path: &Path, role: LoadRole) {
        self.error = None;
        self.loader.load(path, role, self.prefs.sess_trail);
    }

    /// How many time points the Graph is plotting.
    fn plotted_length(&self) -> usize {
        let series = &self.session.controller().series;
        series
            .source
            .and_then(|id| self.session.store.get(id))
            .or_else(|| self.session.underlay())
            .map_or(0, |d| d.nvols)
    }

    /// Remember a dataset chosen from those already loaded.
    fn note_loaded(&mut self, kind: RecentKind, id: crate::session::DatasetId) {
        if let Some(d) = self.session.store.get(id)
            && let Source::File(path) = &d.source
        {
            self.recents.note(kind, path);
        }
    }

    /// Apply the loads that finished and take in the folder listings that
    /// arrived. A failed load is reported in the status bar.
    fn poll_loads(&mut self) {
        let (ready, listings) = self.loader.poll();
        for loaded in ready {
            match loaded.result {
                Ok(d) => {
                    self.error = None;
                    self.recents.note(RecentKind::of(loaded.role), &loaded.path);
                    match loaded.role {
                        LoadRole::GraphSource if d.nvols < 2 => {
                            self.error = Some(format!(
                                "{}: one time point, so there is no time series to plot",
                                d.name
                            ));
                        }
                        LoadRole::GraphSource => {
                            let id = self.session.store.add(d);
                            self.session
                                .apply(SessionAction::Series(SeriesChange::Source(Some(id))));
                        }
                        LoadRole::GraphFit => {
                            let plotted = self.plotted_length();
                            if d.nvols == plotted {
                                let id = self.session.store.add(d);
                                self.session
                                    .apply(SessionAction::Series(SeriesChange::Fit(Some(id))));
                            } else {
                                self.error = Some(format!(
                                    "{}: {} time points, but the plotted series has {plotted}",
                                    d.name, d.nvols
                                ));
                            }
                        }
                        LoadRole::Underlay => self.add(d),
                        LoadRole::Overlay => {
                            let id = self.session.store.add(d);
                            self.session.apply(SessionAction::AddOverlay(id));
                        }
                        LoadRole::Layer(layer) => {
                            let id = self.session.store.add(d);
                            self.session
                                .apply(SessionAction::Layer(layer, OverlayChange::Dataset(id)));
                            if self.session.layer(layer).is_none() {
                                // The layer was removed while this was loading.
                                self.session.store.release(id);
                            }
                        }
                    }
                }
                Err(e) => {
                    let name = loaded.path.display();
                    self.error = Some(format!("{name}: {e}"));
                }
            }
        }
        for (dir, result) in listings {
            if let Some(f) = self.folders.iter_mut().find(|f| f.dir == dir) {
                match result {
                    Ok(entries) => {
                        f.entries = Some(entries);
                        f.error = None;
                    }
                    Err(e) => {
                        f.entries = Some(Vec::new());
                        f.error = Some(e);
                    }
                }
            }
        }
    }

    /// Make the folders listed from now on include their subfolders.
    pub fn set_recursive_listing(&mut self, on: bool) {
        self.recursive_listing = on;
    }

    /// List the datasets of `dir` in the Datasets card (reading it in the
    /// background). A folder already listed is read again.
    pub fn add_folder(&mut self, dir: &Path) {
        if !self.prefs.folder_browser {
            return; // AFNIRU_FOLDER_BROWSER = NO
        }
        if !self.folders.iter().any(|f| f.dir == dir) {
            self.folders.push(FolderListing {
                dir: dir.to_path_buf(),
                recursive: self.recursive_listing,
                entries: None,
                error: None,
            });
        }
        // A folder already listed is read again the way it was first.
        let recursive = self
            .folders
            .iter()
            .find(|f| f.dir == dir)
            .is_some_and(|f| f.recursive);
        self.loader.scan(dir, recursive);
        self.poll_loads();
    }

    /// Carry out the actions cards asked for; reset the view's caches if
    /// what it displays changed.
    fn apply(&mut self, actions: Vec<SessionAction>) {
        if !actions.is_empty() {
            self.notice = None; // what was saved is news only until the next action
        }
        for a in actions {
            match a {
                SessionAction::RemoveController(i)
                    if self.session.controllers.len() > 1 && i < self.views.len() =>
                {
                    // Its view and clusters go with it.
                    self.views.remove(i);
                    self.clusters.remove(i);
                    self.last_cursors.remove(i);
                    self.view_generations.remove(i);
                    self.last_zooms.remove(i);
                    self.session.apply(SessionAction::RemoveController(i));
                    continue;
                }
                SessionAction::SaveClusters(id) => {
                    self.save_clusters(id);
                    continue;
                }
                SessionAction::LoadStim => {
                    self.load_stim();
                    continue;
                }
                SessionAction::LoadDataset(path, role) => {
                    self.request_load(&path, role);
                    continue;
                }
                SessionAction::ScanFolder(dir) => {
                    self.add_folder(&dir);
                    continue;
                }
                SessionAction::CloseFolder(dir) => {
                    self.folders.retain(|f| f.dir != dir);
                    continue;
                }
                SessionAction::Export(what) => {
                    let opts = self.current_export_options();
                    self.export(what, &opts);
                    continue;
                }
                SessionAction::ExportDialog(plane) => {
                    self.open_export_dialog(plane);
                    continue;
                }
                SessionAction::CancelLoad(id) => {
                    self.loader.cancel(id);
                    continue;
                }
                _ => {}
            }
            match &a {
                SessionAction::SetUnderlay(id) => self.note_loaded(RecentKind::Underlay, *id),
                SessionAction::AddOverlay(id)
                | SessionAction::Layer(_, OverlayChange::Dataset(id)) => {
                    self.note_loaded(RecentKind::Overlay, *id);
                }
                SessionAction::Series(SeriesChange::Source(Some(id)))
                | SessionAction::Series(SeriesChange::Fit(Some(id))) => {
                    self.note_loaded(RecentKind::Graph, *id);
                }
                _ => {}
            }
            // Cloning to compare switches the view area to the compare layout.
            let cloned = matches!(a, SessionAction::CloneController { .. });
            self.session.apply(a);
            if cloned {
                self.compare = true;
            }
        }
        if self.session.controllers.len() < 2 {
            self.compare = false;
        }
        self.ensure_controllers();
    }

    /// The saved-image options, with the slice number as shown on screen.
    fn current_export_options(&self) -> ExportOptions {
        ExportOptions {
            label: self.options.slice_label,
            ..self.export_options
        }
    }

    /// The canvas color saved images are laid out on.
    fn export_background(&self) -> [u8; 3] {
        match self.prefs.canvas {
            CanvasBackground::Black => [0, 0, 0],
            CanvasBackground::White => [255, 255, 255],
        }
    }

    /// Open the "Save images" dialog for `plane`.
    fn open_export_dialog(&mut self, plane: crate::geom::Plane) {
        let Some(ds) = self.session.underlay() else {
            return;
        };
        let count = ds.dims[ds.orient.slice_axis(plane)];
        self.export_dialog = Some(ExportDialog::new(
            plane,
            count,
            self.current_export_options(),
        ));
    }

    /// Render the pictures `what` asks for, as the views show them now.
    fn render_export(
        &mut self,
        what: ExportWhat,
        opts: &ExportOptions,
    ) -> Result<Vec<(String, Rgba8Image)>, String> {
        let active = self.session.active;
        let Some(data) = self.target_data(active) else {
            return Err("there is nothing to save: no dataset".into());
        };
        let cursor = self.session.controller().cursor;
        let background = self.export_background();
        self.views[active].options = self.options;
        let target = data.target(&self.session.store);
        self.views[active].export_images(&target, &cursor, what, opts, background, &Theme::dark())
    }

    /// Ask for a file name and save what `what` asks for. Several pictures
    /// (separate files) are named after the chosen name.
    fn export(&mut self, what: ExportWhat, opts: &ExportOptions) {
        let default = match what {
            ExportWhat::Slice(p) => format!("{}.png", p.name().to_lowercase()),
            ExportWhat::Views(_) => "views.png".to_string(),
            ExportWhat::Montage(m) => format!("montage_{}.png", m.plane.name().to_lowercase()),
            ExportWhat::Graph => "graph.png".to_string(),
        };
        let Some(path) = rfd::FileDialog::new()
            .set_title("Save image")
            .add_filter("PNG", &["png"])
            .set_file_name(default)
            .save_file()
        else {
            return;
        };
        self.export_to(what, opts, &path);
    }

    /// Render and write to `path` (see [`App::export`]).
    fn export_to(&mut self, what: ExportWhat, opts: &ExportOptions, path: &Path) {
        let result = self.render_export(what, opts).and_then(|images| {
            let mut written = Vec::new();
            for (suffix, image) in &images {
                let file = if suffix.is_empty() {
                    path.with_extension("png")
                } else {
                    export::numbered_name(path, suffix)
                };
                image.save_png(&file)?;
                written.push(file);
            }
            Ok(written)
        });
        match result {
            Ok(files) => {
                self.error = None;
                self.notice = Some(match files.as_slice() {
                    [one] => format!("Saved {}", one.display()),
                    many => format!("Saved {} images next to {}", many.len(), path.display()),
                });
            }
            Err(e) => self.error = Some(e),
        }
    }

    /// The "Save images" dialog.
    fn export_dialog(&mut self, ctx: &egui::Context, theme: &Theme) {
        let Some(dialog) = &mut self.export_dialog else {
            return;
        };
        let mut outcome = ExportOutcome::Open;
        let response = egui::Modal::new(egui::Id::new("export_dialog")).show(ctx, |ui| {
            outcome = dialog.ui(ui, theme);
        });
        match outcome {
            ExportOutcome::Save(what, opts) => {
                self.export_options = ExportOptions {
                    label: SliceLabel::default(),
                    ..opts
                };
                // The slice number is the views' setting; the dialog's box is
                // a shortcut for it.
                self.options.slice_label.show = opts.label.show;
                self.export_dialog = None;
                self.export(what, &opts);
            }
            ExportOutcome::Cancel => self.export_dialog = None,
            ExportOutcome::Open if response.should_close() => self.export_dialog = None,
            ExportOutcome::Open => {}
        }
    }

    /// Ask for a `.1D` file and show its first column as the Graph's stimulus.
    fn load_stim(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .set_title("Stimulus (.1D)")
            .add_filter("1D", &["1D", "txt"])
            .pick_file()
        else {
            return;
        };
        match crate::tools::graph::series::load_stim(&path) {
            Ok(stim) => self
                .session
                .apply(SessionAction::Series(SeriesChange::Stim(Some(stim)))),
            Err(e) => self.error = Some(e),
        }
    }

    /// Ask for a file name and write the cluster table of layer `id`.
    fn save_clusters(&mut self, id: crate::session::LayerId) {
        let Some(text) = self.cluster_report(id) else {
            return;
        };
        if let Some(path) = rfd::FileDialog::new()
            .set_title("Save clusters")
            .set_file_name("clusters.1D")
            .save_file()
            && let Err(e) = std::fs::write(&path, text)
        {
            self.error = Some(format!("writing {}: {e}", path.display()));
        }
    }

    /// The cluster table of layer `id` as text, if it has clusters.
    fn cluster_report(&self, id: crate::session::LayerId) -> Option<String> {
        let layer = self.session.layer_any(id)?;
        let settings = layer.cluster?;
        let out = self
            .clusters
            .iter()
            .find_map(|e| e.get(id))?
            .result
            .as_ref()
            .ok()?;
        let name = &self.session.store.get(layer.dataset)?.name;
        Some(crate::tools::clusterize::compute::report_text(
            out,
            self.prefs.coord_orient,
            &crate::tools::clusterize::heading(layer, name, &settings),
        ))
    }

    fn handle(&mut self, ctx: &egui::Context, action: Action) {
        match action {
            Action::Open => {
                let picked = rfd::FileDialog::new()
                    .set_title("Open dataset")
                    .add_filter("AFNI / NIfTI", &["HEAD", "BRIK", "nii", "gz"])
                    .pick_file();
                if let Some(p) = picked {
                    self.open(&p);
                }
            }
            Action::OpenDemo => self.add(synthetic::phantom()),
            Action::OpenResults => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Open afni_proc.py results directory")
                    .pick_folder()
                    && let Err(e) = self.set_processing(&dir)
                {
                    self.error = Some(e);
                }
            }
            Action::OpenFolder => {
                if let Some(dir) = rfd::FileDialog::new()
                    .set_title("Open folder of datasets")
                    .pick_folder()
                {
                    self.add_folder(&dir);
                    let _ = self.set_processing(&dir);
                }
            }
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}

impl App {
    /// Draw one frame and return what the user asked for in the shell. Kept
    /// apart from [`eframe::App::ui`] (which needs an `eframe::Frame`) so
    /// tests can drive it with a bare `Ui`.
    fn draw(&mut self, ui: &mut egui::Ui) -> Option<Action> {
        let ctx = ui.ctx().clone();
        fonts::ensure(&ctx);
        // `system_theme` is the OS appearance and updates live on macOS.
        let system_dark = ctx.system_theme().is_none_or(|t| t == egui::Theme::Dark);
        let theme = Theme::resolve(&self.prefs, system_dark);
        theme.apply(&ctx);

        let dropped: Vec<PathBuf> = ctx.input(|i| {
            i.raw
                .dropped_files
                .iter()
                .map(|f| f.path().to_path_buf())
                .filter(|p| !p.as_os_str().is_empty())
                .collect()
        });
        for p in dropped {
            if p.is_dir() {
                // List its datasets; if it is an afni_proc.py run, open that too.
                self.add_folder(&p);
                let _ = self.set_processing(&p);
            } else {
                self.open(&p);
            }
        }
        self.poll_loads();
        if self.loader.busy() {
            // Workers do not wake the window: check on them now and then.
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        self.poll_processing(&ctx);

        let underlay = self.session.underlay().cloned();
        let mut action = None;
        egui::Panel::top("menu").show(ui, |ui| {
            action = shell::menu_bar(ui);
        });
        self.ensure_controllers();
        let active = self.session.active;
        let count = self.session.controllers.len();
        let pair = self.compared_pair();
        let mut multi = shell::Multi {
            count,
            compare: &mut self.compare,
            links: self.session.links,
            pair,
            differences: if count >= 2 {
                self.session.differences(pair.0, pair.1)
            } else {
                Vec::new()
            },
            actions: Vec::new(),
        };
        egui::Panel::top("toolbar").show(ui, |ui| {
            shell::toolbar(
                ui,
                &theme,
                underlay.as_deref(),
                &mut self.options,
                Some(&mut multi),
            );
        });
        let toolbar_actions = std::mem::take(&mut multi.actions);
        egui::Panel::bottom("status").show(ui, |ui| {
            shell::status_bar(
                ui,
                &theme,
                underlay.as_deref(),
                self.error.as_deref(),
                &self.views[active].conventions(underlay.as_deref()),
                &self.loader.loading(),
                self.notice.as_deref(),
            );
        });

        // Clusters follow the layers' thresholds; they wait for the mouse to
        // be released so that dragging a threshold stays smooth. Every
        // controller has its own.
        let settled = !ctx.input(|i| i.pointer.any_down());
        let wake: Wake = {
            let ctx = ctx.clone();
            std::sync::Arc::new(move || ctx.request_repaint())
        };
        for k in 0..count {
            let Some(under) = self.session.controllers[k]
                .underlay
                .and_then(|id| self.session.store.get(id))
                .cloned()
            else {
                continue;
            };
            let view = &self.views[k];
            let waiting = self.clusters[k].update(
                &self.session,
                k,
                &under,
                settled,
                &|layers, id| view.passed_everywhere(&under, layers, id),
                &wake,
            );
            if waiting {
                ctx.request_repaint();
            } else if self.clusters[k].busy() {
                // The worker wakes the interface when done; this is a net.
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }

        // The controller (left), then the views.
        let mut actions = {
            let controller = self.session.controller();
            let loading = self.loader.loading();
            let layers = self.session.overlay_layers();
            let plain: Vec<_> = layers.iter().map(|(l, _)| l.clone()).collect();
            let probes = underlay
                .as_deref()
                .map(|d| self.views[active].probe(d, &plain, &controller.cursor))
                .unwrap_or_default();
            let cx = ToolContext {
                theme: &theme,
                session: &self.session,
                controller,
                dataset: underlay.as_deref(),
                coord_orient: self.prefs.coord_orient,
                value: underlay
                    .as_deref()
                    .and_then(|d| self.views[active].value_at(d, &controller.cursor)),
                loading: &loading,
                recents: &self.recents,
                folders: &self.folders,
                overlays: layers
                    .iter()
                    .enumerate()
                    .map(|(n, (layer, dataset))| OverlayContext {
                        layer,
                        dataset: dataset.as_ref(),
                        drawn: probes.get(n).and_then(|p| p.drawn),
                        problem: probes.get(n).and_then(|p| p.problem.clone()),
                        frames: self.views[active].overlay_frames(layer.id),
                        cluster: self.clusters[active].get(layer.id),
                        values: underlay.as_deref().and_then(|d| {
                            self.views[active].overlay_values_at(layer.id, d, &controller.cursor)
                        }),
                    })
                    .collect(),
            };
            self.controller.panel(ui, &cx)
        };
        actions.extend(toolbar_actions);
        self.apply(actions);

        // The Processing rail (right), when there is a run.
        let rail_events = match &mut self.processing {
            Some(model) => self.rail.panel(ui, &theme, model),
            None => Vec::new(),
        };
        self.handle_rail(rail_events);

        // Cards may have changed the underlay.
        self.ensure_controllers();
        let active = self.session.active;
        let underlay = self.session.underlay().cloned();
        if let Some(ds) = &underlay {
            self.views[active].handle_keys(&ctx, ds, &mut self.session.controllers[active].cursor);
            if let Some(data) = self.target_data(active) {
                let target = data.target(&self.session.store);
                let cursor = self.session.controller().cursor;
                egui::Panel::bottom("readout").show(ui, |ui| {
                    self.views[active].readout(ui, &theme, &target, &cursor);
                });
            }
        }
        let mut graph_actions = Vec::new();
        let background = egui::Frame::new().fill(theme.bg).inner_margin(8);
        let pair = self.compared_pair();
        let comparing = self.compare && self.session.controllers.len() >= 2;
        let pressed = ctx.input(|i| {
            i.pointer
                .primary_pressed()
                .then(|| i.pointer.interact_pos())
                .flatten()
        });
        egui::CentralPanel::default()
            .frame(background)
            .show(ui, |ui| {
                let shown: Vec<usize> = if comparing {
                    vec![pair.0, pair.1]
                } else {
                    vec![active]
                };
                let area = ui.available_rect_before_wrap();
                for (n, &k) in shown.iter().enumerate() {
                    let rect = half(area, n, shown.len());
                    // Compare mode: a strip above each half names its controller.
                    let (strip, body) = if comparing {
                        controller_strip(ui, &theme, rect, k, active, &self.session)
                    } else {
                        (None, rect)
                    };
                    let _ = strip;
                    let Some(data) = self.target_data(k) else {
                        ui.scope_builder(egui::UiBuilder::new().max_rect(body), |ui| {
                            ui.centered_and_justified(|ui| {
                                ui.label(egui::RichText::new("no dataset").color(theme.text_faint));
                            });
                        });
                        continue;
                    };
                    let mut child = ui.new_child(egui::UiBuilder::new().max_rect(body));
                    self.views[k].options = self.options;
                    let target = data.target(&self.session.store);
                    // Disjoint fields: the store is read while the cursor moves.
                    let cursor = &mut self.session.controllers[k].cursor;
                    self.views[k].ui(&mut child, &theme, &target, cursor);
                    self.options = self.views[k].options;
                    let acts = self.views[k].take_actions();
                    // A menu used in the other controller's half acts on it.
                    if k != active && !acts.is_empty() {
                        graph_actions.push(SessionAction::SelectController(k));
                    }
                    graph_actions.extend(acts);
                    if comparing && k != active && pressed.is_some_and(|p| body.contains(p)) {
                        graph_actions.push(SessionAction::SelectController(k));
                    }
                    if comparing && k == active && pressed.is_some_and(|p| body.contains(p)) {
                        // Already active.
                    }
                }
                if shown.is_empty() {
                    ui.centered_and_justified(|ui| {
                        ui.label(egui::RichText::new("no dataset").color(theme.text_faint));
                    });
                }
            });
        self.sync_links();
        self.apply(graph_actions);
        self.view_dialog(&ctx, &theme);
        self.export_dialog(&ctx, &theme);
        action
    }
}

/// Everything a view needs of one controller, owned so that the session can
/// still be borrowed (for the crosshair) while the view is drawn.
struct TargetData {
    ds: std::sync::Arc<Dataset>,
    sub_brick: usize,
    generation: u64,
    layers: Vec<(crate::session::OverlayLayer, std::sync::Arc<Dataset>)>,
    keeps: Vec<Option<std::sync::Arc<Vec<bool>>>>,
    series: crate::session::SeriesSettings,
}

impl TargetData {
    fn target<'a>(&'a self, store: &'a crate::session::DatasetStore) -> Target<'a> {
        Target {
            ds: &self.ds,
            sub_brick: self.sub_brick,
            generation: self.generation,
            overlays: self
                .layers
                .iter()
                .zip(&self.keeps)
                .map(|((layer, ds), keep)| OverlayTarget {
                    layer,
                    ds: ds.as_ref(),
                    keep: keep.clone(),
                })
                .collect(),
            store,
            series: &self.series,
        }
    }
}

/// Half `n` of `count` side-by-side halves of `area` (the whole area for one).
fn half(area: egui::Rect, n: usize, count: usize) -> egui::Rect {
    if count < 2 {
        return area;
    }
    let gap = 8.0;
    let w = (area.width() - gap) / 2.0;
    egui::Rect::from_min_size(
        egui::pos2(area.left() + n as f32 * (w + gap), area.top()),
        egui::vec2(w, area.height()),
    )
}

/// The label strip above a controller's half in compare mode: its letter,
/// the underlay's name, and a mark for the active controller. Returns the
/// strip and the rest of the half.
fn controller_strip(
    ui: &mut egui::Ui,
    theme: &Theme,
    rect: egui::Rect,
    k: usize,
    active: usize,
    session: &Session,
) -> (Option<egui::Rect>, egui::Rect) {
    let strip = egui::Rect::from_min_size(rect.min, egui::vec2(rect.width(), 22.0));
    let body = egui::Rect::from_min_max(egui::pos2(rect.left(), strip.bottom() + 4.0), rect.max);
    let name = session
        .controllers
        .get(k)
        .and_then(|c| c.underlay)
        .and_then(|id| session.store.get(id))
        .map_or("no dataset".to_string(), |d| d.name.clone());
    let is_active = k == active;
    ui.scope_builder(egui::UiBuilder::new().max_rect(strip), |ui| {
        ui.horizontal(|ui| {
            let (chip, _) = ui.allocate_exact_size(egui::vec2(22.0, 18.0), egui::Sense::hover());
            ui.painter().rect_filled(
                chip,
                4.0,
                if is_active {
                    theme.accent
                } else {
                    theme.card_hi
                },
            );
            ui.painter().text(
                chip.center(),
                egui::Align2::CENTER_CENTER,
                Session::controller_name(k),
                egui::FontId::proportional(13.0),
                if is_active {
                    egui::Color32::BLACK
                } else {
                    theme.text
                },
            );
            ui.label(egui::RichText::new(name).color(if is_active {
                theme.text
            } else {
                theme.text_dim
            }));
        });
    });
    (Some(strip), body)
}

/// Storage key for the recently chosen datasets.
const RECENTS_STORAGE_KEY: &str = "afniru_recents";

/// Storage key for the Processing rail's state.
const RAIL_STORAGE_KEY: &str = "afniru_processing_rail";

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        if let Some(action) = self.draw(ui) {
            self.handle(&ui.ctx().clone(), action);
        }
    }

    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        eframe::set_value(storage, controller::STORAGE_KEY, &self.controller);
        eframe::set_value(storage, RAIL_STORAGE_KEY, &self.rail);
        eframe::set_value(storage, RECENTS_STORAGE_KEY, &self.recents);
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use egui::vec2;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::geom::Plane;
    use crate::prefs::{CanvasBackground, ThemeChoice};
    use crate::recent::RecentKind;
    use crate::render::export::{MontageSpec, ViewsLayout};
    use crate::tools::ToolId;

    fn fixture(name: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("tests/fixtures")
            .join(name)
    }

    fn app() -> App {
        App::new(Prefs::default(), &[], false)
    }

    #[test]
    fn starts_empty() {
        let a = app();
        assert!(a.session.store.is_empty());
        assert!(a.error.is_none());
    }

    #[test]
    fn open_shows_the_dataset_and_recenters_the_view() {
        let mut a = app();
        a.open(&fixture("tiny2+orig.HEAD"));
        assert_eq!(a.session.store.len(), 1);
        assert_eq!(a.session.underlay().unwrap().name, "tiny2+orig");
        assert_eq!(a.session.controller().cursor.ijk, [2, 2, 3]); // dims 4×5×6, centered
        assert!(a.error.is_none());
    }

    #[test]
    fn failed_open_keeps_the_current_dataset_and_reports() {
        let mut a = app();
        a.open(&fixture("tiny2+orig.HEAD"));
        let generation = a.session.generation;
        a.open(&fixture("nonexistent+orig.HEAD"));
        assert_eq!(a.session.store.len(), 1);
        assert_eq!(a.session.generation, generation);
        assert!(
            a.error
                .as_deref()
                .is_some_and(|e| e.contains("nonexistent"))
        );
        a.add(synthetic::phantom()); // a later success clears the error
        assert!(a.error.is_none());
    }

    #[test]
    fn first_command_line_path_is_the_underlay() {
        let paths = [fixture("tiny2+orig.HEAD"), fixture("obl+orig.HEAD")];
        let a = App::new(Prefs::default(), &paths, false);
        assert_eq!(a.session.store.len(), 2);
        assert_eq!(a.session.underlay().unwrap().name, "tiny2+orig");
    }

    #[test]
    fn demo_flag_opens_the_phantom() {
        let a = App::new(Prefs::default(), &[], true);
        assert_eq!(a.session.underlay().unwrap().name, "phantom");
    }

    #[test]
    fn each_new_dataset_bumps_the_generation() {
        let mut a = app();
        let g = a.session.generation;
        a.add(synthetic::phantom());
        a.add(synthetic::phantom());
        assert_eq!(a.session.generation, g + 2);
    }

    /// Render `App::draw` (menu, toolbar, controller, views, readout, status
    /// bar). `setup` can change the app before the first frame; `click`
    /// clicks a widget by label afterwards.
    fn snapshot(name: &str, mut app: App, size: egui::Vec2, click: Option<&str>) {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(size)
            .build_ui(move |ui| {
                app.draw(ui);
            });
        harness.run();
        if let Some(label) = click {
            harness.get_by_label(label).click();
            harness.run();
        }
        harness.snapshot(name);
    }

    fn prefs(theme: ThemeChoice, canvas: CanvasBackground) -> Prefs {
        Prefs {
            theme,
            canvas,
            ..Prefs::default()
        }
    }

    fn demo(theme: ThemeChoice) -> App {
        App::new(prefs(theme, CanvasBackground::Black), &[], true)
    }

    #[test]
    fn shell_empty() {
        let app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        snapshot("shell_empty_dark", app, vec2(1000.0, 560.0), None);
    }

    #[test]
    fn shell_demo_dark() {
        snapshot(
            "shell_demo_dark",
            demo(ThemeChoice::Dark),
            vec2(1300.0, 800.0),
            None,
        );
    }

    #[test]
    fn shell_demo_light() {
        snapshot(
            "shell_demo_light",
            demo(ThemeChoice::Light),
            vec2(1300.0, 800.0),
            None,
        );
    }

    #[test]
    fn shell_file_menu_open() {
        snapshot(
            "shell_file_menu",
            demo(ThemeChoice::Dark),
            vec2(1000.0, 600.0),
            Some("File"),
        );
    }

    #[test]
    fn shell_error_in_status_bar() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.open(Path::new("/nonexistent/anat+orig.HEAD"));
        snapshot("shell_error", app, vec2(1000.0, 560.0), None);
    }

    #[test]
    fn shell_cards_folded_and_a_hidden_tool() {
        let mut app = demo(ThemeChoice::Dark);
        let ws = app.controller.workspaces.current_mut();
        ws.toggle_collapsed(ToolId::Datasets);
        ws.toggle_collapsed(ToolId::Crosshair);
        snapshot("controller_folded", app, vec2(1000.0, 600.0), None);
    }

    #[test]
    fn shell_crosshair_hidden_shows_an_off_tile() {
        let mut app = demo(ThemeChoice::Dark);
        app.controller
            .workspaces
            .current_mut()
            .close(ToolId::Crosshair);
        snapshot("controller_crosshair_off", app, vec2(1000.0, 600.0), None);
    }

    #[test]
    fn shell_rail() {
        let mut app = demo(ThemeChoice::Dark);
        app.controller.rail = true;
        snapshot("controller_rail", app, vec2(1000.0, 600.0), None);
    }

    #[test]
    fn shell_rail_popover() {
        let mut app = demo(ThemeChoice::Dark);
        app.controller.rail = true;
        app.controller.popover = Some((ToolId::Crosshair, 120.0));
        snapshot("controller_rail_popover", app, vec2(1000.0, 600.0), None);
    }

    #[test]
    fn clicking_a_card_title_folds_it() {
        let app = demo(ThemeChoice::Dark);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1000.0, 1000.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        // The card's title (a tile of the shelf has the same name).
        harness
            .get_all_by_label_contains("Crosshair")
            .find(|n| {
                egui_kittest::kittest::NodeT::accesskit_node(n)
                    .label()
                    .is_some_and(|l| l.contains("  "))
            })
            .unwrap()
            .click();
        harness.run();
        let state = harness
            .state()
            .controller
            .workspaces
            .current()
            .state(ToolId::Crosshair)
            .unwrap();
        assert!(state.collapsed);
    }

    #[test]
    fn the_afniru_file_can_start_afniru_in_neurological_view() {
        // `AFNI_LEFT_IS_LEFT = YES` in ~/.afniru, as parsed by the preferences.
        let p = Prefs::parse("***ENVIRONMENT\n AFNI_LEFT_IS_LEFT = YES\n");
        let app = App::new(p, &[], true);
        assert!(app.options.left_is_left);
        assert!(app.views[0].conventions(None).contains("neurological"));
        let harness = run_frames(app, vec2(1000.0, 700.0));
        assert!(harness.query_all_by_label("L↔R").next().is_some());
        // Without it, the default is radiological.
        let app = demo(ThemeChoice::Dark);
        assert!(!app.options.left_is_left);
        assert!(app.views[0].conventions(None).contains("radiological"));
    }

    #[test]
    fn the_left_right_button_flips_its_label_and_the_status_bar_follows() {
        let app = demo(ThemeChoice::Dark);
        let mut harness = run_frames(app, vec2(1000.0, 700.0));
        assert!(!harness.state().options.left_is_left);
        harness.get_by_label("R↔L").click();
        harness.run();
        assert!(harness.state().options.left_is_left);
        harness.get_by_label("L↔R").click(); // the button now reads L↔R
        harness.run();
        assert!(!harness.state().options.left_is_left);
        assert!(harness.query_all_by_label("R↔L").next().is_some());
        assert!(harness.query_all_by_label("L↔R").next().is_none());
    }

    #[test]
    fn collapse_button_switches_to_the_rail_and_expand_button_back() {
        let app = demo(ThemeChoice::Dark);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1000.0, 600.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        harness
            .get_by_label(egui_phosphor::regular::CARET_DOUBLE_LEFT)
            .click();
        harness.run();
        assert!(harness.state().controller.rail);
        harness
            .get_by_label(egui_phosphor::regular::CARET_DOUBLE_RIGHT)
            .click();
        harness.run();
        assert!(!harness.state().controller.rail);
    }

    #[test]
    fn crosshair_card_moves_the_crosshair_through_the_session() {
        let mut app = demo(ThemeChoice::Dark);
        app.session
            .apply(SessionAction::MoveCrosshair([10, 20, 30]));
        let before = app.session.controller().cursor.ijk;
        app.apply(vec![SessionAction::JumpToRas([60.0, 70.0, -20.0])]);
        assert_ne!(app.session.controller().cursor.ijk, before);
        assert_eq!(app.session.controller().cursor.ijk, [15, 20, 55]);
    }

    // ---- Overlay layers ----

    /// The ids of the layers, bottom first.
    fn layer_ids(app: &App) -> Vec<crate::session::LayerId> {
        app.session
            .controller()
            .overlays
            .iter()
            .map(|l| l.id)
            .collect()
    }

    /// Change the bottom layer.
    fn overlay_change(app: &mut App, c: OverlayChange) {
        let id = layer_ids(app)[0];
        app.apply(vec![SessionAction::Layer(id, c)]);
    }

    /// Add `ds` as a new top layer and return its id.
    fn add_layer(app: &mut App, ds: crate::data::Dataset) -> crate::session::LayerId {
        let id = app.session.store.add(ds);
        app.apply(vec![SessionAction::AddOverlay(id)]);
        *layer_ids(app).last().unwrap()
    }

    #[test]
    fn the_demo_has_a_tmap_overlay_thresholded_at_3_1() {
        let a = demo(ThemeChoice::Dark);
        let layers = a.session.overlay_layers();
        assert_eq!(layers.len(), 1);
        let (l, ds) = &layers[0];
        assert_eq!(
            l.colorscale,
            prefs(ThemeChoice::Dark, CanvasBackground::Black).colorscale
        );
        assert_eq!(l.threshold, 3.1);
        let p = l.p_value(ds).unwrap();
        assert!((p - 0.00242029).abs() < 1e-6, "{p}"); // `cdf -t2p fitt 3.1 118`
    }

    #[test]
    fn later_command_line_datasets_become_layers_in_order() {
        let paths = [
            fixture("tiny2+orig.HEAD"),
            fixture("stat+orig.HEAD"),
            fixture("obl+orig.HEAD"),
        ];
        let a = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &paths,
            false,
        );
        assert_eq!(a.session.underlay().unwrap().name, "tiny2+orig");
        let names: Vec<_> = a
            .session
            .overlay_layers()
            .iter()
            .map(|(_, d)| d.name.clone())
            .collect();
        assert_eq!(names, ["stat+orig", "obl+orig"]); // the last is on top
        assert_eq!(a.session.controller().overlays[0].threshold, 0.0);
    }

    #[test]
    fn a_single_command_line_dataset_has_no_overlay() {
        let a = App::new(Prefs::default(), &[fixture("tiny2+orig.HEAD")], false);
        assert!(a.session.controller().overlays.is_empty());
    }

    #[test]
    fn hiding_a_layer_keeps_its_settings() {
        let mut app = demo(ThemeChoice::Dark);
        overlay_change(&mut app, OverlayChange::Threshold(4.2));
        overlay_change(&mut app, OverlayChange::Visible(false));
        let l = &app.session.controller().overlays[0];
        assert!(!l.visible && l.threshold == 4.2);
    }

    // -- looks --

    #[test]
    fn snapshot_overlay_positive_only_and_boxed() {
        let mut app = demo(ThemeChoice::Dark);
        overlay_change(&mut app, OverlayChange::Signed(false));
        overlay_change(&mut app, OverlayChange::Boxed(true));
        overlay_change(&mut app, OverlayChange::Threshold(3.0));
        snapshot("overlay_positive_boxed", app, vec2(1300.0, 1000.0), None);
    }

    #[test]
    fn snapshot_overlay_faded_with_a_fixed_range_and_reduced_opacity() {
        let mut app = demo(ThemeChoice::Dark);
        overlay_change(&mut app, OverlayChange::Fade(true));
        overlay_change(&mut app, OverlayChange::Range(Some(10.0)));
        overlay_change(&mut app, OverlayChange::Opacity(0.7));
        overlay_change(&mut app, OverlayChange::Threshold(4.0));
        overlay_change(
            &mut app,
            OverlayChange::ColorScale(afni_core::afni_colors::AfniColorScale::RedsAndBlues),
        );
        snapshot(
            "overlay_faded_range_opacity",
            app,
            vec2(1300.0, 1000.0),
            None,
        );
    }

    #[test]
    fn snapshot_no_overlay_layers() {
        let mut app = demo(ThemeChoice::Dark);
        let id = layer_ids(&app)[0];
        app.apply(vec![SessionAction::RemoveOverlay(id)]);
        snapshot("overlay_none", app, vec2(1000.0, 700.0), None);
    }

    #[test]
    fn snapshot_overlay_from_a_real_stat_dataset_shows_p_and_q() {
        let paths = [fixture("tiny2+orig.HEAD"), fixture("stat+orig.HEAD")];
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &paths,
            false,
        );
        overlay_change(&mut app, OverlayChange::Threshold(3.1));
        snapshot("overlay_real_stat", app, vec2(1300.0, 1000.0), None);
    }

    #[test]
    fn snapshot_overlay_light_theme() {
        snapshot(
            "overlay_light",
            demo(ThemeChoice::Light),
            vec2(1300.0, 1000.0),
            None,
        );
    }

    /// Two layers: the t-map, and its mirror on top in blues and reds at 70%.
    fn two_layers() -> App {
        let mut app = demo(ThemeChoice::Dark);
        let top = add_layer(&mut app, synthetic::mirrored_tmap());
        app.apply(vec![
            SessionAction::Layer(
                top,
                OverlayChange::ColorScale(afni_core::afni_colors::AfniColorScale::RedsAndBlues),
            ),
            SessionAction::Layer(top, OverlayChange::Opacity(0.7)),
            SessionAction::Layer(top, OverlayChange::Threshold(4.5)),
        ]);
        app
    }

    #[test]
    fn snapshot_two_layers_with_two_cards_and_a_layer_list() {
        snapshot(
            "overlay_two_layers",
            two_layers(),
            vec2(1500.0, 1400.0),
            None,
        );
    }

    #[test]
    fn snapshot_two_layers_one_hidden_one_folded() {
        let mut app = two_layers();
        let ids = layer_ids(&app);
        app.apply(vec![SessionAction::Layer(
            ids[0],
            OverlayChange::Visible(false),
        )]);
        app.controller
            .instance_collapsed
            .insert((ToolId::Overlay, ids[1].0), true);
        snapshot(
            "overlay_two_layers_hidden_folded",
            app,
            vec2(1300.0, 1000.0),
            None,
        );
    }

    #[test]
    fn snapshot_two_boxed_layers_draw_their_outlines_over_the_fills() {
        let mut app = two_layers();
        let ids = layer_ids(&app);
        // The bottom layer is boxed, the top one filled: the outline still shows on top.
        app.apply(vec![SessionAction::Layer(
            ids[0],
            OverlayChange::Boxed(true),
        )]);
        snapshot(
            "overlay_two_layers_boxed_bottom",
            app,
            vec2(1300.0, 1000.0),
            None,
        );
    }

    #[test]
    fn snapshot_the_crosshair_card_lists_every_layers_value_with_a_swatch() {
        let mut app = two_layers();
        // Put the crosshair on the t-map's peak.
        let frame = synthetic::tmap().frame(0).unwrap();
        let peak = (0..frame.len())
            .max_by(|a, b| frame[*a].total_cmp(&frame[*b]))
            .unwrap();
        let [nx, ny, _] = [150, 180, 150];
        app.apply(vec![SessionAction::MoveCrosshair([
            peak % nx,
            (peak / nx) % ny,
            peak / (nx * ny),
        ])]);
        let ids = layer_ids(&app);
        app.controller
            .instance_collapsed
            .insert((ToolId::Overlay, ids[0].0), true);
        app.controller
            .instance_collapsed
            .insert((ToolId::Overlay, ids[1].0), true);
        app.controller
            .workspaces
            .current_mut()
            .toggle_collapsed(ToolId::Datasets);
        snapshot("overlay_crosshair_values", app, vec2(1300.0, 900.0), None);
    }

    #[test]
    fn each_layer_has_its_own_card_titled_with_its_number_and_dataset() {
        let app = two_layers();
        let ids = layer_ids(&app);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1300.0, 1400.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        harness.get_by_label_contains(&format!("Overlay {} · tmap_mirror", ids[1].0));
        harness.get_by_label_contains(&format!("Overlay {} · tmap", ids[0].0));
    }

    #[test]
    fn clicking_a_layer_card_title_folds_only_that_card() {
        let app = two_layers();
        let ids = layer_ids(&app);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1300.0, 1400.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        harness
            .get_by_label_contains(&format!("Overlay {} · tmap_mirror", ids[1].0))
            .click();
        harness.run();
        let c = &harness.state().controller.instance_collapsed;
        assert_eq!(c.get(&(ToolId::Overlay, ids[1].0)), Some(&true));
        assert_eq!(c.get(&(ToolId::Overlay, ids[0].0)), None);
    }

    #[test]
    fn the_eye_hides_the_layer_it_is_on_and_the_trash_removes_it() {
        let app = two_layers();
        let ids = layer_ids(&app);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1300.0, 1400.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        // Rows are listed top layer first.
        harness
            .get_all_by_label(egui_phosphor::regular::EYE)
            .next()
            .unwrap()
            .click();
        harness.run();
        let layers = &harness.state().session.controller().overlays;
        assert!(layers[0].visible && !layers[1].visible);
        harness
            .get_all_by_label(egui_phosphor::regular::TRASH)
            .next()
            .unwrap()
            .click();
        harness.run();
        assert_eq!(layer_ids(harness.state()), [ids[0]]);
    }

    #[test]
    fn dragging_a_layer_handle_restacks_the_layers() {
        let app = two_layers();
        let before = layer_ids(&app); // bottom first: [1, 2]
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1300.0, 1400.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        // Handles in order: the Datasets card's, then the layer rows (top
        // layer first), then the cards'. Take the bottom layer's row handle
        // and the top layer's row position.
        let handles: Vec<_> = harness
            .get_all_by_label(egui_phosphor::regular::DOTS_SIX_VERTICAL)
            .collect();
        let bottom = handles[2].rect().center();
        let top_row = handles[1].rect().center();
        harness.hover_at(bottom);
        harness.drag_at(bottom);
        harness.run();
        harness.hover_at(egui::pos2(top_row.x, top_row.y - 12.0));
        harness.run();
        harness.drop_at(egui::pos2(top_row.x, top_row.y - 12.0));
        harness.run();
        let after = layer_ids(harness.state());
        assert_eq!(
            after,
            [before[1], before[0]],
            "the dragged layer is now on top"
        );
    }

    #[test]
    fn add_overlay_from_the_datasets_card_adds_a_layer_on_top() {
        let mut app = demo(ThemeChoice::Dark);
        app.session.store.add(synthetic::mirrored_tmap());
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1300.0, 1200.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        harness.get_by_label_contains("Add overlay").click();
        harness.run();
        harness.get_by_label("tmap_mirror").click();
        harness.run();
        let names: Vec<_> = harness
            .state()
            .session
            .overlay_layers()
            .iter()
            .map(|(_, d)| d.name.clone())
            .collect();
        assert_eq!(names, ["tmap", "tmap_mirror"]);
    }

    // ---- Mask layers ----

    use crate::session::overlay::{Binding, MaskRule};

    /// Layers 1 (the t-map) and 2 (its mirror) as masks "t > 4", and layer 3
    /// as the rule `a*b` over where each is drawn. Returns the app and ids.
    fn three_masks() -> (App, [crate::session::LayerId; 3]) {
        let mut app = demo(ThemeChoice::Dark);
        let first = layer_ids(&app)[0];
        let second = add_layer(&mut app, synthetic::mirrored_tmap());
        let third = add_layer(&mut app, synthetic::phantom());
        let mut actions = Vec::new();
        for (id, thr) in [(first, 4.0), (second, 4.0)] {
            actions.push(SessionAction::Layer(id, OverlayChange::MaskMode(true)));
            actions.push(SessionAction::Layer(id, OverlayChange::Threshold(thr)));
        }
        actions.extend([
            SessionAction::Layer(third, OverlayChange::MaskMode(true)),
            SessionAction::Layer(
                third,
                OverlayChange::MaskRule(MaskRule::Expression("a*b".into())),
            ),
            SessionAction::Layer(
                third,
                OverlayChange::Bind('a', Some(Binding::LayerMask(first))),
            ),
            SessionAction::Layer(
                third,
                OverlayChange::Bind('b', Some(Binding::LayerMask(second))),
            ),
        ]);
        app.apply(actions);
        (app, [first, second, third])
    }

    /// The voxel (index) of the first value that satisfies `want(t, mirror)`.
    fn voxel_where(want: impl Fn(f32, f32) -> bool) -> [usize; 3] {
        let a = synthetic::tmap().frame(0).unwrap();
        let b = synthetic::mirrored_tmap().frame(0).unwrap();
        let n = (0..a.len())
            .find(|&n| want(a[n], b[n]))
            .expect("such a voxel exists");
        [n % 150, (n / 150) % 180, n / (150 * 180)]
    }

    /// Run one frame so the views build their data, then probe the crosshair.
    fn probe_at(app: App, ijk: [usize; 3]) -> Vec<Option<afni_core::color::Rgba>> {
        let mut app = app;
        app.apply(vec![SessionAction::MoveCrosshair(ijk)]);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1300.0, 1000.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        let app = harness.state();
        let layers: Vec<_> = app
            .session
            .overlay_layers()
            .into_iter()
            .map(|(l, _)| l)
            .collect();
        let under = app.session.underlay().unwrap().clone();
        app.views[0]
            .probe(&under, &layers, &app.session.controller().cursor)
            .into_iter()
            .map(|p| p.drawn)
            .collect()
    }

    #[test]
    fn masks_give_where_a_is_where_b_is_and_where_both_are() {
        let only_a = voxel_where(|a, b| a > 4.0 && b < 1.0 && b > -1.0);
        let only_b = voxel_where(|a, b| b > 4.0 && a < 1.0 && a > -1.0);
        let both = voxel_where(|a, b| a > 4.0 && b > 4.0);
        let neither = voxel_where(|a, b| a.abs() < 1.0 && b.abs() < 1.0 && a != 0.0 && b != 0.0);
        let colors = |ijk| probe_at(three_masks().0, ijk);
        let shown = |v: &[Option<afni_core::color::Rgba>]| {
            v.iter().map(Option::is_some).collect::<Vec<_>>()
        };
        assert_eq!(shown(&colors(only_a)), [true, false, false], "only A");
        assert_eq!(shown(&colors(only_b)), [false, true, false], "only B");
        assert_eq!(shown(&colors(both)), [true, true, true], "both");
        assert_eq!(shown(&colors(neither)), [false, false, false], "neither");
        // Every "on" voxel of a layer is exactly that layer's one color.
        let c = colors(both);
        let (a, b, ab) = (c[0].unwrap(), c[1].unwrap(), c[2].unwrap());
        assert_ne!(a, b);
        assert_ne!(a, ab);
        assert_eq!(c.iter().flatten().filter(|x| x.a == 1.0).count(), 3);
    }

    #[test]
    fn a_hidden_layer_still_feeds_the_rule() {
        let (mut app, [first, second, third]) = three_masks();
        app.apply(vec![
            SessionAction::Layer(first, OverlayChange::Visible(false)),
            SessionAction::Layer(second, OverlayChange::Visible(false)),
        ]);
        let both = voxel_where(|a, b| a > 4.0 && b > 4.0);
        let colors = probe_at(app, both);
        assert_eq!(
            colors.iter().map(Option::is_some).collect::<Vec<_>>(),
            [false, false, true]
        );
        let _ = third;
    }

    #[test]
    fn a_rule_can_read_a_dataset_sub_brick_and_coordinates() {
        let (mut app, [_, _, third]) = three_masks();
        let mirror = app.session.overlay_layers()[1].0.dataset;
        app.apply(vec![
            SessionAction::Layer(
                third,
                OverlayChange::MaskRule(MaskRule::Expression("step(c-4)*step(30-abs(x))".into())),
            ),
            SessionAction::Layer(
                third,
                OverlayChange::Bind(
                    'c',
                    Some(Binding::Sub {
                        dataset: mirror,
                        sub: 0,
                    }),
                ),
            ),
        ]);
        let on = voxel_where(|_, b| b > 4.0);
        let drawn = |app: App| probe_at(app, on)[2].is_some();
        // |x| < 30 mm only near the midline: the mirror's blobs may or may not be there.
        let ds = synthetic::phantom();
        let x = -crate::geom::coords::ijk_to_ras(&ds.ijk_to_ras, on)[0];
        assert_eq!(drawn(app), x.abs() < 30.0, "x = {x}");
    }

    #[test]
    fn switching_a_layer_to_mask_and_typing_a_rule_goes_through_the_card() {
        use egui::accesskit::Role;
        let app = demo(ThemeChoice::Dark);
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1300.0, 1400.0))
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        harness.get_by_label("Mask").click();
        harness.run();
        assert!(harness.state().session.overlay_layers()[0].0.as_mask);
        harness.get_by_label("rule").click();
        harness.run();
        // The rule starts as `step(a-<threshold>)`.
        let text = match &harness.state().session.overlay_layers()[0].0.mask.rule {
            MaskRule::Expression(t) => t.clone(),
            other => panic!("{other:?}"),
        };
        assert_eq!(text, "step(a-3.1)");
        let input = harness.get_by_role(Role::TextInput);
        input.focus();
        harness.run();
        harness.get_by_role(Role::TextInput).type_text("*x");
        harness.run();
        let rule = &harness.state().session.overlay_layers()[0].0.mask.rule;
        assert_eq!(rule, &MaskRule::Expression("step(a-3.1)*x".into()));
    }

    #[test]
    fn snapshot_three_masks_a_b_and_both() {
        let (app, _) = three_masks();
        snapshot("masks_a_b_both", app, vec2(1500.0, 1500.0), None);
    }

    #[test]
    fn snapshot_only_the_conjunction_visible() {
        let (mut app, [first, second, _]) = three_masks();
        app.apply(vec![
            SessionAction::Layer(first, OverlayChange::Visible(false)),
            SessionAction::Layer(second, OverlayChange::Visible(false)),
        ]);
        snapshot("masks_only_both", app, vec2(1500.0, 1500.0), None);
    }

    #[test]
    fn snapshot_a_rule_with_a_mistake_says_what_is_wrong() {
        let (mut app, [_, _, third]) = three_masks();
        app.apply(vec![SessionAction::Layer(
            third,
            OverlayChange::MaskRule(MaskRule::Expression("step(a>3)".into())),
        )]);
        snapshot("masks_rule_error", app, vec2(1300.0, 1500.0), None);
    }

    #[test]
    fn snapshot_a_rule_with_an_unbound_letter_says_so() {
        let (mut app, [_, _, third]) = three_masks();
        app.apply(vec![SessionAction::Layer(
            third,
            OverlayChange::MaskRule(MaskRule::Expression("a*c".into())),
        )]);
        snapshot("masks_unbound_letter", app, vec2(1300.0, 1500.0), None);
    }

    #[test]
    fn snapshot_a_threshold_mask_card() {
        let (app, _) = three_masks();
        snapshot("masks_threshold_card", app, vec2(1300.0, 1500.0), None);
    }

    // ---- Saving images ----

    fn export_app() -> (App, crate::testutil::TempDir) {
        let (mut app, _) = clusterize_app();
        // The crosshair at a known voxel.
        app.apply(vec![SessionAction::MoveCrosshair([1, 2, 3])]);
        (app, crate::testutil::TempDir::new("export"))
    }

    fn plain() -> ExportOptions {
        ExportOptions {
            zoom: 4,
            letters: false,
            crosshair: false,
            label: SliceLabel::default(),
            graph: false,
        }
    }

    fn png(path: &Path) -> image::RgbaImage {
        image::open(path).unwrap().to_rgba8()
    }

    #[test]
    fn a_saved_slice_has_square_pixels_at_the_chosen_zoom() {
        let (mut app, dir) = export_app();
        // 4x5x6 voxels of 2x2x3 mm; the smallest edge (2 mm) is 4 pixels wide.
        let axial = dir.path().join("a.png");
        app.export_to(ExportWhat::Slice(Plane::Axial), &plain(), &axial);
        let img = png(&axial);
        assert_eq!((img.width(), img.height()), (4 * 2 * 2, 5 * 2 * 2)); // 16 x 20
        // A coronal slice is 4 voxels across and 6 slices (3 mm each) tall.
        let coronal = dir.path().join("c.png");
        app.export_to(ExportWhat::Slice(Plane::Coronal), &plain(), &coronal);
        let img = png(&coronal);
        assert_eq!((img.width(), img.height()), (16, 6 * 3 * 2)); // 16 x 36
        assert!(
            app.notice
                .as_deref()
                .is_some_and(|n| n.starts_with("Saved"))
        );
        assert!(app.error.is_none());
    }

    #[test]
    fn zoom_scales_every_voxel_to_a_block() {
        let (mut app, dir) = export_app();
        let mut opts = plain();
        opts.zoom = 1;
        let path = dir.path().join("small.png");
        app.export_to(ExportWhat::Slice(Plane::Axial), &opts, &path);
        let small = png(&path);
        opts.zoom = 3;
        let path = dir.path().join("big.png");
        app.export_to(ExportWhat::Slice(Plane::Axial), &opts, &path);
        let big = png(&path);
        assert_eq!((small.width(), small.height()), (4, 5)); // 1 px per 2 mm
        assert_eq!((big.width(), big.height()), (12, 15));
        // Each voxel is a flat block: no smoothing between pixels.
        assert_eq!(big.get_pixel(0, 0), big.get_pixel(2, 2)); // one voxel, 3x3 pixels
        assert_eq!(big.get_pixel(0, 0), big.get_pixel(2, 0));
    }

    #[test]
    fn the_three_views_save_as_a_row_a_column_a_grid_or_separate_files() {
        let (mut app, dir) = export_app();
        let mut size = |what, name: &str| {
            let path = dir.path().join(name);
            app.export_to(what, &plain(), &path);
            path
        };
        // Tiles: axial 16x20, sagittal 20x36 (5 across, 6x3 mm down), coronal 16x36.
        let row = png(&size(ExportWhat::Views(ViewsLayout::Row), "row.png"));
        let gap = 8;
        assert_eq!((row.width(), row.height()), (3 * 20 + 2 * gap, 36));
        let column = png(&size(ExportWhat::Views(ViewsLayout::Column), "col.png"));
        assert_eq!((column.width(), column.height()), (20, 3 * 36 + 2 * gap));
        let grid = png(&size(ExportWhat::Views(ViewsLayout::Grid), "grid.png"));
        assert_eq!((grid.width(), grid.height()), (2 * 20 + gap, 2 * 36 + gap));
        // Separate files are named after the chosen one.
        let _ = size(ExportWhat::Views(ViewsLayout::Individual), "fig.png");
        for view in ["axial", "sagittal", "coronal"] {
            assert!(
                dir.path().join(format!("fig_{view}.png")).is_file(),
                "{view}"
            );
        }
        assert!(!dir.path().join("fig.png").exists());
    }

    #[test]
    fn a_montage_lays_slices_out_left_to_right_and_top_to_bottom() {
        let (mut app, dir) = export_app();
        let spec = MontageSpec {
            plane: Plane::Axial,
            rows: 2,
            cols: 3,
            first: 0,
            last: 5,
            step: 1,
        };
        let path = dir.path().join("m.png");
        app.export_to(ExportWhat::Montage(spec), &plain(), &path);
        let img = png(&path);
        let gap = 4; // zoom pixels between tiles
        assert_eq!(
            (img.width(), img.height()),
            (3 * 16 + 2 * gap, 2 * 20 + gap)
        );
        // Fewer slices than tiles use only the rows they need.
        let short = MontageSpec { last: 2, ..spec };
        let path = dir.path().join("short.png");
        app.export_to(ExportWhat::Montage(short), &plain(), &path);
        let img = png(&path);
        assert_eq!((img.width(), img.height()), (3 * 16 + 2 * gap, 20));
        // A range with no slices is an error, not an empty file.
        let none = MontageSpec {
            first: 9,
            last: 9,
            ..spec
        };
        app.export_to(
            ExportWhat::Montage(none),
            &plain(),
            &dir.path().join("none.png"),
        );
        assert!(app.error.is_some());
        assert!(!dir.path().join("none.png").exists());
    }

    #[test]
    fn orientation_letters_the_slice_number_and_the_crosshair_are_drawn_when_asked() {
        let (mut app, dir) = export_app();
        let base_path = dir.path().join("base.png");
        app.export_to(ExportWhat::Slice(Plane::Axial), &plain(), &base_path);
        let base = png(&base_path);
        // Letters make the picture bigger.
        let mut opts = plain();
        opts.letters = true;
        let p = dir.path().join("letters.png");
        app.export_to(ExportWhat::Slice(Plane::Axial), &opts, &p);
        let with_letters = png(&p);
        assert!(with_letters.width() > base.width() && with_letters.height() > base.height());
        // The slice number adds white pixels in its corner; off, none.
        let mut opts = plain();
        opts.label = SliceLabel {
            show: true,
            corner: crate::render::label::Corner::BottomRight,
            size: crate::render::label::LabelSize::ExtraLarge,
        };
        let p = dir.path().join("number.png");
        app.export_to(ExportWhat::Slice(Plane::Axial), &opts, &p);
        let numbered = png(&p);
        // The number changes the bottom right corner and leaves the top left alone.
        let differs = |x0: u32, x1: u32, y0: u32, y1: u32| {
            (y0..y1).any(|y| (x0..x1).any(|x| numbered.get_pixel(x, y) != base.get_pixel(x, y)))
        };
        let (w, h) = (base.width(), base.height());
        assert!(differs(w / 2, w, h / 2, h));
        assert!(!differs(0, w / 4, 0, h / 4));
        assert_eq!(
            (numbered.width(), numbered.height()),
            (base.width(), base.height())
        );
        // The crosshair changes the picture without changing its size.
        let mut opts = plain();
        opts.crosshair = true;
        let p = dir.path().join("cross.png");
        app.export_to(ExportWhat::Slice(Plane::Axial), &opts, &p);
        let crossed = png(&p);
        assert_eq!(
            (crossed.width(), crossed.height()),
            (base.width(), base.height())
        );
        assert_ne!(crossed.as_raw(), base.as_raw());
    }

    #[test]
    fn the_saved_picture_shows_the_overlay_like_the_screen_does() {
        let (mut app, dir) = export_app();
        let path = dir.path().join("with.png");
        app.export_to(ExportWhat::Slice(Plane::Axial), &plain(), &path);
        let with = png(&path);
        // Hide the overlay and save again: the pictures differ.
        let id = layer_ids(&app)[0];
        app.apply(vec![SessionAction::Layer(
            id,
            OverlayChange::Visible(false),
        )]);
        let path = dir.path().join("without.png");
        app.export_to(ExportWhat::Slice(Plane::Axial), &plain(), &path);
        assert_ne!(with.as_raw(), png(&path).as_raw());
    }

    #[test]
    fn saving_with_no_dataset_is_an_error() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        let dir = crate::testutil::TempDir::new("none");
        app.export_to(
            ExportWhat::Slice(Plane::Axial),
            &plain(),
            &dir.path().join("x.png"),
        );
        assert!(app.error.is_some());
    }

    #[test]
    fn the_white_canvas_pref_gives_a_white_background_to_montages_and_views() {
        let (mut app, dir) = export_app();
        app.prefs.canvas = CanvasBackground::White;
        let path = dir.path().join("w.png");
        app.export_to(ExportWhat::Views(ViewsLayout::Grid), &plain(), &path);
        let img = png(&path);
        // The empty fourth cell (the Graph's) is the background.
        assert_eq!(
            img.get_pixel(img.width() - 1, img.height() - 1).0,
            [255, 255, 255, 255]
        );
    }

    /// Right-click at `pos` (press and release of the secondary button).
    fn right_click(harness: &mut egui_kittest::Harness<'_, App>, pos: egui::Pos2) {
        harness.hover_at(pos);
        harness.run();
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Secondary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run();
        }
    }

    #[test]
    fn the_right_click_menu_turns_the_slice_number_on_for_every_view() {
        let (app, _) = clusterize_app();
        let mut harness = run_frames(app, vec2(1300.0, 900.0));
        assert!(!harness.state().options.slice_label.show);
        // Right-click the sagittal image (any view will do).
        let header = harness.get_all_by_value("Sagittal").next().unwrap().rect();
        right_click(&mut harness, header.center() + vec2(0.0, 120.0));
        harness.get_by_label("Slice number").click();
        harness.run();
        let label = harness.state().options.slice_label;
        assert!(label.show);
        // The setting is the views', not one card's: the axial card draws it too.
        harness.snapshot("slice_numbers_on_all_views");
    }

    #[test]
    fn the_menu_moves_and_resizes_the_number() {
        use crate::render::label::{Corner, LabelSize};
        let (app, _) = clusterize_app();
        let mut harness = run_frames(app, vec2(1300.0, 900.0));
        let header = harness.get_all_by_value("Axial").next().unwrap().rect();
        right_click(&mut harness, header.center() + vec2(0.0, 120.0));
        harness.get_by_label_contains("Number position").click();
        harness.run();
        harness.get_by_label("bottom right").click();
        harness.run();
        let l = harness.state().options.slice_label;
        assert_eq!(l.corner, Corner::BottomRight);
        assert!(l.show, "choosing a position turns the number on");
        right_click(&mut harness, header.center() + vec2(0.0, 120.0));
        harness.get_by_label_contains("Number size").click();
        harness.run();
        harness.get_by_label("extra large").click();
        harness.run();
        assert_eq!(
            harness.state().options.slice_label.size,
            LabelSize::ExtraLarge
        );
    }

    #[test]
    fn the_menu_opens_the_save_dialog_and_the_dialog_keeps_its_choices() {
        let (app, _) = clusterize_app();
        let mut harness = run_frames(app, vec2(1300.0, 900.0));
        let header = harness.get_all_by_value("Coronal").next().unwrap().rect();
        right_click(&mut harness, header.center() + vec2(0.0, 100.0));
        harness.get_by_label("Montage and more options…").click();
        harness.run();
        harness.run();
        let dialog = harness
            .state()
            .export_dialog
            .clone()
            .expect("the dialog is open");
        assert_eq!(dialog.plane, Plane::Coronal);
        assert_eq!(dialog.count, 5); // y has 5 slices in clust+orig
        // The three views, as a column: pick it in the dialog.
        harness.get_by_label("The three views").click();
        harness.run();
        harness.get_by_label("one column").click();
        harness.run();
        let dialog = harness.state().export_dialog.clone().unwrap();
        assert_eq!(dialog.what(), ExportWhat::Views(ViewsLayout::Column));
        harness.get_by_label("Cancel").click();
        harness.run();
        assert!(harness.state().export_dialog.is_none());
    }

    // ---- Dropdowns: filter, recent, Graph datasets ----

    fn app_with_many_datasets() -> (App, crate::testutil::TempDir) {
        let dir = crate::testutil::TempDir::new("many");
        for n in 0..15 {
            std::fs::write(dir.path().join(format!("filler{n:02}.nii")), b"x").unwrap();
        }
        std::fs::write(dir.path().join("zz_target.nii"), b"x").unwrap();
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.add_folder(dir.path());
        (app, dir)
    }

    #[test]
    fn clicking_and_typing_in_the_filter_box_keeps_the_dropdown_open() {
        use egui::accesskit::Role;
        let (app, _dir) = app_with_many_datasets();
        let mut harness = run_frames(app, vec2(1000.0, 900.0));
        harness.get_by_value("choose a dataset").click();
        harness.run();
        assert!(harness.query_all_by_label("filler03.nii").next().is_some());
        // Click in the filter box: the list must stay.
        let filter = harness.get_by_role(Role::TextInput);
        filter.click();
        harness.run();
        assert!(
            harness.query_all_by_label("filler03.nii").next().is_some(),
            "the dropdown closed when the filter box was clicked"
        );
        harness.get_by_role(Role::TextInput).type_text("zz_");
        harness.run();
        harness.run();
        assert!(harness.query_all_by_label("zz_target.nii").next().is_some());
        assert!(harness.query_all_by_label("filler03.nii").next().is_none());
        // Choosing an entry closes the list and loads it (a fake file: it fails).
        harness.get_by_label("zz_target.nii").click();
        harness.run();
        assert!(harness.query_all_by_label("zz_target.nii").next().is_none());
    }

    // ---- The Graph ----

    /// The 40-point fixture as the underlay, the crosshair in the middle.
    fn graph_app() -> App {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("bold+orig"),
            LoadRole::Underlay,
        )]);
        app.poll_loads();
        app.apply(vec![SessionAction::MoveCrosshair([2, 2, 3])]);
        app
    }

    fn graph_cell(harness: &egui_kittest::Harness<'_, App>) -> egui::Rect {
        // The Graph's header is in the fourth cell: the plot is below it.
        let header = harness.get_all_by_value("Graph").last().unwrap().rect();
        egui::Rect::from_min_size(header.left_top() + vec2(0.0, 20.0), vec2(380.0, 230.0))
    }

    #[test]
    fn clicking_in_the_graph_changes_the_time_point_of_the_underlay() {
        let mut harness = run_frames(graph_app(), vec2(1400.0, 900.0));
        assert_eq!(harness.state().session.controller().underlay_sub_brick, 0);
        // Click a third of the way across the plot: about time point 13 of 40.
        let cell = graph_cell(&harness);
        let at = egui::pos2(cell.left() + cell.width() * 0.4, cell.center().y);
        harness.hover_at(at);
        harness.run();
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run();
        }
        harness.run();
        let tr = harness.state().session.controller().underlay_sub_brick;
        assert!((8..=24).contains(&tr) && tr != 0, "time point {tr}");
    }

    #[test]
    fn the_graph_matrix_shows_the_neighbors_and_a_click_moves_the_crosshair() {
        use crate::session::SeriesChange;
        let mut app = graph_app();
        app.apply(vec![SessionAction::Series(SeriesChange::Matrix(3))]);
        let mut harness = run_frames(app, vec2(1400.0, 900.0));
        harness.snapshot("graph_matrix_3x3");
        let before = harness.state().session.controller().cursor.ijk;
        // Click the top-left small graph of the 3x3.
        let cell = graph_cell(&harness);
        let at = egui::pos2(
            cell.left() + cell.width() * 0.12,
            cell.top() + cell.height() * 0.1,
        );
        harness.hover_at(at);
        harness.run();
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: at,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run();
        }
        harness.run();
        let after = harness.state().session.controller().cursor.ijk;
        assert_ne!(before, after);
        for a in 0..3 {
            assert!(before[a].abs_diff(after[a]) <= 1, "{before:?} -> {after:?}");
        }
    }

    #[test]
    fn a_stimulus_is_shaded_and_a_fit_is_drawn_in_the_graph() {
        use crate::session::SeriesChange;
        use crate::session::series::Stim;
        let mut app = graph_app();
        let stim = Stim {
            name: "blocks.1D".into(),
            on: (0..40).map(|t| t % 10 >= 5).collect(),
        };
        app.apply(vec![SessionAction::Series(SeriesChange::Stim(Some(stim)))]);
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("bold+orig"),
            LoadRole::GraphFit,
        )]);
        app.poll_loads();
        assert!(app.session.controller().series.fit.is_some());
        let mut harness = run_frames(app, vec2(1400.0, 900.0));
        harness.snapshot("graph_stimulus_and_fit");
    }

    #[test]
    fn the_graph_can_be_saved_alone_or_with_the_views() {
        let mut app = graph_app();
        let dir = crate::testutil::TempDir::new("graphsave");
        let opts = ExportOptions {
            letters: false,
            ..plain()
        };
        // On its own: 200 x 100 pixels per zoom step.
        let path = dir.path().join("g.png");
        app.export_to(ExportWhat::Graph, &opts, &path);
        assert!(app.error.is_none(), "{:?}", app.error);
        let g = png(&path);
        assert_eq!((g.width(), g.height()), (800, 400));
        // The picture has the trace on the black canvas.
        assert!(g.pixels().filter(|p| p.0[0] > 200).count() > 200);
        // With the three views: a fourth picture in a row, a column, or the grid's cell.
        let size = |app: &mut App, layout, graph, name: &str| {
            let path = dir.path().join(name);
            app.export_to(
                ExportWhat::Views(layout),
                &ExportOptions { graph, ..opts },
                &path,
            );
            png(&path)
        };
        // The Graph is given at least 240 x 150 pixels, more than these tiny views.
        let (w, h, gap) = (240, 150, 8);
        let row = size(&mut app, ViewsLayout::Row, true, "row.png");
        assert_eq!((row.width(), row.height()), (4 * w + 3 * gap, h));
        let row3 = size(&mut app, ViewsLayout::Row, false, "row3.png");
        assert_eq!(row3.width(), 3 * 20 + 2 * gap); // the views' own size, no Graph
        let column = size(&mut app, ViewsLayout::Column, true, "col.png");
        assert_eq!((column.width(), column.height()), (w, 4 * h + 3 * gap));
        let grid = size(&mut app, ViewsLayout::Grid, true, "grid.png");
        assert_eq!((grid.width(), grid.height()), (2 * w + gap, 2 * h + gap));
        // In the 2x2 grid the Graph fills the fourth cell, which is empty without it.
        let bright = |img: &image::RgbaImage, (w, h): (u32, u32)| {
            (h + gap..2 * h + gap)
                .flat_map(|y| (w + gap..2 * w + gap).map(move |x| (x, y)))
                .filter(|&(x, y)| img.get_pixel(x, y).0[0] > 100)
                .count()
        };
        assert!(bright(&grid, (w, h)) > 20);
        let grid3 = size(&mut app, ViewsLayout::Grid, false, "grid3.png");
        assert_eq!(bright(&grid3, (20, 36)), 0, "the fourth cell stays empty");
        // Separate files: the Graph is the fourth.
        app.export_to(
            ExportWhat::Views(ViewsLayout::Individual),
            &ExportOptions {
                graph: true,
                ..opts
            },
            &dir.path().join("fig.png"),
        );
        for view in ["axial", "sagittal", "coronal", "graph"] {
            assert!(
                dir.path().join(format!("fig_{view}.png")).is_file(),
                "{view}"
            );
        }
    }

    #[test]
    fn saving_the_graph_with_no_time_series_says_so() {
        let (mut app, dir) = export_app(); // a single-volume underlay
        app.export_to(ExportWhat::Graph, &plain(), &dir.path().join("g.png"));
        assert!(
            app.error
                .as_deref()
                .is_some_and(|e| e.contains("no time series"))
        );
        assert!(!dir.path().join("g.png").exists());
    }

    #[test]
    fn dragging_a_box_in_the_graph_zooms_and_the_button_returns_to_the_full_course() {
        let mut harness = run_frames(graph_app(), vec2(1400.0, 900.0));
        let cell = graph_cell(&harness);
        let full_course_enabled = |h: &egui_kittest::Harness<'_, App>| {
            !egui_kittest::kittest::NodeT::accesskit_node(&h.get_by_label("Full course"))
                .is_disabled()
        };
        assert!(!full_course_enabled(&harness), "nothing to undo at first");
        // Drag a box across a quarter of the time course.
        let from = egui::pos2(cell.left() + cell.width() * 0.3, cell.center().y - 30.0);
        let to = egui::pos2(cell.left() + cell.width() * 0.5, cell.center().y + 30.0);
        harness.hover_at(from);
        harness.run();
        harness.drag_at(from);
        harness.run();
        // The drag starts with the first small move (as with a real mouse).
        harness.hover_at(from + vec2(3.0, 0.0));
        harness.run();
        harness.hover_at(to);
        harness.run();
        harness.drop_at(to);
        harness.run();
        harness.run();
        assert!(full_course_enabled(&harness), "zoomed in");
        // Zooming is not a click: the time point did not change.
        assert_eq!(harness.state().session.controller().underlay_sub_brick, 0);
        harness.get_by_label("Full course").click();
        harness.run();
        harness.run();
        assert!(!full_course_enabled(&harness), "back to the whole course");
    }

    #[test]
    fn hovering_the_graph_shows_the_time_point_and_value() {
        let mut harness = run_frames(graph_app(), vec2(1400.0, 900.0));
        let cell = graph_cell(&harness);
        harness.hover_at(egui::pos2(
            cell.left() + cell.width() * 0.5,
            cell.center().y,
        ));
        harness.run();
        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("time point")
                .next()
                .is_some()
        );
        assert!(
            harness
                .query_all_by_label_contains("value")
                .next()
                .is_some()
        );
    }

    #[test]
    fn the_graph_card_summary_and_settings_follow_the_session() {
        use crate::session::SeriesChange;
        let mut app = graph_app();
        app.apply(vec![
            SessionAction::Series(SeriesChange::Matrix(5)),
            SessionAction::Series(SeriesChange::Ignore(3)),
            SessionAction::Series(SeriesChange::Percent(true)),
        ]);
        let s = app.session.controller().series.clone();
        assert_eq!((s.matrix, s.ignore, s.percent), (5, 3, true));
        // Nonsense is refused.
        app.apply(vec![
            SessionAction::Series(SeriesChange::Matrix(4)),
            SessionAction::Series(SeriesChange::Ignore(1_000_000)),
        ]);
        let s = app.session.controller().series.clone();
        assert_eq!(s.matrix, 5);
        assert!(s.ignore <= crate::session::series::MAX_IGNORE);
    }

    #[test]
    fn a_long_sub_brick_list_can_be_filtered_by_number_and_picked() {
        use egui::accesskit::Role;
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("bold+orig"),
            LoadRole::Underlay,
        )]);
        app.poll_loads();
        let mut harness = run_frames(app, vec2(1000.0, 1000.0));
        // Combos in order: the workspace menu, ULay, then the sub-brick chooser.
        harness
            .get_all_by_role(Role::ComboBox)
            .nth(2)
            .unwrap()
            .click();
        harness.run();
        harness.get_by_role(Role::TextInput).click();
        harness.run();
        harness.get_by_role(Role::TextInput).type_text("#17");
        harness.run();
        harness.run();
        assert!(harness.query_all_by_label_contains("#17").next().is_some());
        assert!(harness.query_all_by_label_contains("#3 ").next().is_none());
        harness
            .get_all_by_label_contains("#17")
            .next()
            .unwrap()
            .click();
        harness.run();
        harness.run();
        assert_eq!(harness.state().session.controller().underlay_sub_brick, 17);
    }

    #[test]
    fn datasets_chosen_before_are_offered_at_the_top_of_the_dropdowns() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        let fx = fixtures_dir();
        let load = |app: &mut App, name: &str, role| {
            app.apply(vec![SessionAction::LoadDataset(fx.join(name), role)]);
            app.poll_loads();
        };
        load(&mut app, "tiny2+orig", LoadRole::Underlay);
        load(&mut app, "stat+orig", LoadRole::Overlay);
        load(&mut app, "bold+orig", LoadRole::Underlay); // replaces tiny2
        assert_eq!(
            app.recents.list(RecentKind::Underlay),
            [fx.join("bold+orig"), fx.join("tiny2+orig")]
        );
        assert_eq!(
            app.recents.list(RecentKind::Overlay),
            [fx.join("stat+orig")]
        );
        assert!(app.recents.list(RecentKind::Graph).is_empty());
        // tiny2 is no longer in memory, so the underlay dropdown lists it under Recent.
        let mut harness = run_frames(app, vec2(1000.0, 1000.0));
        // The ULay combo is the last place the underlay's name is written as a value.
        harness
            .get_all_by_value("bold+orig")
            .last()
            .unwrap()
            .click();
        harness.run();
        assert!(harness.query_all_by_label("tiny2+orig").next().is_some());
        assert!(
            harness.query_all_by_value("Recent").next().is_some()
                || harness
                    .query_all_by_label_contains("Recent")
                    .next()
                    .is_some()
        );
        // Choosing it reads it again and puts it back on top.
        harness.get_by_label("tiny2+orig").click();
        harness.run();
        harness.run();
        assert_eq!(
            harness.state().session.underlay().unwrap().name,
            "tiny2+orig"
        );
        assert_eq!(
            harness.state().recents.list(RecentKind::Underlay)[0],
            fx.join("tiny2+orig")
        );
    }

    #[test]
    fn recents_also_follow_datasets_chosen_from_those_already_loaded() {
        let (mut app, _) = clusterize_app(); // clust+orig as underlay and overlay
        let path = fixtures_dir().join("clust+orig");
        let first = app.session.controller().underlay.unwrap();
        app.apply(vec![SessionAction::SetUnderlay(first)]);
        assert!(
            app.recents
                .list(RecentKind::Underlay)
                .iter()
                .any(|p| p.ends_with("clust+orig"))
        );
        let _ = path;
    }

    #[test]
    fn the_graph_can_plot_any_dataset_and_fit_any_dataset_of_the_same_length() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        let fx = fixtures_dir();
        let load = |app: &mut App, name: &str, role| {
            app.apply(vec![SessionAction::LoadDataset(fx.join(name), role)]);
            app.poll_loads();
        };
        load(&mut app, "tiny2+orig", LoadRole::Underlay);
        // A 4D dataset that is neither the underlay nor an overlay.
        load(&mut app, "bold+orig", LoadRole::GraphSource);
        let series = app.session.controller().series.clone();
        let source = app.session.store.get(series.source.unwrap()).unwrap();
        assert_eq!((source.name.as_str(), source.nvols), ("bold+orig", 40));
        assert_eq!(app.session.underlay().unwrap().name, "tiny2+orig"); // untouched
        assert!(app.session.overlay_layers().is_empty());
        // A fit of another length is refused, and not kept in memory.
        load(&mut app, "stat+orig", LoadRole::GraphFit);
        assert!(
            app.error
                .as_deref()
                .is_some_and(|e| e.contains("time points"))
        );
        assert!(app.session.controller().series.fit.is_none());
        assert_eq!(app.session.store.iter().count(), 2);
        // A fit with 40 time points is taken.
        load(&mut app, "bold+orig", LoadRole::GraphFit);
        assert!(app.session.controller().series.fit.is_some());
        // A dataset with one time point cannot be plotted.
        load(&mut app, "stat+orig", LoadRole::GraphSource);
        assert!(
            app.error
                .as_deref()
                .is_some_and(|e| e.contains("no time series"))
        );
        assert_eq!(app.recents.list(RecentKind::Graph).len(), 2);
    }

    // ---- Folders and background loading ----

    fn fixtures_dir() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
    }

    #[test]
    fn a_folder_lists_its_datasets_without_loading_any() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.add_folder(&fixtures_dir());
        app.poll_loads();
        let listing = &app.folders[0];
        let labels: Vec<&str> = listing
            .entries
            .as_ref()
            .unwrap()
            .iter()
            .map(|e| e.label.as_str())
            .collect();
        assert!(labels.contains(&"tiny2+orig") && labels.contains(&"stat+orig"));
        // Nothing is read into memory until one is picked.
        assert!(app.session.store.is_empty());
        assert!(app.session.underlay().is_none());
    }

    #[test]
    fn a_recursive_listing_shows_each_subfolder_under_its_name() {
        let dir = crate::testutil::TempDir::new("rtree");
        for rel in [
            "anat/T1.nii.gz",
            "func/run1/bold+tlrc.HEAD",
            "top+orig.HEAD",
        ] {
            let p = dir.path().join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, b"x").unwrap();
        }
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.set_recursive_listing(true);
        app.add_folder(dir.path());
        let entries = app.folders[0].entries.clone().unwrap();
        assert_eq!(entries.len(), 3);
        assert!(app.folders[0].recursive);
        let name = dir
            .path()
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string();
        let mut harness = run_frames(app, vec2(1000.0, 900.0));
        // The header says how many folders there are.
        assert!(
            harness
                .query_all_by_value("3 datasets in 3 folders")
                .next()
                .is_some()
        );
        harness.get_by_value("choose a dataset").click();
        harness.run();
        for heading in [format!("{name}/anat"), format!("{name}/func/run1")] {
            assert!(
                harness
                    .query_all_by_label_contains(&heading)
                    .next()
                    .is_some(),
                "{heading}"
            );
        }
        // Without -R the same folder is one level.
        let mut flat = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        flat.add_folder(dir.path());
        assert_eq!(flat.folders[0].entries.as_ref().unwrap().len(), 1);
    }

    #[test]
    fn the_folder_listing_can_be_turned_off_in_the_prefs() {
        let mut p = prefs(ThemeChoice::Dark, CanvasBackground::Black);
        p.folder_browser = false;
        let mut app = App::new(p, &[], false);
        app.add_folder(&fixtures_dir());
        assert!(app.folders.is_empty());
    }

    #[test]
    fn picking_datasets_from_a_folder_loads_the_underlay_and_an_overlay() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.add_folder(&fixtures_dir());
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("tiny2+orig"),
            LoadRole::Underlay,
        )]);
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("stat+orig"),
            LoadRole::Overlay,
        )]);
        app.poll_loads();
        assert_eq!(app.session.underlay().unwrap().name, "tiny2+orig");
        let layers = app.session.overlay_layers();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].1.name, "stat+orig");
        // Only the two chosen datasets are in memory.
        assert_eq!(app.session.store.len(), 2);
    }

    #[test]
    fn a_failed_load_is_reported_and_changes_nothing() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("nope+orig"),
            LoadRole::Overlay,
        )]);
        app.poll_loads();
        assert!(
            app.error
                .as_deref()
                .is_some_and(|e| e.contains("nope+orig"))
        );
        assert!(app.session.store.is_empty());
    }

    #[test]
    fn the_dropdowns_pick_the_underlay_and_overlays_from_the_folder() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.add_folder(&fixtures_dir());
        let mut harness = run_frames(app, vec2(1000.0, 1000.0));
        // Underlay: the ULay dropdown lists the folder's datasets.
        harness.get_by_value("choose a dataset").click();
        harness.run();
        harness.get_by_label("tiny2+orig").click();
        harness.run();
        harness.run();
        assert_eq!(
            harness.state().session.underlay().unwrap().name,
            "tiny2+orig"
        );
        // Overlay: Define Overlay has the same kind of dropdown.
        harness.get_by_value("no overlay: choose a dataset").click();
        harness.run();
        harness.get_by_label("stat+orig").click();
        harness.run();
        harness.run();
        let layers = harness.state().session.overlay_layers();
        assert_eq!(layers.len(), 1);
        assert_eq!(layers[0].1.name, "stat+orig");
        // The layer's own dropdown replaces its dataset (reading it from the folder).
        harness
            .get_all_by_value("stat+orig")
            .next()
            .unwrap()
            .click();
        harness.run();
        harness.get_by_label("clust+orig").click();
        harness.run();
        harness.run();
        let layers = harness.state().session.overlay_layers();
        assert_eq!((layers.len(), layers[0].1.name.as_str()), (1, "clust+orig"));
        // The replaced overlay dataset is not kept.
        assert_eq!(harness.state().session.store.len(), 3);
        assert_eq!(harness.state().session.store.iter().count(), 2);
    }

    #[test]
    fn the_plus_next_to_the_overlay_dataset_adds_a_second_overlay() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.add_folder(&fixtures_dir());
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("tiny2+orig"),
            LoadRole::Underlay,
        )]);
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("stat+orig"),
            LoadRole::Overlay,
        )]);
        app.poll_loads();
        let mut harness = run_frames(app, vec2(1000.0, 1100.0));
        // The first + is the controller tab's; the second is the overlay card's.
        harness
            .get_all_by_label(egui_phosphor::regular::PLUS)
            .nth(1)
            .unwrap()
            .click();
        harness.run();
        harness.get_by_label("clust+orig").click();
        harness.run();
        harness.run();
        let names: Vec<String> = harness
            .state()
            .session
            .overlay_layers()
            .iter()
            .map(|(_, d)| d.name.clone())
            .collect();
        assert_eq!(names, ["stat+orig", "clust+orig"]); // the new one on top
    }

    #[test]
    fn a_dataset_picked_for_a_layer_that_was_removed_meanwhile_is_dropped() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("tiny2+orig"),
            LoadRole::Underlay,
        )]);
        app.apply(vec![SessionAction::LoadDataset(
            fixtures_dir().join("stat+orig"),
            LoadRole::Layer(crate::session::LayerId(7)),
        )]);
        app.poll_loads();
        assert_eq!(app.session.store.iter().count(), 1);
    }

    #[test]
    fn snapshot_a_folder_listing_in_the_datasets_card() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.add_folder(&fixtures_dir());
        snapshot("folder_listing", app, vec2(1000.0, 700.0), None);
    }

    #[test]
    fn boxed_keeps_the_fill_and_adds_an_outline() {
        let (mut app, id) = clusterize_app();
        app.apply(vec![SessionAction::Layer(id, OverlayChange::Boxed(true))]);
        app.apply(vec![SessionAction::Layer(id, OverlayChange::Fade(true))]);
        snapshot(
            "overlay_boxed_filled_with_fade",
            app,
            vec2(1000.0, 700.0),
            None,
        );
    }

    // ---- Controllers A and B ----

    /// The clustered-statistic dataset as controller A, cloned into B.
    fn two_controllers() -> (App, crate::session::LayerId) {
        let (mut app, id) = clusterize_app();
        app.apply(vec![SessionAction::CloneController { from: 0, to: 1 }]);
        (app, id)
    }

    #[test]
    fn the_plus_clones_the_active_controller_into_b_and_switches_to_compare() {
        let (app, _) = clusterize_app();
        let mut harness = run_frames(app, vec2(1500.0, 900.0));
        assert_eq!(harness.state().session.controllers.len(), 1);
        assert!(!harness.state().compare);
        // The first + is the controller tabs' (the overlay card has one too).
        harness
            .get_all_by_label(egui_phosphor::regular::PLUS)
            .next()
            .unwrap()
            .click();
        harness.run();
        harness.run();
        let s = &harness.state().session;
        assert_eq!(s.controllers.len(), 2);
        assert_eq!(s.active, 1, "the copy is active");
        assert!(harness.state().compare, "the view area compares A and B");
        assert_eq!(harness.state().views.len(), 2);
        assert_eq!(harness.state().clusters.len(), 2);
        // Both tabs exist.
        assert!(harness.query_all_by_label("Controller A").next().is_some());
        assert!(harness.query_all_by_label("Controller B").next().is_some());
    }

    #[test]
    fn clicking_a_tab_or_a_half_makes_that_controller_the_active_one() {
        let (app, _) = two_controllers();
        let mut harness = run_frames(app, vec2(1500.0, 900.0));
        assert_eq!(harness.state().session.active, 1);
        harness.get_by_label("Controller A").click();
        harness.run();
        assert_eq!(harness.state().session.active, 0);
        // Clicking in B's half (the right one) selects B.
        let right = egui::pos2(1100.0, 400.0);
        harness.hover_at(right);
        harness.run();
        for pressed in [true, false] {
            harness.event(egui::Event::PointerButton {
                pos: right,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::NONE,
            });
            harness.run();
        }
        harness.run();
        assert_eq!(harness.state().session.active, 1);
    }

    #[test]
    fn moving_the_crosshair_in_one_controller_moves_it_in_the_other_when_linked() {
        let (mut app, _) = two_controllers();
        app.apply(vec![SessionAction::SelectController(0)]);
        app.apply(vec![SessionAction::MoveCrosshair([3, 4, 5])]);
        let mut harness = run_frames(app, vec2(1500.0, 900.0));
        let s = &harness.state().session;
        assert_eq!(s.controllers[0].cursor.ijk, [3, 4, 5]);
        assert_eq!(s.controllers[1].cursor.ijk, [3, 4, 5], "B followed A");
        // The other way round, too.
        harness.state_mut().apply(vec![
            SessionAction::SelectController(1),
            SessionAction::MoveCrosshair([0, 1, 2]),
        ]);
        harness.run();
        harness.run();
        let s = &harness.state().session;
        assert_eq!(s.controllers[0].cursor.ijk, [0, 1, 2], "A followed B");
        // Unlinked, they go their own ways.
        harness
            .state_mut()
            .apply(vec![SessionAction::SetLinks(crate::session::Links {
                crosshair: false,
                zoom: true,
            })]);
        harness
            .state_mut()
            .apply(vec![SessionAction::MoveCrosshair([1, 1, 1])]);
        harness.run();
        harness.run();
        let s = &harness.state().session;
        assert_eq!(s.controllers[1].cursor.ijk, [1, 1, 1]);
        assert_eq!(s.controllers[0].cursor.ijk, [0, 1, 2]);
    }

    #[test]
    fn each_controller_keeps_its_own_overlay_settings_and_clusters() {
        let (mut app, id_a) = two_controllers();
        let id_b = app.session.controllers[1].overlays[0].id;
        assert_ne!(id_a, id_b);
        app.apply(vec![SessionAction::Layer(
            id_b,
            OverlayChange::Threshold(2.5),
        )]);
        assert_eq!(app.session.controllers[0].overlays[0].threshold, 1.5);
        assert_eq!(app.session.controllers[1].overlays[0].threshold, 2.5);
        // Clusterize on B only.
        app.apply(vec![SessionAction::Layer(
            id_b,
            OverlayChange::Cluster(Some(crate::session::ClusterSettings {
                nn: 2,
                min_size: 2.0,
                ..Default::default()
            })),
        )]);
        let harness = run_frames(app, vec2(1500.0, 900.0));
        let app = harness.state();
        assert!(app.clusters[1].get(id_b).is_some());
        assert!(app.clusters[0].get(id_b).is_none());
        assert!(app.clusters[0].get(id_a).is_none());
        // B's table is B's: its threshold 2.5 gives fewer voxels than A's 1.5 would.
        assert!(!cluster_rows_of(app, 1, id_b).is_empty());
    }

    fn cluster_rows_of(app: &App, ctl: usize, id: crate::session::LayerId) -> Vec<usize> {
        app.clusters[ctl]
            .get(id)
            .and_then(|e| e.result.as_ref().ok())
            .map(|o| o.rows.iter().map(|r| r.voxels).collect())
            .unwrap_or_default()
    }

    #[test]
    fn the_toolbar_lists_what_differs_and_can_make_one_like_the_other() {
        let (mut app, _) = two_controllers();
        let id_b = app.session.controllers[1].overlays[0].id;
        app.apply(vec![SessionAction::Layer(
            id_b,
            OverlayChange::Threshold(4.0),
        )]);
        let mut harness = run_frames(app, vec2(1500.0, 900.0));
        // The chip counts the differences.
        harness.get_by_label_contains("A ≠ B").click();
        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("threshold of overlay 1")
                .next()
                .is_some()
        );
        harness.get_by_label("Make B like A").click();
        harness.run();
        harness.run();
        let s = &harness.state().session;
        assert_eq!(s.controllers[1].overlays[0].threshold, 1.5);
        assert!(s.differences(0, 1).is_empty());
        assert!(
            harness
                .query_all_by_label_contains("A = B")
                .next()
                .is_some()
        );
    }

    /// Zoom with a pinch over the first axial image, then let the frame settle.
    fn pinch_over_axial(harness: &mut egui_kittest::Harness<'_, App>, factor: f32) {
        let header = harness.get_all_by_value("Axial").next().unwrap().rect();
        let at = header.center() + vec2(0.0, 130.0);
        harness.hover_at(at);
        harness.run();
        harness.event(egui::Event::Zoom(factor));
        harness.run();
        harness.run();
    }

    #[test]
    fn a_pinch_zooms_the_views_and_a_double_click_shows_the_whole_image_again() {
        let (app, _) = clusterize_app();
        // Short frames, so that two clicks a frame apart are a double-click.
        let mut harness = egui_kittest::Harness::builder()
            .with_size(vec2(1300.0, 900.0))
            .with_step_dt(0.02)
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        assert_eq!(harness.state().views[0].zoom_state().zoom, 1.0);
        pinch_over_axial(&mut harness, 2.0);
        let z = harness.state().views[0].zoom_state().zoom;
        assert!((z - 2.0).abs() < 0.01, "{z}");
        // The zoom is shared by the three views: the zoom % reads twice as much.
        pinch_over_axial(&mut harness, 1.5);
        assert!((harness.state().views[0].zoom_state().zoom - 3.0).abs() < 0.01);
        // Never below the whole image or above 16x.
        pinch_over_axial(&mut harness, 0.01);
        assert_eq!(harness.state().views[0].zoom_state().zoom, 1.0);
        pinch_over_axial(&mut harness, 100.0);
        assert_eq!(harness.state().views[0].zoom_state().zoom, 16.0);
        // A double-click resets.
        let header = harness.get_all_by_value("Axial").next().unwrap().rect();
        let at = header.center() + vec2(0.0, 130.0);
        for _ in 0..2 {
            for pressed in [true, false] {
                harness.event(egui::Event::PointerButton {
                    pos: at,
                    button: egui::PointerButton::Primary,
                    pressed,
                    modifiers: egui::Modifiers::NONE,
                });
                harness.run_steps(1);
            }
        }
        harness.run();
        assert_eq!(harness.state().views[0].zoom_state().zoom, 1.0);
        assert_eq!(
            harness.state().views[0].zoom_state().pans,
            [egui::Vec2::ZERO; 3]
        );
    }

    #[test]
    fn zoom_is_linked_between_controllers_unless_unlinked() {
        let (mut app, _) = two_controllers();
        app.apply(vec![SessionAction::SelectController(0)]);
        let mut harness = run_frames(app, vec2(1500.0, 900.0));
        pinch_over_axial(&mut harness, 2.0); // over A's axial image
        let zooms = |h: &egui_kittest::Harness<'_, App>| {
            (
                h.state().views[0].zoom_state().zoom,
                h.state().views[1].zoom_state().zoom,
            )
        };
        let (a, b) = zooms(&harness);
        assert!((a - 2.0).abs() < 0.01 && (b - 2.0).abs() < 0.01, "{a} {b}");
        harness
            .state_mut()
            .apply(vec![SessionAction::SetLinks(crate::session::Links {
                crosshair: true,
                zoom: false,
            })]);
        pinch_over_axial(&mut harness, 2.0);
        let (a, b) = zooms(&harness);
        assert!((a - 4.0).abs() < 0.01, "{a}");
        assert!((b - 2.0).abs() < 0.01, "B kept its own zoom: {b}");
    }

    #[test]
    fn snapshot_a_zoomed_view_keeps_its_orientation_letters_in_the_card() {
        let (app, _) = clusterize_app();
        let mut harness = run_frames(app, vec2(1300.0, 900.0));
        pinch_over_axial(&mut harness, 3.0);
        harness.snapshot("zoomed_views");
    }

    #[test]
    fn closing_a_controller_removes_its_view_and_leaves_compare() {
        let (app, _) = two_controllers();
        let mut harness = run_frames(app, vec2(1500.0, 900.0));
        harness
            .state_mut()
            .apply(vec![SessionAction::RemoveController(1)]);
        harness.run();
        harness.run();
        let a = harness.state();
        assert_eq!(
            (a.session.controllers.len(), a.views.len(), a.clusters.len()),
            (1, 1, 1)
        );
        assert!(!a.compare);
        assert_eq!(a.session.active, 0);
    }

    #[test]
    fn snapshot_two_controllers_side_by_side() {
        let (mut app, _) = two_controllers();
        let id_b = app.session.controllers[1].overlays[0].id;
        app.apply(vec![SessionAction::Layer(
            id_b,
            OverlayChange::Threshold(3.0),
        )]);
        app.controller
            .workspaces
            .current_mut()
            .toggle_collapsed(ToolId::Datasets);
        snapshot("compare_a_b", app, vec2(1500.0, 800.0), None);
    }

    // ---- Clusterize ----

    /// The clustered-statistic fixture as underlay and overlay, thresholded at
    /// 1.5 with NN2 and at least 2 voxels (23, 22, 9 and 2 voxels, as
    /// `3dClusterize` finds).
    fn clusterize_app() -> (App, crate::session::LayerId) {
        let path =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/clust+orig");
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[path.clone(), path],
            false,
        );
        let id = layer_ids(&app)[0];
        app.apply(vec![SessionAction::Layer(
            id,
            OverlayChange::Threshold(1.5),
        )]);
        (app, id)
    }

    /// Fold the Datasets and Define Overlay cards so the cluster table fits.
    fn fold_for_table(app: &mut App, id: crate::session::LayerId) {
        let ws = app.controller.workspaces.current_mut();
        ws.toggle_collapsed(ToolId::Datasets);
        ws.show(ToolId::Clusterize);
        app.controller
            .instance_collapsed
            .insert((ToolId::Overlay, id.0), true);
    }

    fn hook(app: &mut App, id: crate::session::LayerId, only_clusters: bool) {
        app.apply(vec![SessionAction::Layer(
            id,
            OverlayChange::Cluster(Some(crate::session::ClusterSettings {
                nn: 2,
                min_size: 2.0,
                only_clusters,
                ..Default::default()
            })),
        )]);
    }

    fn run_frames(app: App, size: egui::Vec2) -> egui_kittest::Harness<'static, App> {
        let mut harness = egui_kittest::Harness::builder()
            .with_size(size)
            .build_ui_state(
                |ui, app: &mut App| {
                    app.draw(ui);
                },
                app,
            );
        harness.run();
        harness
    }

    fn cluster_rows(app: &App, id: crate::session::LayerId) -> Vec<usize> {
        app.clusters[0]
            .get(id)
            .and_then(|e| e.result.as_ref().ok())
            .map(|o| o.rows.iter().map(|r| r.voxels).collect())
            .unwrap_or_default()
    }

    #[test]
    fn hooking_clusterize_under_a_layer_clusters_it_like_3dclusterize() {
        let (mut app, id) = clusterize_app();
        hook(&mut app, id, false);
        let harness = run_frames(app, vec2(1300.0, 1100.0));
        assert_eq!(cluster_rows(harness.state(), id), [23, 22, 9, 2]);
    }

    #[test]
    fn the_attach_chip_hooks_and_unhooks_and_shows_the_card() {
        let (app, id) = clusterize_app();
        let mut harness = run_frames(app, vec2(1300.0, 1100.0));
        let chip = format!("{} Cluster", egui_phosphor::regular::PLUS_SQUARE);
        harness.get_by_label(&chip).click();
        harness.run();
        harness.run();
        let state = harness.state();
        assert!(state.session.layer(id).unwrap().cluster.is_some());
        // The tile was off; hooking turned it on so the card shows.
        assert!(
            state
                .controller
                .workspaces
                .current()
                .state(ToolId::Clusterize)
                .unwrap()
                .on
        );
        assert!(state.clusters[0].get(id).is_some());
        harness.get_by_label(&chip).click();
        harness.run();
        assert!(harness.state().session.layer(id).unwrap().cluster.is_none());
        assert!(harness.state().clusters[0].get(id).is_none());
    }

    #[test]
    fn the_cluster_tile_hooks_itself_under_the_top_layer() {
        let (app, id) = clusterize_app();
        let mut harness = run_frames(app, vec2(1300.0, 1100.0));
        harness.get_by_label("Clusterize").click();
        harness.run();
        assert!(harness.state().session.layer(id).unwrap().cluster.is_some());
    }

    #[test]
    fn only_clusters_hides_the_voxels_outside_them() {
        let (mut app, id) = clusterize_app();
        hook(&mut app, id, true);
        let harness = run_frames(app, vec2(1300.0, 1100.0));
        let state = harness.state();
        let layer = state.session.layer(id).unwrap();
        let keep = state.clusters[0]
            .keep(layer)
            .expect("restricted to clusters");
        assert_eq!(keep.iter().filter(|k| **k).count(), 23 + 22 + 9 + 2);
        // The views were given the same voxels.
        assert!(state.views[0].overlay_frames(id).unwrap().keep.is_some());
    }

    #[test]
    fn clicking_a_cluster_row_jumps_the_crosshair_to_its_peak() {
        let (mut app, id) = clusterize_app();
        hook(&mut app, id, false);
        fold_for_table(&mut app, id);
        let mut harness = run_frames(app, vec2(1300.0, 1100.0));
        // The largest cluster peaks at -5.2776, where 3dClusterize says RAI
        // (4, -0.5, 9) mm.
        harness.get_all_by_value("-5.2776").next().unwrap().click();
        harness.run();
        let ds = harness.state().session.underlay().unwrap().clone();
        let ijk = harness.state().session.controller().cursor.ijk;
        let ras = crate::geom::coords::ijk_to_ras(&ds.ijk_to_ras, ijk);
        assert!(
            (-ras[0] - 4.0).abs() < 0.6
                && (-ras[1] + 0.5).abs() < 0.6
                && (ras[2] - 9.0).abs() < 1.6,
            "{ras:?}"
        );
    }

    #[test]
    fn dragging_the_cluster_tile_onto_a_layer_card_hooks_it_there() {
        let (app, id) = clusterize_app();
        let mut harness = run_frames(app, vec2(1300.0, 1100.0));
        let tile = harness.get_by_label("Clusterize").rect().center();
        harness.hover_at(tile);
        harness.drag_at(tile);
        harness.run();
        harness.hover_at(tile + egui::vec2(3.0, 30.0));
        harness.run();
        // While dragging, the card offers a slot.
        let slot = harness
            .get_by_label_contains("Drop to attach")
            .rect()
            .center();
        harness.hover_at(slot);
        harness.run();
        harness.drop_at(slot);
        harness.run();
        harness.run();
        let state = harness.state();
        assert!(state.session.layer(id).unwrap().cluster.is_some());
        assert!(state.controller.tile_drag.is_none());
    }

    #[test]
    fn snapshot_dragging_a_tile_shows_the_dashed_drop_slot() {
        let (app, id) = clusterize_app();
        let mut app = app;
        // Fold the Overlay card so the slot is in view.
        app.controller
            .instance_collapsed
            .insert((ToolId::Overlay, id.0), true);
        let mut harness = run_frames(app, vec2(1300.0, 900.0));
        let tile = harness.get_by_label("Clusterize").rect().center();
        harness.hover_at(tile);
        harness.drag_at(tile);
        harness.run();
        harness.hover_at(tile + egui::vec2(3.0, 30.0));
        harness.run();
        let slot = harness
            .get_by_label_contains("Drop to attach")
            .rect()
            .center();
        harness.hover_at(slot);
        harness.run();
        harness.snapshot("clusterize_drop_slot");
    }

    #[test]
    fn a_tile_dropped_elsewhere_hooks_nothing() {
        let (app, id) = clusterize_app();
        let mut harness = run_frames(app, vec2(1300.0, 1100.0));
        let tile = harness.get_by_label("Clusterize").rect().center();
        harness.hover_at(tile);
        harness.drag_at(tile);
        harness.run();
        let away = egui::pos2(900.0, 700.0); // over the views
        harness.hover_at(away);
        harness.run();
        harness.drop_at(away);
        harness.run();
        assert!(harness.state().session.layer(id).unwrap().cluster.is_none());
        assert!(harness.state().controller.tile_drag.is_none());
    }

    #[test]
    fn clicking_a_table_heading_sorts_and_clicking_again_reverses() {
        let (mut app, id) = clusterize_app();
        hook(&mut app, id, false);
        fold_for_table(&mut app, id);
        let mut harness = run_frames(app, vec2(1300.0, 1100.0));
        let y = |h: &egui_kittest::Harness<'_, App>, v: &str| {
            h.get_all_by_value(v).next().unwrap().rect().min.y
        };
        assert!(y(&harness, "23") < y(&harness, "9")); // by rank: largest first
        for name in [
            "vox".to_string(),
            format!("vox {}", egui_phosphor::regular::CARET_DOWN),
        ] {
            // The heading follows the combo box showing the same unit.
            harness.get_all_by_value(&name).last().unwrap().click();
            harness.run();
        }
        assert!(y(&harness, "23") > y(&harness, "9")); // reversed: smallest first
    }

    #[test]
    fn the_saved_table_says_what_was_clustered_and_lists_the_clusters() {
        let (mut app, id) = clusterize_app();
        hook(&mut app, id, false);
        let harness = run_frames(app, vec2(1300.0, 1100.0));
        let text = harness.state().cluster_report(id).unwrap();
        assert!(text.starts_with("# afniru clusters of overlay 1 (clust+orig) |thr >= 1.5; NN2"));
        assert_eq!(text.lines().filter(|l| !l.starts_with('#')).count(), 4);
        // A layer without Clusterize has nothing to save.
        let (plain, id) = clusterize_app();
        assert!(plain.cluster_report(id).is_none());
    }

    #[test]
    fn clicking_the_same_cluster_again_alternates_between_peak_and_center_of_mass() {
        let (mut app, id) = clusterize_app();
        hook(&mut app, id, false);
        fold_for_table(&mut app, id);
        let mut harness = run_frames(app, vec2(1300.0, 1100.0));
        let cursor = |h: &egui_kittest::Harness<'_, App>| h.state().session.controller().cursor.ijk;
        let click_peak = |h: &mut egui_kittest::Harness<'_, App>| {
            h.get_all_by_value("-5.2776").next().unwrap().click();
            h.run();
            h.run();
        };
        click_peak(&mut harness);
        let at_peak = cursor(&harness);
        assert!(
            harness
                .query_all_by_value(&format!("{} peak", egui_phosphor::regular::TARGET))
                .next()
                .is_some(),
            "labelled as the peak"
        );
        // The same cluster again: its center of mass, labelled.
        click_peak(&mut harness);
        let at_center = cursor(&harness);
        assert_ne!(at_peak, at_center);
        assert!(
            harness
                .query_all_by_value(&format!(
                    "{} center",
                    egui_phosphor::regular::CROSSHAIR_SIMPLE
                ))
                .next()
                .is_some(),
            "labelled as the center"
        );
        assert!(
            harness
                .query_all_by_value(&format!("{} peak", egui_phosphor::regular::TARGET))
                .next()
                .is_none()
        );
        // And once more: back to the peak.
        click_peak(&mut harness);
        assert_eq!(cursor(&harness), at_peak);
        assert!(
            harness
                .query_all_by_value(&format!("{} peak", egui_phosphor::regular::TARGET))
                .next()
                .is_some()
        );
    }

    #[test]
    fn snapshot_clusterize_hooked_under_define_overlay() {
        let (mut app, id) = clusterize_app();
        hook(&mut app, id, false);
        fold_for_table(&mut app, id);
        app.controller
            .workspaces
            .current_mut()
            .show(ToolId::Clusterize);
        snapshot("clusterize_hooked", app, vec2(1300.0, 1100.0), None);
    }

    #[test]
    fn snapshot_a_folded_group_is_one_line_with_the_childs_summary() {
        let (mut app, id) = clusterize_app();
        hook(&mut app, id, false);
        app.controller
            .workspaces
            .current_mut()
            .show(ToolId::Clusterize);
        app.controller.group_folded.insert((ToolId::Overlay, id.0));
        snapshot("clusterize_group_folded", app, vec2(1300.0, 900.0), None);
    }

    #[test]
    fn snapshot_only_clusters_draws_just_the_surviving_voxels() {
        let (mut app, id) = clusterize_app();
        // Only the two big clusters (23 and 22 voxels) survive 22 voxels.
        app.apply(vec![SessionAction::Layer(
            id,
            OverlayChange::Cluster(Some(crate::session::ClusterSettings {
                nn: 2,
                min_size: 22.0,
                only_clusters: true,
                ..Default::default()
            })),
        )]);
        fold_for_table(&mut app, id);
        snapshot("clusterize_only_clusters", app, vec2(1300.0, 1100.0), None);
    }

    // ---- Processing rail ----

    use crate::processing::Health;
    use crate::processing::discover::testkit;

    fn app_with_run(demo_data: bool) -> (App, crate::testutil::TempDir) {
        let (tmp, _) = testkit::build("complete", &[], None);
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            demo_data,
        );
        app.detect_processing(&[tmp.path().to_path_buf()], &[]);
        (app, tmp)
    }

    #[test]
    fn hovering_a_traffic_light_shows_its_status_and_clicking_it_opens_the_details() {
        let (app, _tmp) = app_with_run(true);
        let mut harness = run_frames(app, vec2(1600.0, 900.0));
        // A row of the rail is labelled "<step>, <health>"; its light is at the
        // right end of the row.
        let row = harness
            .get_all_by_label_contains("Slice timing,")
            .next()
            .unwrap()
            .rect();
        let light = egui::pos2(row.right() - 12.0, row.center().y);
        assert!(
            harness
                .query_all_by_label_contains("Click for every check")
                .next()
                .is_none()
        );
        harness.hover_at(light);
        harness.run();
        harness.run();
        // The pop-up names the step and its state and invites a click.
        assert!(
            harness
                .query_all_by_label_contains("Slice timing ·")
                .next()
                .is_some()
        );
        let invite = harness.get_by_label_contains("Click for every check");
        // It opens to the left of the rail, so it does not cover the steps.
        assert!(
            invite.rect().right() <= row.left() + 1.0,
            "{:?} vs {row:?}",
            invite.rect()
        );
        assert!(!harness.state().rail.detail_open);
        // Moving onto the pop-up keeps it open; clicking it opens the details.
        harness.hover_at(invite.rect().center());
        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("Click for every check")
                .next()
                .is_some()
        );
        harness
            .get_by_label_contains("Click for every check")
            .click();
        harness.run();
        harness.run();
        assert!(harness.state().rail.detail_open);
        assert!(
            harness
                .query_all_by_label_contains("Click for every check")
                .next()
                .is_none()
        );
        // The selected step is the one whose light was used.
        let model = harness.state().processing.as_ref().unwrap();
        assert!(model.selected.as_ref().is_some_and(|id| {
            model
                .run
                .step(id)
                .is_some_and(|s| s.label == "Slice timing")
        }));
    }

    #[test]
    fn the_status_popup_closes_when_the_pointer_leaves() {
        let (app, _tmp) = app_with_run(true);
        let mut harness = run_frames(app, vec2(1600.0, 900.0));
        let row = harness
            .get_all_by_label_contains("Alignment,")
            .next()
            .unwrap()
            .rect();
        harness.hover_at(egui::pos2(row.right() - 12.0, row.center().y));
        harness.run();
        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("Alignment ·")
                .next()
                .is_some()
        );
        harness.hover_at(egui::pos2(300.0, 600.0)); // over the controller
        harness.run();
        harness.run();
        assert!(
            harness
                .query_all_by_label_contains("Click for every check")
                .next()
                .is_none()
        );
    }

    #[test]
    fn a_results_directory_given_explicitly_opens_in_the_rail() {
        let (app, _tmp) = app_with_run(false);
        let model = app.processing.as_ref().unwrap();
        assert_eq!(model.run.name, "sub-01");
        assert!(app.error.is_none());
    }

    #[test]
    fn a_bad_explicit_directory_is_reported_and_implicit_ones_are_silent() {
        let tmp = crate::testutil::TempDir::new("empty");
        let mut app = demo(ThemeChoice::Dark);
        app.detect_processing(&[], &[tmp.path().to_path_buf()]);
        assert!(app.processing.is_none());
        assert!(app.error.is_none());
        app.detect_processing(&[tmp.path().to_path_buf()], &[]);
        assert!(app.processing.is_none());
        assert!(
            app.error
                .as_deref()
                .is_some_and(|e| e.contains("no afni_proc.py script"))
        );
    }

    #[test]
    fn the_first_implicit_directory_with_a_run_wins() {
        let (tmp, _) = testkit::build("complete", &[], None);
        let empty = crate::testutil::TempDir::new("empty2");
        let mut app = demo(ThemeChoice::Dark);
        app.detect_processing(&[], &[empty.path().to_path_buf(), tmp.path().to_path_buf()]);
        assert!(app.processing.is_some());
    }

    fn step_id(s: &str) -> StepId {
        StepId(s.into())
    }

    #[test]
    fn view_with_an_underlay_asks_before_replacing_it() {
        let (mut app, _tmp) = app_with_run(true);
        let before = app.session.underlay().unwrap().name.clone();
        app.request_view(&step_id("blur"));
        let pending = app.pending_view.as_ref().expect("asks first");
        assert_eq!(pending.step_label, "Smoothing");
        assert!(pending.options.len() >= 2);
        assert_eq!(app.session.underlay().unwrap().name, before); // nothing replaced yet
    }

    #[test]
    fn view_with_no_underlay_and_one_dataset_opens_it_directly() {
        let (mut app, _tmp) = app_with_run(false);
        assert!(app.session.underlay().is_none());
        app.request_view(&step_id("tcat")); // one output, nothing earlier
        assert!(app.pending_view.is_none());
        assert_eq!(
            app.session.underlay().unwrap().name,
            "pb00.sub-01.r01.tcat+orig"
        );
    }

    #[test]
    fn view_of_a_step_without_datasets_says_so_instead_of_asking() {
        let (mut app, _tmp) = app_with_run(false);
        app.request_view(&step_id("outcount"));
        assert!(app.pending_view.is_none());
        assert!(
            app.error
                .as_deref()
                .is_some_and(|e| e.contains("no dataset to view"))
        );
    }

    #[test]
    fn choosing_a_dataset_replaces_the_underlay_and_keeps_the_crosshair_in_the_world() {
        let (mut app, tmp) = app_with_run(false);
        let results = tmp.path().join("sub-01.results");
        app.open(&results.join("pb01.sub-01.r01.tshift+orig.HEAD"));
        app.session.apply(SessionAction::MoveCrosshair([1, 2, 3]));
        let world = {
            let ds = app.session.underlay().unwrap().clone();
            crate::geom::coords::ijk_to_ras(&ds.ijk_to_ras, [1, 2, 3])
        };
        app.open(&results.join("pb03.sub-01.r01.blur+tlrc.HEAD"));
        assert_eq!(
            app.session.underlay().unwrap().name,
            "pb03.sub-01.r01.blur+tlrc"
        );
        let ds = app.session.underlay().unwrap().clone();
        let now =
            crate::geom::coords::ijk_to_ras(&ds.ijk_to_ras, app.session.controller().cursor.ijk);
        assert_eq!(now, world);
    }

    #[test]
    fn refreshing_the_run_never_changes_the_displayed_dataset() {
        let (mut app, _tmp) = app_with_run(true);
        let name = app.session.underlay().unwrap().name.clone();
        let generation = app.session.generation;
        let model = app.processing.as_mut().unwrap();
        model.select(Some(step_id("align")));
        app.handle_rail(vec![RailEvent::Refresh]);
        assert_eq!(app.session.underlay().unwrap().name, name);
        assert_eq!(app.session.generation, generation);
        assert_eq!(
            app.processing.as_ref().unwrap().selected,
            Some(step_id("align"))
        );
    }

    #[test]
    fn processing_rail_state_survives_a_restart() {
        use eframe::App as _;
        let (mut a, _tmp) = app_with_run(true);
        a.rail.collapsed = true;
        let mut storage = MemStorage::default();
        a.save(&mut storage);
        let mut b = demo(ThemeChoice::Dark);
        b.restore(&storage);
        assert!(b.rail.collapsed);
        assert!(!b.rail.drawer_open);
    }

    #[test]
    fn health_is_available_for_every_step_of_the_loaded_run() {
        let (app, _tmp) = app_with_run(false);
        let run = &app.processing.as_ref().unwrap().run;
        assert!(
            run.steps
                .iter()
                .all(|s| s.assessment.health != Health::Failed),
            "complete fixture is healthy"
        );
    }

    #[test]
    fn shell_with_the_processing_rail() {
        let (mut app, _tmp) = app_with_run(true);
        app.processing
            .as_mut()
            .unwrap()
            .select(Some(step_id("blur")));
        snapshot("shell_processing_rail", app, vec2(1500.0, 1000.0), None);
    }

    #[test]
    fn shell_narrow_window_collapses_the_rail_to_a_strip() {
        let (app, _tmp) = app_with_run(true);
        snapshot("shell_processing_narrow", app, vec2(900.0, 640.0), None);
    }

    #[test]
    fn shell_view_dialog() {
        let (mut app, _tmp) = app_with_run(true);
        app.request_view(&step_id("blur"));
        assert!(app.pending_view.is_some());
        snapshot("shell_view_dialog", app, vec2(1400.0, 800.0), None);
    }

    #[derive(Default)]
    struct MemStorage(std::collections::HashMap<String, String>);

    impl eframe::Storage for MemStorage {
        fn get_string(&self, key: &str) -> Option<String> {
            self.0.get(key).cloned()
        }
        fn set_string(&mut self, key: &str, value: String) {
            self.0.insert(key.into(), value);
        }
        fn remove_string(&mut self, key: &str) {
            self.0.remove(key);
        }
        fn flush(&mut self) {}
    }

    #[test]
    fn workspaces_and_rail_state_survive_a_restart() {
        use eframe::App as _;
        let mut a = demo(ThemeChoice::Dark);
        a.controller
            .workspaces
            .current_mut()
            .toggle_collapsed(ToolId::Crosshair);
        a.controller.workspaces.save_as("Mine");
        a.controller.rail = true;
        let mut storage = MemStorage::default();
        a.save(&mut storage);

        let mut b = demo(ThemeChoice::Dark);
        b.restore(&storage);
        assert_eq!(b.controller.workspaces, a.controller.workspaces);
        assert!(b.controller.rail);
        assert_eq!(b.controller.workspaces.current().name, "Mine");
        // Transient state is not saved.
        assert!(b.controller.popover.is_none());
    }

    #[test]
    fn restoring_from_empty_storage_keeps_the_defaults() {
        let mut a = demo(ThemeChoice::Dark);
        a.restore(&MemStorage::default());
        assert_eq!(a.controller.workspaces.current().name, "Default");
        assert!(!a.controller.rail);
    }
}
