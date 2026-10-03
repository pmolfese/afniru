//! The eframe application: owns the session and the controller, and runs one
//! frame.

use std::path::{Path, PathBuf};

use crate::data::{Dataset, synthetic};
use crate::loader::{FolderListing, Loader};
use crate::prefs::Prefs;
use crate::processing::StepId;
use crate::processing::model::{ProcessingModel, ViewOption};
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
use crate::ui::view_area::{OverlayTarget, Target, ViewArea};

/// Everything afniru holds while running.
pub struct App {
    prefs: Prefs,
    /// Datasets and controllers (no egui types).
    session: Session,
    /// Last load error, shown in the status bar until the next load.
    error: Option<String>,
    /// The view area: layout, window, texture caches.
    view: ViewArea,
    /// The controller sidebar: workspaces, rail state.
    controller: ControllerUi,
    /// The clusters of the layers Clusterize is hooked under.
    clusters: Engine,
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
            view: ViewArea::new(&prefs),
            prefs,
            session,
            error: None,
            controller: ControllerUi::default(),
            clusters: Engine::default(),
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

    /// Make `d` the underlay.
    fn add(&mut self, d: Dataset) {
        self.error = None;
        self.session.add_dataset(d);
        self.view.reset();
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

    /// Apply the loads that finished and take in the folder listings that
    /// arrived. A failed load is reported in the status bar.
    fn poll_loads(&mut self) {
        let (ready, listings) = self.loader.poll();
        for loaded in ready {
            match loaded.result {
                Ok(d) => {
                    self.error = None;
                    match loaded.role {
                        LoadRole::Underlay => self.add(d),
                        LoadRole::Overlay => {
                            let id = self.session.store.add(d);
                            self.session.apply(SessionAction::AddOverlay(id));
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

    /// List the datasets of `dir` in the Datasets card (reading it in the
    /// background). A folder already listed is read again.
    pub fn add_folder(&mut self, dir: &Path) {
        if !self.prefs.folder_browser {
            return; // AFNIRU_FOLDER_BROWSER = NO
        }
        if !self.folders.iter().any(|f| f.dir == dir) {
            self.folders.push(FolderListing {
                dir: dir.to_path_buf(),
                entries: None,
                error: None,
            });
        }
        self.loader.scan(dir);
        self.poll_loads();
    }

    /// Carry out the actions cards asked for; reset the view's caches if
    /// what it displays changed.
    fn apply(&mut self, actions: Vec<SessionAction>) {
        let before = self.session.generation;
        for a in actions {
            match a {
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
                SessionAction::CancelLoad(id) => {
                    self.loader.cancel(id);
                    continue;
                }
                _ => {}
            }
            self.session.apply(a);
        }
        if self.session.generation != before {
            self.view.reset();
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
        let layer = self.session.layer(id)?;
        let settings = layer.cluster?;
        let out = self.clusters.get(id)?.result.as_ref().ok()?;
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
        egui::Panel::top("toolbar").show(ui, |ui| {
            shell::toolbar(ui, &theme, underlay.as_deref(), &mut self.view.options);
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            shell::status_bar(
                ui,
                &theme,
                underlay.as_deref(),
                self.error.as_deref(),
                &self.view.conventions(underlay.as_deref()),
                &self.loader.loading(),
            );
        });

        // Clusters follow the layers' thresholds; they wait for the mouse to
        // be released so that dragging a threshold stays smooth.
        if let Some(under) = &underlay {
            let settled = !ctx.input(|i| i.pointer.any_down());
            let view = &self.view;
            let wake: Wake = {
                let ctx = ctx.clone();
                std::sync::Arc::new(move || ctx.request_repaint())
            };
            let waiting = self.clusters.update(
                &self.session,
                under,
                settled,
                &|layers, id| view.passed_everywhere(under, layers, id),
                &wake,
            );
            if waiting {
                ctx.request_repaint();
            } else if self.clusters.busy() {
                // The worker wakes the interface when done; this is a net.
                ctx.request_repaint_after(std::time::Duration::from_millis(250));
            }
        }

        // The controller (left), then the views.
        let actions = {
            let controller = self.session.controller();
            let loading = self.loader.loading();
            let layers = self.session.overlay_layers();
            let plain: Vec<_> = layers.iter().map(|(l, _)| l.clone()).collect();
            let probes = underlay
                .as_deref()
                .map(|d| self.view.probe(d, &plain, &controller.cursor))
                .unwrap_or_default();
            let cx = ToolContext {
                theme: &theme,
                session: &self.session,
                controller,
                dataset: underlay.as_deref(),
                coord_orient: self.prefs.coord_orient,
                value: underlay
                    .as_deref()
                    .and_then(|d| self.view.value_at(d, &controller.cursor)),
                loading: &loading,
                folders: &self.folders,
                overlays: layers
                    .iter()
                    .enumerate()
                    .map(|(n, (layer, dataset))| OverlayContext {
                        layer,
                        dataset: dataset.as_ref(),
                        drawn: probes.get(n).and_then(|p| p.drawn),
                        problem: probes.get(n).and_then(|p| p.problem.clone()),
                        frames: self.view.overlay_frames(layer.id),
                        cluster: self.clusters.get(layer.id),
                        values: underlay.as_deref().and_then(|d| {
                            self.view.overlay_values_at(layer.id, d, &controller.cursor)
                        }),
                    })
                    .collect(),
            };
            self.controller.panel(ui, &cx)
        };
        self.apply(actions);

        // The Processing rail (right), when there is a run.
        let rail_events = match &mut self.processing {
            Some(model) => self.rail.panel(ui, &theme, model),
            None => Vec::new(),
        };
        self.handle_rail(rail_events);

        // Cards may have changed the underlay.
        let underlay = self.session.underlay().cloned();
        let sub_brick = self.session.controller().underlay_sub_brick;
        let generation = self.session.generation;
        let layers = self.session.overlay_layers();
        let series = self.session.controller().series.clone();
        let overlay_targets = || -> Vec<OverlayTarget> {
            layers
                .iter()
                .map(|(layer, ds)| OverlayTarget {
                    layer,
                    ds: ds.as_ref(),
                    keep: self.clusters.keep(layer),
                })
                .collect()
        };
        if let Some(ds) = &underlay {
            self.view
                .handle_keys(&ctx, ds, &mut self.session.controller_mut().cursor);
            let target = Target {
                ds,
                sub_brick,
                generation,
                overlays: overlay_targets(),
                store: &self.session.store,
                series: &series,
            };
            let cursor = self.session.controller().cursor;
            egui::Panel::bottom("readout").show(ui, |ui| {
                self.view.readout(ui, &theme, &target, &cursor);
            });
        }
        let mut graph_actions = Vec::new();
        let background = egui::Frame::new().fill(theme.bg).inner_margin(8);
        egui::CentralPanel::default()
            .frame(background)
            .show(ui, |ui| match &underlay {
                Some(ds) => {
                    let target = Target {
                        ds,
                        sub_brick,
                        generation,
                        overlays: overlay_targets(),
                        store: &self.session.store,
                        series: &series,
                    };
                    // Disjoint fields: the store is read while the cursor moves.
                    let active = self.session.active;
                    let cursor = &mut self.session.controllers[active].cursor;
                    self.view.ui(ui, &theme, &target, cursor);
                    graph_actions = self.view.take_actions();
                }
                None => {
                    ui.centered_and_justified(|ui| {
                        ui.label(egui::RichText::new("no dataset").color(theme.text_faint));
                    });
                }
            });
        self.apply(graph_actions);
        self.view_dialog(&ctx, &theme);
        action
    }
}

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
    }
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use egui::vec2;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::prefs::{CanvasBackground, ThemeChoice};
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
        app.view
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
    fn the_folder_buttons_in_the_datasets_card_load_the_dataset() {
        let mut app = App::new(
            prefs(ThemeChoice::Dark, CanvasBackground::Black),
            &[],
            false,
        );
        app.add_folder(&fixtures_dir());
        let mut harness = run_frames(app, vec2(1000.0, 900.0));
        // One "ULay" button per listed dataset; take the first.
        harness.get_all_by_label("ULay").next().unwrap().click();
        harness.run();
        harness.run();
        assert!(harness.state().session.underlay().is_some());
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
        app.clusters
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
        assert!(state.clusters.get(id).is_some());
        harness.get_by_label(&chip).click();
        harness.run();
        assert!(harness.state().session.layer(id).unwrap().cluster.is_none());
        assert!(harness.state().clusters.get(id).is_none());
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
        let keep = state.clusters.keep(layer).expect("restricted to clusters");
        assert_eq!(keep.iter().filter(|k| **k).count(), 23 + 22 + 9 + 2);
        // The views were given the same voxels.
        assert!(state.view.overlay_frames(id).unwrap().keep.is_some());
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
        for name in ["vox", "vox ▼"] {
            // The heading follows the combo box showing the same unit.
            harness.get_all_by_value(name).last().unwrap().click();
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
