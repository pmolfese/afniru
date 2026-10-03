//! The window frame: menu bar, toolbar and status bar.
//!
//! These functions draw and return what the user asked for as an [`Action`];
//! they never touch the session themselves (see `app.rs`).

use egui::{RichText, Ui};

use super::theme::Theme;
use crate::data::Dataset;

/// Something the user asked for in the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// File → Open…
    Open,
    /// File → Open demo phantom.
    OpenDemo,
    /// File → Quit.
    Quit,
}

/// The in-window menu bar. Native menus are not a goal.
pub fn menu_bar(ui: &mut Ui) -> Option<Action> {
    let mut action = None;
    egui::MenuBar::new().ui(ui, |ui| {
        ui.menu_button("File", |ui| {
            if ui.button("Open…").clicked() {
                action = Some(Action::Open);
                ui.close();
            }
            if ui.button("Open demo phantom").clicked() {
                action = Some(Action::OpenDemo);
                ui.close();
            }
            ui.separator();
            if ui.button("Quit").clicked() {
                action = Some(Action::Quit);
                ui.close();
            }
        });
    });
    action
}

/// Toolbar: a breadcrumb of what is being viewed on the left, and a
/// placeholder for the layout switcher on the right (M2).
pub fn toolbar(ui: &mut Ui, theme: &Theme, current: Option<&Dataset>) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("afniru").color(theme.accent).strong());
        ui.label(RichText::new("›").color(theme.text_faint));
        match current {
            Some(d) => ui.label(RichText::new(&d.name).color(theme.text)),
            None => ui.label(RichText::new("no dataset").color(theme.text_dim)),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new("layout: 2×2 (soon)").color(theme.text_faint));
        });
    });
}

/// Status bar: dataset summary, or the last error.
pub fn status_bar(ui: &mut Ui, theme: &Theme, current: Option<&Dataset>, error: Option<&str>) {
    ui.horizontal(|ui| {
        if let Some(e) = error {
            ui.label(RichText::new(e).color(theme.error));
        } else if let Some(d) = current {
            ui.label(RichText::new(d.summary()).color(theme.text_dim));
        } else {
            ui.label(
                RichText::new("Open a dataset: File ▸ Open…, drop it on the window, or pass it on the command line")
                    .color(theme.text_faint),
            );
        }
    });
}
