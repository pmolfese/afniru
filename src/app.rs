//! The eframe application: owns the session state and runs one frame.

use std::path::{Path, PathBuf};

use crate::data::{Dataset, load, synthetic};
use crate::prefs::Prefs;
use crate::ui::shell::{self, Action};
use crate::ui::theme::Theme;
use crate::ui::view_card::ViewCard;

/// Everything afniru holds while running.
pub struct App {
    prefs: Prefs,
    /// Opened datasets; the first is the underlay shown.
    datasets: Vec<Dataset>,
    /// Last load error, shown in the status bar until the next load.
    error: Option<String>,
    /// Bumped whenever the shown dataset changes, to invalidate caches.
    generation: u64,
    view: ViewCard,
}

impl App {
    /// Start with `paths` loaded (and the demo phantom if `demo`).
    pub fn new(prefs: Prefs, paths: &[PathBuf], demo: bool) -> Self {
        let mut app = Self {
            prefs,
            datasets: Vec::new(),
            error: None,
            generation: 0,
            view: ViewCard::default(),
        };
        if demo {
            app.add(synthetic::phantom());
        }
        // Each opened dataset becomes the one shown, so open the first
        // argument (the underlay) last.
        for p in paths.iter().rev() {
            app.open(p);
        }
        app
    }

    /// Make `d` the dataset shown.
    fn add(&mut self, d: Dataset) {
        self.error = None;
        self.generation += 1;
        self.view.reset(&d);
        self.datasets.insert(0, d);
    }

    /// Load `path`, keeping the error for the status bar on failure.
    fn open(&mut self, path: &Path) {
        match load::load(path, self.prefs.sess_trail) {
            Ok(d) => self.add(d),
            Err(e) => self.error = Some(format!("{e:#}")),
        }
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
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
        }
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
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
            self.open(&p);
        }

        let mut action = None;
        egui::Panel::top("menu").show(ui, |ui| {
            action = shell::menu_bar(ui);
        });
        egui::Panel::top("toolbar").show(ui, |ui| {
            shell::toolbar(ui, &theme, self.datasets.first());
        });
        egui::Panel::bottom("status").show(ui, |ui| {
            shell::status_bar(ui, &theme, self.datasets.first(), self.error.as_deref());
        });
        if let Some(ds) = self.datasets.first() {
            self.view.handle_keys(&ctx, ds);
        }
        egui::CentralPanel::default().show(ui, |ui| match self.datasets.first() {
            Some(ds) => self
                .view
                .ui(ui, &theme, ds, self.generation, self.prefs.left_is_left),
            None => {
                ui.centered_and_justified(|ui| {
                    ui.label(egui::RichText::new("no dataset").color(theme.text_faint));
                });
            }
        });

        if let Some(a) = action {
            self.handle(&ctx, a);
        }
    }
}
