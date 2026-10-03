//! The window frame: menu bar, toolbar and status bar.
//!
//! These functions draw and return what the user asked for as an [`Action`];
//! they never touch the session themselves (see `app.rs`).

use egui::{Color32, Rect, RichText, Sense, Ui, pos2, vec2};

use super::theme::Theme;
use super::view_state::{Layout, ViewOptions};
use crate::data::{Dataset, Source};
use crate::loader::LoadingInfo;
use crate::session::{Action as SessionAction, Difference, Links, Session};
use egui_phosphor::regular as icon;

/// Something the user asked for in the shell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Action {
    /// File → Open…
    Open,
    /// File → Open demo phantom.
    OpenDemo,
    /// File → Open results directory…
    OpenResults,
    /// File → Open folder…
    OpenFolder,
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
            if ui.button("Open folder…").clicked() {
                action = Some(Action::OpenFolder);
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

/// What the toolbar needs to offer several controllers: compare, link, and
/// what differs between the two being compared. What the user asked for
/// is collected in `actions`.
pub struct Multi<'a> {
    /// How many controllers there are.
    pub count: usize,
    /// Show two side by side?
    pub compare: &'a mut bool,
    /// What is linked.
    pub links: Links,
    /// The two controllers compared (left, right), when comparing.
    pub pair: (usize, usize),
    /// What differs between them.
    pub differences: Vec<Difference>,
    /// Session actions the controls asked for.
    pub actions: Vec<SessionAction>,
}

/// The controls for several controllers: Compare, Link, and the differences chip.
fn multi_controls(ui: &mut Ui, theme: &Theme, multi: &mut Multi) {
    // Right to left: the rightmost comes first.
    let (a, b) = (
        Session::controller_name(multi.pair.0),
        Session::controller_name(multi.pair.1),
    );
    if *multi.compare && multi.count >= 2 {
        let n = multi.differences.len();
        let label = if n == 0 {
            format!("{a} = {b}")
        } else {
            format!("{a} ≠ {b} · {n}")
        };
        let color = if n == 0 { theme.text_dim } else { theme.accent };
        ui.menu_button(RichText::new(label).color(color), |ui| {
            ui.set_min_width(260.0);
            if multi.differences.is_empty() {
                ui.label(format!("{a} and {b} have the same settings."));
            }
            for d in &multi.differences {
                ui.horizontal_wrapped(|ui| {
                    ui.label(RichText::new(&d.what).small().color(theme.text_dim));
                    ui.label(
                        RichText::new(format!("{a}: {}  {b}: {}", d.a, d.b))
                            .small()
                            .monospace(),
                    );
                });
            }
            ui.separator();
            if ui.button(format!("Make {b} like {a}")).clicked() {
                multi.actions.push(SessionAction::CloneController {
                    from: multi.pair.0,
                    to: multi.pair.1,
                });
                ui.close();
            }
            if ui.button(format!("Make {a} like {b}")).clicked() {
                multi.actions.push(SessionAction::CloneController {
                    from: multi.pair.1,
                    to: multi.pair.0,
                });
                ui.close();
            }
        })
        .response
        .on_hover_text("What differs between the two controllers");
    }
    ui.menu_button(icon::LINK, |ui| {
        let mut links = multi.links;
        ui.checkbox(&mut links.crosshair, "Crosshair and slices")
            .on_hover_text("Moving the crosshair in one controller moves it in the others");
        ui.checkbox(&mut links.zoom, "Zoom and pan")
            .on_hover_text("Zooming or panning one controller does the same in the others");
        if links != multi.links {
            multi.actions.push(SessionAction::SetLinks(links));
        }
    })
    .response
    .on_hover_text("Link the controllers");
    if multi.count >= 2 {
        ui.toggle_value(multi.compare, "Compare")
            .on_hover_text("Show controllers A and B side by side");
    }
    ui.separator();
}

/// Toolbar: a breadcrumb of what is being viewed on the left; on the right
/// the R↔L and crosshair toggles and the layout switcher.
pub fn toolbar(
    ui: &mut Ui,
    theme: &Theme,
    current: Option<&Dataset>,
    options: &mut ViewOptions,
    multi: Option<&mut Multi>,
) {
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
            if let Some(multi) = multi {
                multi_controls(ui, theme, multi);
            }
            for layout in [Layout::Grid, Layout::Column, Layout::Row] {
                if layout_button(ui, theme, layout, options.layout == layout).clicked() {
                    options.layout = layout;
                }
            }
            ui.separator();
            ui.toggle_value(&mut options.crosshair, "Xhairs")
                .on_hover_text("Show the crosshair lines");
            // The label reads the screen from left to right: R↔L is radiological
            // (the subject's right on the screen's left), L↔R is neurological
            // (left on left), and the lit button marks the swapped one.
            let (label, mode) = if options.left_is_left {
                (
                    "L↔R",
                    "neurological: the subject's left is on the screen's left",
                )
            } else {
                (
                    "R↔L",
                    "radiological: the subject's right is on the screen's left",
                )
            };
            ui.toggle_value(&mut options.left_is_left, label)
                .on_hover_text(format!("{mode}\nClick to swap left and right"));
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
    loading: &[LoadingInfo],
    notice: Option<&str>,
) {
    ui.horizontal(|ui| {
        if let Some(first) = loading.first() {
            ui.add(
                egui::ProgressBar::new(0.0)
                    .desired_width(110.0)
                    .animate(true),
            );
            let more = if loading.len() > 1 {
                format!(" (+{} more)", loading.len() - 1)
            } else {
                String::new()
            };
            ui.label(
                RichText::new(format!("Loading {}…{more}", loading_text(first)))
                    .color(theme.accent),
            );
        } else if let Some(n) = notice.filter(|_| error.is_none()) {
            ui.label(RichText::new(n).color(theme.good));
        } else if let Some(e) = error {
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

/// `anat+tlrc  1.2 GB  12 s` for a load in progress.
pub fn loading_text(l: &LoadingInfo) -> String {
    let size = l
        .bytes
        .map_or(String::new(), |b| format!("  {}", format_bytes(b)));
    format!("{}{size}  {} s", l.name, l.elapsed.as_secs())
}

/// `512 KB`, `1.2 GB`.
pub fn format_bytes(b: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut v = b as f64;
    let mut unit = 0;
    while v >= 1024.0 && unit + 1 < UNITS.len() {
        v /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{b} B")
    } else {
        format!("{v:.1} {}", UNITS[unit])
    }
}
