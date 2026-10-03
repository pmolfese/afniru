//! The window frame: menu bar, toolbar and status bar.
//!
//! These functions draw and return what the user asked for as an [`Action`];
//! they never touch the session themselves (see `app.rs`).

use egui::{Color32, Rect, RichText, Sense, Ui, pos2, vec2};

use super::theme::Theme;
use super::view_state::{Layout, ViewOptions};
use crate::data::{Dataset, Source};

/// Something the user asked for in the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// File → Open…
    Open,
    /// File → Open demo phantom.
    OpenDemo,
    /// File → Open results directory…
    OpenResults,
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
            if ui.button("Open results directory…").clicked() {
                action = Some(Action::OpenResults);
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

/// Toolbar: a breadcrumb of what is being viewed on the left; on the right
/// the R↔L and crosshair toggles and the layout switcher.
pub fn toolbar(ui: &mut Ui, theme: &Theme, current: Option<&Dataset>, options: &mut ViewOptions) {
    ui.horizontal(|ui| {
        ui.label(RichText::new("afniru").color(theme.accent).strong());
        ui.label(RichText::new("›").color(theme.text_faint));
        match current {
            Some(d) => {
                let r = ui.label(RichText::new(&d.name).color(theme.text));
                match &d.source {
                    Source::File(path) => r.on_hover_text(path.display().to_string()),
                    Source::Synthetic => r.on_hover_text("built-in demo phantom"),
                }
            }
            None => ui.label(RichText::new("no dataset").color(theme.text_dim)),
        };
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            // Right-to-left: the rightmost widget comes first.
            for layout in [Layout::Grid, Layout::Column, Layout::Row] {
                if layout_button(ui, theme, layout, options.layout == layout).clicked() {
                    options.layout = layout;
                }
            }
            ui.separator();
            ui.toggle_value(&mut options.crosshair, "Xhairs")
                .on_hover_text("Show the crosshair lines");
            ui.toggle_value(&mut options.left_is_left, "R↔L")
                .on_hover_text("Swap left and right: radiological ↔ neurological");
        });
    });
}

/// A layout switcher button with a small painted icon (the default fonts have
/// no suitable glyphs).
fn layout_button(ui: &mut Ui, theme: &Theme, layout: Layout, selected: bool) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(vec2(30.0, 22.0), Sense::click());
    let response = response.on_hover_text(match layout {
        Layout::Row => "1×3: three views side by side",
        Layout::Column => "3×1: three views stacked",
        Layout::Grid => "2×2: three views and the Graph",
    });
    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, 4.0, theme.accent);
    } else if response.hovered() {
        painter.rect_filled(rect, 4.0, theme.card_hi);
    }
    let ink = if selected {
        Color32::BLACK
    } else {
        theme.text_dim
    };
    let icon = Rect::from_center_size(rect.center(), vec2(16.0, 12.0));
    let (cols, rows) = match layout {
        Layout::Row => (3, 1),
        Layout::Column => (1, 3),
        Layout::Grid => (2, 2),
    };
    let gap = 2.0;
    let (cw, ch) = (
        (icon.width() - gap * (cols - 1) as f32) / cols as f32,
        (icon.height() - gap * (rows - 1) as f32) / rows as f32,
    );
    for r in 0..rows {
        for c in 0..cols {
            let min = pos2(
                icon.left() + c as f32 * (cw + gap),
                icon.top() + r as f32 * (ch + gap),
            );
            painter.rect_filled(Rect::from_min_size(min, vec2(cw, ch)), 1.0, ink);
        }
    }
    response
}

/// Status bar: dataset summary, or the last error.
pub fn status_bar(
    ui: &mut Ui,
    theme: &Theme,
    current: Option<&Dataset>,
    error: Option<&str>,
    conventions: &str,
) {
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
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            ui.label(RichText::new(conventions).color(theme.text_faint));
        });
    });
}
