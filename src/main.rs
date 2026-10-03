//! afniru: a Rust rebuild of the AFNI volume viewer.
#![warn(missing_docs)]

mod analysis;
mod app;
mod data;
mod export_dialog;
mod geom;
mod loader;
mod prefs;
mod processing;
mod recent;
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
    /// Datasets to open (AFNI `.HEAD` or NIfTI): the first is the underlay and
    /// the others become overlay layers, in order (`afniru anat+tlrc stats+tlrc`).
    /// A directory lists its datasets (AFNI and NIfTI) in the Datasets card to
    /// pick from, and is also opened as an `afni_proc.py` results directory if
    /// it is one.
    datasets: Vec<PathBuf>,

    /// List the datasets of the folders given, and of all their subfolders,
    /// each shown under its subfolder's name (`-R` or `-r`).
    #[arg(short = 'R', short_alias = 'r', long = "recursive")]
    recursive: bool,

    /// Open the built-in demo phantom with a fake t-map overlay (no data needed).
    #[arg(long)]
    demo: bool,
}

fn main() -> eframe::Result {
    let cli = Cli::parse();
    let prefs = prefs::Prefs::load_or_create();
    // The AFNI logo, as the window and dock icon (a bad file only loses the icon).
    let mut viewport = egui::ViewportBuilder::default()
        .with_inner_size([1280.0, 800.0])
        .with_drag_and_drop(true);
    if let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!("../assets/afni_icon.png")) {
        viewport = viewport.with_icon(icon);
    }
    let options = eframe::NativeOptions {
        viewport,
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
            app.set_recursive_listing(cli.recursive);
            for dir in &dirs {
                app.add_folder(dir);
            }
            // Look for an afni_proc.py run: directories given on the command
            // line, then the working directory and the folders of the files.
            // Failing to find one is silent: a directory is also just a folder.
            let mut implicit: Vec<PathBuf> = dirs.clone();
            implicit.extend(std::env::current_dir());
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
            app.detect_processing(&[], &implicit);
            if let Some(storage) = cc.storage {
                app.restore(storage);
            }
            Ok(Box::new(app))
        }),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn r_and_capital_r_both_ask_for_a_recursive_listing() {
        for flag in ["-R", "-r", "--recursive"] {
            let cli = Cli::try_parse_from(["afniru", flag, "results"]).unwrap();
            assert!(cli.recursive, "{flag}");
            assert_eq!(cli.datasets, [PathBuf::from("results")]);
        }
        let cli = Cli::try_parse_from(["afniru", "anat+tlrc", "stats+tlrc"]).unwrap();
        assert!(!cli.recursive);
        assert_eq!(cli.datasets.len(), 2);
    }
}
