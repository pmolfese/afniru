//! afniru: a Rust rebuild of the AFNI volume viewer.
#![warn(missing_docs)]
// Skeleton stage: theme tokens and prefs are defined ahead of their users.
// Remove once M1/M2 consume them.
#![allow(dead_code)]

mod app;
mod data;
mod geom;
mod prefs;
mod render;
mod ui;

use std::path::PathBuf;

use clap::Parser;

/// View AFNI and NIfTI datasets.
#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Datasets to open (AFNI `.HEAD` or NIfTI); the first is the underlay.
    datasets: Vec<PathBuf>,

    /// Open the built-in demo phantom (no data needed).
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
        Box::new(|_cc| Ok(Box::new(app::App::new(prefs, &cli.datasets, cli.demo)))),
    )
}
