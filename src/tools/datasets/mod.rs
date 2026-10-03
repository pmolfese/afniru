//! Datasets tool: pick the underlay and its sub-brick, and see what it is.
//! (The overlay picker joins it in Milestone 4; window/level may move here.)

use egui::{Button, Color32, ComboBox, DragValue, Label, Rect, RichText, Sense, Ui, vec2};
use egui_phosphor::regular as icon;

use super::{Instance, Tool, ToolContext};
use crate::data::Source;
use crate::session::action::LoadRole;
use std::path::{Path, PathBuf};

use crate::data::Dataset;
use crate::recent::RecentKind;
use crate::session::series::SeriesChange;
use crate::session::{Action, DatasetId, LayerId, OverlayChange};

/// The Datasets tool. Pinned: it is always in the card stack.
pub struct DatasetsTool;

impl Tool for DatasetsTool {
    fn pinned(&self) -> bool {
        true
    }

    fn card_ui(&self, ui: &mut Ui, cx: &ToolContext, _instance: &Instance) -> Vec<Action> {
        let mut actions = Vec::new();
        let theme = cx.theme;
        actions.extend(loading_rows(ui, cx));
        let Some(ds) = cx.dataset else {
            if cx.loading.is_empty() && cx.folders.is_empty() {
                ui.label(
                    RichText::new("No dataset: File ▸ Open…, or drop one on the window.")
                        .color(theme.text_dim),
                );
            } else {
                ui.horizontal(|ui| {
                    ui.label(RichText::new("ULay").color(theme.text_dim));
                    dataset_combo(
                        ui,
                        cx,
                        "ulay",
                        ui.available_width(),
                        "choose a dataset",
                        None,
                        &Picks::underlay(),
                        &mut actions,
                    );
                });
            }
            actions.extend(folder_rows(ui, cx));
            return actions;
        };

        egui::Grid::new("datasets_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(RichText::new("ULay").color(theme.text_dim));
                let current = cx.controller.underlay;
                dataset_combo(
                    ui,
                    cx,
                    "ulay",
                    ui.available_width(),
                    &ds.name,
                    current,
                    &Picks::underlay(),
                    &mut actions,
                );
                ui.end_row();

                ui.label(RichText::new("Sub-brick").color(theme.text_dim));
                let shown = cx.controller.underlay_sub_brick;
                ComboBox::from_id_salt("ulay_sub")
                    .width(ui.available_width())
                    .selected_text(sub_brick_text(ds, shown))
                    .show_ui(ui, |ui| {
                        for t in 0..ds.nvols {
                            if ui
                                .selectable_label(shown == t, sub_brick_text(ds, t))
                                .clicked()
                            {
                                actions.push(Action::SetUnderlaySubBrick(t));
                            }
                        }
                    });
                ui.end_row();
            });

        ui.add_space(2.0);
        ui.label(RichText::new(ds.summary()).color(theme.text_dim).small());
        ui.separator();
        actions.extend(layer_list(ui, cx));
        actions.extend(folder_rows(ui, cx));
        actions
    }

    fn summary(&self, cx: &ToolContext, _instance: &Instance) -> String {
        let n = cx.overlays.len();
        let base = cx.dataset.map_or("none".into(), |d| d.name.clone());
        match n {
            0 => base,
            1 => format!("{base} + 1 overlay"),
            n => format!("{base} + {n} overlays"),
        }
    }
}

/// One row for each dataset being read: its name and size, an animated bar,
/// the time so far, and a button to stop waiting.
fn loading_rows(ui: &mut Ui, cx: &ToolContext) -> Vec<Action> {
    let mut actions = Vec::new();
    for l in cx.loading {
        ui.horizontal(|ui| {
            let what = match l.role {
                LoadRole::Underlay => "ULay".to_string(),
                LoadRole::Overlay => "Overlay".to_string(),
                LoadRole::Layer(l) => format!("Overlay {}", l.0),
                LoadRole::GraphSource => "Graph".to_string(),
                LoadRole::GraphFit => "Graph fit".to_string(),
            };
            let text = if l.waiting {
                format!("{what} {} · read, waiting for the one before", l.name)
            } else {
                format!("{what} {}", crate::ui::shell::loading_text(l))
            };
            ui.label(RichText::new(text).small().color(cx.theme.accent));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(Button::new(RichText::new(icon::X)).frame(false))
                    .on_hover_text("Stop waiting for this dataset")
                    .clicked()
                {
                    actions.push(Action::CancelLoad(l.id));
                }
            });
        });
        ui.add(
            egui::ProgressBar::new(0.0)
                .desired_height(4.0)
                .animate(true),
        );
        ui.add_space(2.0);
    }
    actions
}

/// Which loaded datasets a picker offers.
type DatasetFilter = Box<dyn Fn(&Dataset) -> bool>;

/// An item before the datasets in a picker ("none", "the underlay").
pub(crate) struct Leading {
    /// Its text.
    pub label: String,
    /// Is it the current choice?
    pub selected: bool,
    /// What choosing it does.
    pub action: Action,
}

/// What choosing a dataset in a picker does: for one already loaded, and for
/// one only listed in a folder or remembered (which is read from disk then).
pub(crate) struct Picks {
    loaded: Box<dyn Fn(DatasetId) -> Action>,
    from_folder: Box<dyn Fn(&Path) -> Action>,
    /// Which recent list the picker shows.
    recent: RecentKind,
    /// Which loaded datasets are offered (all, if `None`).
    only: Option<DatasetFilter>,
    /// Items before the datasets.
    leading: Vec<Leading>,
}

impl Picks {
    fn new(
        loaded: impl Fn(DatasetId) -> Action + 'static,
        from_folder: impl Fn(&Path) -> Action + 'static,
        recent: RecentKind,
    ) -> Self {
        Self {
            loaded: Box::new(loaded),
            from_folder: Box::new(from_folder),
            recent,
            only: None,
            leading: Vec::new(),
        }
    }

    /// Make the dataset the underlay.
    pub(crate) fn underlay() -> Self {
        Self::new(
            Action::SetUnderlay,
            |p| Action::LoadDataset(p.to_path_buf(), LoadRole::Underlay),
            RecentKind::Underlay,
        )
    }

    /// Add the dataset as a new overlay layer.
    pub(crate) fn new_overlay() -> Self {
        Self::new(
            Action::AddOverlay,
            |p| Action::LoadDataset(p.to_path_buf(), LoadRole::Overlay),
            RecentKind::Overlay,
        )
    }

    /// Make the dataset the one drawn by an existing layer.
    pub(crate) fn layer(layer: LayerId) -> Self {
        Self::new(
            move |id| Action::Layer(layer, OverlayChange::Dataset(id)),
            move |p| Action::LoadDataset(p.to_path_buf(), LoadRole::Layer(layer)),
            RecentKind::Overlay,
        )
    }

    /// The dataset the Graph plots: any dataset with several time points, or
    /// the underlay (`source` is the current choice).
    pub(crate) fn graph_source(source: Option<DatasetId>) -> Self {
        let mut p = Self::new(
            |id| Action::Series(SeriesChange::Source(Some(id))),
            |p| Action::LoadDataset(p.to_path_buf(), LoadRole::GraphSource),
            RecentKind::Graph,
        );
        p.only = Some(Box::new(|d| d.nvols > 1));
        p.leading.push(Leading {
            label: "ULay (the underlay)".into(),
            selected: source.is_none(),
            action: Action::Series(SeriesChange::Source(None)),
        });
        p
    }

    /// The Graph's fit: any dataset with `len` time points, or none.
    pub(crate) fn graph_fit(len: usize, fit: Option<DatasetId>) -> Self {
        let mut p = Self::new(
            |id| Action::Series(SeriesChange::Fit(Some(id))),
            |p| Action::LoadDataset(p.to_path_buf(), LoadRole::GraphFit),
            RecentKind::Graph,
        );
        p.only = Some(Box::new(move |d| len > 1 && d.nvols == len));
        p.leading.push(Leading {
            label: "none".into(),
            selected: fit.is_none(),
            action: Action::Series(SeriesChange::Fit(None)),
        });
        p
    }
}

/// Is a dataset from this path already in memory?
fn is_loaded(cx: &ToolContext, path: &Path) -> bool {
    let wanted = path.to_string_lossy();
    cx.session.store.iter().any(
        |(_, d)| matches!(&d.source, Source::File(p) if p.to_string_lossy().starts_with(&*wanted)),
    )
}

/// A combo box that picks a dataset (see [`dataset_items`]). It stays open
/// while you type in its filter box and closes when you choose.
#[allow(clippy::too_many_arguments)]
pub(crate) fn dataset_combo(
    ui: &mut Ui,
    cx: &ToolContext,
    id: impl std::hash::Hash + std::fmt::Debug,
    width: f32,
    selected: &str,
    current: Option<DatasetId>,
    picks: &Picks,
    actions: &mut Vec<Action>,
) {
    ComboBox::from_id_salt(id)
        .width(width)
        .selected_text(selected)
        .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
        .show_ui(ui, |ui| dataset_items(ui, cx, current, picks, actions));
}

/// A menu button that picks a dataset (see [`dataset_items`]); like the combo
/// box it stays open while you use its filter box.
pub(crate) fn dataset_menu(
    ui: &mut Ui,
    cx: &ToolContext,
    label: &str,
    picks: &Picks,
    actions: &mut Vec<Action>,
) -> egui::Response {
    egui::containers::menu::MenuButton::new(label)
        .config(
            egui::containers::menu::MenuConfig::default()
                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
        )
        .ui(ui, |ui| dataset_items(ui, cx, None, picks, actions))
        .0
}

/// The items of a dataset picker (inside a combo box or menu): the leading
/// choices, the datasets already loaded, the recent ones, then, per folder, the
/// ones not loaded yet (they are read when chosen). A filter box appears when
/// the list is long.
pub(crate) fn dataset_items(
    ui: &mut Ui,
    cx: &ToolContext,
    current: Option<DatasetId>,
    picks: &Picks,
    actions: &mut Vec<Action>,
) {
    let theme = cx.theme;
    let recent: Vec<&PathBuf> = cx
        .recents
        .list(picks.recent)
        .iter()
        .filter(|p| !is_loaded(cx, p))
        .collect();
    let unloaded: usize = cx
        .folders
        .iter()
        .filter_map(|f| f.entries.as_ref())
        .map(Vec::len)
        .sum();
    let filter_id = ui.id().with("dataset_filter");
    let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
    if unloaded + recent.len() + cx.session.store.iter().count() > 12 {
        ui.add(
            egui::TextEdit::singleline(&mut filter)
                .hint_text("filter")
                .desired_width(240.0),
        );
        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
    } else {
        filter.clear();
    }
    let needle = filter.to_lowercase();
    let shown = |name: &str| needle.is_empty() || name.to_lowercase().contains(&needle);

    for lead in &picks.leading {
        if ui.selectable_label(lead.selected, &lead.label).clicked() {
            actions.push(lead.action.clone());
            ui.close();
        }
    }
    egui::ScrollArea::vertical()
        .max_height(320.0)
        .auto_shrink([true, true])
        .show(ui, |ui| {
            let mut any = !picks.leading.is_empty();
            for (id, d) in cx.session.store.iter() {
                if shown(&d.name) && picks.only.as_ref().is_none_or(|ok| ok(d)) {
                    any = true;
                    if ui
                        .selectable_label(current == Some(id), &d.name)
                        .on_hover_text("Already loaded")
                        .clicked()
                    {
                        actions.push((picks.loaded)(id));
                        ui.close();
                    }
                }
            }
            let mut section = |ui: &mut Ui, heading: String, entries: Vec<(String, &Path)>| {
                if entries.is_empty() {
                    return;
                }
                any = true;
                ui.separator();
                ui.label(RichText::new(heading).small().color(theme.text_faint));
                for (label, path) in entries {
                    if ui
                        .selectable_label(false, label)
                        .on_hover_text(format!("Load {}", path.display()))
                        .clicked()
                    {
                        actions.push((picks.from_folder)(path));
                        ui.close();
                    }
                }
            };
            section(
                ui,
                format!("{} Recent", icon::CLOCK_COUNTER_CLOCKWISE),
                recent
                    .iter()
                    .map(|p| (crate::recent::display_name(p), p.as_path()))
                    .filter(|(label, _)| shown(label))
                    .collect(),
            );
            for folder in cx.folders {
                let Some(entries) = &folder.entries else {
                    continue;
                };
                let name = folder.dir.file_name().map_or_else(
                    || folder.dir.display().to_string(),
                    |n| n.to_string_lossy().into(),
                );
                section(
                    ui,
                    format!("{} {name}", icon::FOLDER_OPEN),
                    entries
                        .iter()
                        .filter(|e| {
                            shown(&e.label)
                                && !is_loaded(cx, &e.path)
                                && !recent.iter().any(|r| **r == e.path)
                        })
                        .map(|e| (e.label.clone(), e.path.as_path()))
                        .collect(),
                );
            }
            if !any {
                ui.label(
                    RichText::new("nothing to choose")
                        .small()
                        .color(theme.text_faint),
                );
            }
        });
}

/// One row for each folder being listed: its name, how many datasets it has,
/// and buttons to read it again and to stop listing it. (Datasets are chosen
/// from the pickers.)
fn folder_rows(ui: &mut Ui, cx: &ToolContext) -> Vec<Action> {
    let mut actions = Vec::new();
    let theme = cx.theme;
    for folder in cx.folders {
        ui.horizontal(|ui| {
            let name = folder.dir.file_name().map_or_else(
                || folder.dir.display().to_string(),
                |n| n.to_string_lossy().into(),
            );
            let count = match (&folder.entries, &folder.error) {
                (_, Some(e)) => e.clone(),
                (None, _) => "reading…".to_string(),
                (Some(e), _) if e.is_empty() => "no AFNI or NIfTI datasets".to_string(),
                (Some(e), _) => format!("{} datasets", e.len()),
            };
            ui.label(
                RichText::new(format!("{} {name}", icon::FOLDER_OPEN))
                    .small()
                    .color(theme.text_dim),
            )
            .on_hover_text(folder.dir.display().to_string());
            ui.label(
                RichText::new(count)
                    .small()
                    .color(if folder.error.is_some() {
                        theme.error
                    } else {
                        theme.text_faint
                    }),
            );
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(Button::new(RichText::new(icon::X)).frame(false))
                    .on_hover_text("Stop listing this folder")
                    .clicked()
                {
                    actions.push(Action::CloseFolder(folder.dir.clone()));
                }
                if ui
                    .add(Button::new(RichText::new(icon::ARROWS_CLOCKWISE)).frame(false))
                    .on_hover_text("Read the folder again")
                    .clicked()
                {
                    actions.push(Action::ScanFolder(folder.dir.clone()));
                }
            });
        });
    }
    actions
}

/// The overlay layers, top (drawn last) first: eye, swatch, name, opacity, a
/// handle to drag to restack, and remove; and "+ Add overlay".
fn layer_list(ui: &mut Ui, cx: &ToolContext) -> Vec<Action> {
    let mut actions = Vec::new();
    let theme = cx.theme;
    ui.horizontal(|ui| {
        ui.label(RichText::new("Overlays").strong().color(theme.text));
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            dataset_menu(
                ui,
                cx,
                &format!("{} Add overlay", icon::PLUS),
                &Picks::new_overlay(),
                &mut actions,
            );
        });
    });
    if cx.overlays.is_empty() {
        ui.label(
            RichText::new("none: color maps drawn over the underlay")
                .small()
                .color(theme.text_faint),
        );
        return actions;
    }
    let drag_id = egui::Id::new("overlay_layer_drag");
    let mut rows: Vec<(LayerId, Rect)> = Vec::new();
    let mut released = false;
    for o in cx.overlays.iter().rev() {
        let layer = o.layer;
        let row = ui
            .horizontal(|ui| {
                let handle = ui
                    .add(
                        Label::new(RichText::new(icon::DOTS_SIX_VERTICAL).color(theme.text_faint))
                            .sense(Sense::drag()),
                    )
                    .on_hover_cursor(egui::CursorIcon::Grab);
                if handle.drag_started() {
                    ui.data_mut(|d| d.insert_temp(drag_id, layer.id));
                }
                released |= handle.drag_stopped();
                let eye = if layer.visible {
                    icon::EYE
                } else {
                    icon::EYE_SLASH
                };
                if ui
                    .add(Button::new(RichText::new(eye)).frame(false))
                    .on_hover_text(if layer.visible {
                        "Hide this layer"
                    } else {
                        "Show this layer"
                    })
                    .clicked()
                {
                    actions.push(Action::Layer(
                        layer.id,
                        OverlayChange::Visible(!layer.visible),
                    ));
                }
                // A swatch: the color scale, or the one color of a mask.
                let (rect, _) = ui.allocate_exact_size(vec2(26.0, 10.0), Sense::hover());
                if layer.as_mask {
                    let [r, g, b] = layer.mask.color;
                    ui.painter()
                        .rect_filled(rect, 2.0, Color32::from_rgb(r, g, b));
                } else if let Ok(map) = layer.colorscale.to_color_map(256) {
                    for i in 0..13 {
                        let c = map.sample(i as f64 / 12.0).to_u8();
                        let w = rect.width() / 13.0;
                        ui.painter().rect_filled(
                            Rect::from_min_size(
                                rect.min + vec2(i as f32 * w, 0.0),
                                vec2(w + 0.5, rect.height()),
                            ),
                            0.0,
                            Color32::from_rgb(c[0], c[1], c[2]),
                        );
                    }
                }
                let name = RichText::new(format!("{} {}", layer.id.0, o.dataset.name)).color(
                    if layer.visible {
                        theme.text
                    } else {
                        theme.text_faint
                    },
                );
                ui.add(Label::new(name).truncate());
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui
                        .add(Button::new(RichText::new(icon::TRASH)).frame(false))
                        .on_hover_text("Remove this overlay")
                        .clicked()
                    {
                        actions.push(Action::RemoveOverlay(layer.id));
                    }
                    let mut pct = layer.opacity * 100.0;
                    if ui
                        .add(
                            DragValue::new(&mut pct)
                                .range(0.0..=100.0)
                                .speed(1.0)
                                .suffix("%")
                                .max_decimals(0),
                        )
                        .on_hover_text("Opacity")
                        .changed()
                    {
                        actions.push(Action::Layer(layer.id, OverlayChange::Opacity(pct / 100.0)));
                    }
                });
            })
            .response
            .rect;
        rows.push((layer.id, row));
    }
    // Drag to restack: the layer lands where the pointer is.
    let dragging: Option<LayerId> = ui.data(|d| d.get_temp(drag_id));
    if let Some(id) = dragging {
        let others: Vec<Rect> = rows
            .iter()
            .filter(|(l, _)| *l != id)
            .map(|(_, r)| *r)
            .collect();
        let slot = ui
            .ctx()
            .pointer_latest_pos()
            .map_or(0, |p| others.iter().filter(|r| r.center().y < p.y).count());
        let line_y = match others.get(slot) {
            Some(r) => r.top() - 2.0,
            None => others.last().map_or(0.0, |r| r.bottom() + 2.0),
        };
        if let Some(first) = rows.first() {
            ui.painter().line_segment(
                [
                    egui::pos2(first.1.left(), line_y),
                    egui::pos2(first.1.right(), line_y),
                ],
                egui::Stroke::new(2.0, theme.accent),
            );
        }
        if released || !ui.ctx().input(|i| i.pointer.any_down()) {
            ui.data_mut(|d| d.remove_by_type::<LayerId>());
            actions.push(Action::MoveOverlay {
                id,
                to: stack_position(others.len(), slot),
            });
        }
    }
    actions
}

/// The position in the bottom-first stack for a layer dropped at `slot` of
/// the top-first list of the other `others` layers (slot 0 = the very top).
pub fn stack_position(others: usize, slot: usize) -> usize {
    others - slot.min(others)
}

/// `#3 label`, as AFNI's sub-brick chooser writes it.
pub(crate) fn sub_brick_text(ds: &crate::data::Dataset, t: usize) -> String {
    match ds.labels.get(t) {
        Some(l) if *l != format!("#{t}") => format!("#{t} {l}"),
        _ => format!("#{t}"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::synthetic;

    #[test]
    fn sub_brick_text_does_not_repeat_default_labels() {
        let mut ds = synthetic::phantom();
        assert_eq!(sub_brick_text(&ds, 0), "#0 anat");
        ds.labels = vec!["#0".into()];
        assert_eq!(sub_brick_text(&ds, 0), "#0");
        assert_eq!(sub_brick_text(&ds, 7), "#7");
    }

    #[test]
    fn dropping_at_the_top_slot_puts_the_layer_on_top() {
        // 3 layers; one is dragged, the other 2 stay: slot 0 is above both.
        assert_eq!(stack_position(2, 0), 2); // top of a 3-layer stack (index 2)
        assert_eq!(stack_position(2, 1), 1);
        assert_eq!(stack_position(2, 2), 0); // below both: the bottom
        assert_eq!(stack_position(2, 9), 0); // clamped
        assert_eq!(stack_position(0, 0), 0);
    }
}
