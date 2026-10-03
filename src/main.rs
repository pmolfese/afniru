//! afniru: a Rust rebuild of the AFNI volume viewer.
#![warn(missing_docs)]

mod analysis;
mod app;
mod data;
mod geom;
mod prefs;
mod processing;
mod render;
mod session;
#[cfg(test)]
mod testutil;
mod tools;
mod ui;

use std::path::PathBuf;

use clap::Parser;

/// View AFNI and NIfTI datasets.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Datasets to open (AFNI `.HEAD` or NIfTI); the first is the underlay and
    /// the second, if any, the overlay (`afniru anat+orig func+orig`).
    /// A directory is opened as an `afni_proc.py` results directory.
    datasets: Vec<PathBuf>,

    /// Open the built-in demo phantom with a fake t-map overlay (no data needed).
    #[arg(long)]
    demo: bool,
}

fn main() -> eframe::Result {
    let cli = Cli::parse();
    let prefs = prefs::Prefs::load_or_create();
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };
    eframe::run_native(
        "afniru",
        options,
        Box::new(|cc| {
            ui::fonts::ensure(&cc.egui_ctx);
            let (dirs, files): (Vec<PathBuf>, Vec<PathBuf>) =
                cli.datasets.iter().cloned().partition(|p| p.is_dir());
            let mut app = app::App::new(prefs, &files, cli.demo);
            // Look for an afni_proc.py run: directories given on the command
            // line, then the working directory and the folders of the files.
            let mut implicit: Vec<PathBuf> = std::env::current_dir().into_iter().collect();
            implicit.extend(files.iter().filter_map(|f| {
                // A bare file name has the empty path as its parent.
                f.parent().map(|p| {
                    if p.as_os_str().is_empty() {
                        PathBuf::from(".")
                    } else {
                        p.to_path_buf()
                    }
                })
            }));
            app.detect_processing(&dirs, &implicit);
            if let Some(storage) = cc.storage {
                app.restore(storage);
            }
            Ok(Box::new(app))
        }),
    )
}
