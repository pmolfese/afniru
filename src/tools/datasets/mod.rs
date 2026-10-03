//! Datasets tool: pick the underlay and its sub-brick, and see what it is.
//! (The overlay picker joins it in Milestone 4; window/level may move here.)

use egui::{Button, Color32, ComboBox, DragValue, Label, Rect, RichText, Sense, Ui, vec2};
use egui_phosphor::regular as icon;

use super::{Instance, Tool, ToolContext};
use crate::data::Source;
use crate::session::action::LoadRole;
use crate::session::{Action, LayerId, OverlayChange};

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
            }
            actions.extend(folder_section(ui, cx));
            return actions;
        };

        egui::Grid::new("datasets_grid")
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(RichText::new("ULay").color(theme.text_dim));
                let current = cx.controller.underlay;
                ComboBox::from_id_salt("ulay")
                    .width(ui.available_width())
                    .selected_text(&ds.name)
                    .show_ui(ui, |ui| {
                        for (id, d) in cx.session.store.iter() {
                            if ui.selectable_label(current == Some(id), &d.name).clicked() {
                                actions.push(Action::SetUnderlay(id));
                            }
                        }
                    });
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
        actions.extend(folder_section(ui, cx));
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
                LoadRole::Underlay => "ULay",
                LoadRole::Overlay => "Overlay",
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

/// The datasets found in each folder, each with buttons to open it as the
/// underlay or as a new overlay layer.
fn folder_section(ui: &mut Ui, cx: &ToolContext) -> Vec<Action> {
    let mut actions = Vec::new();
    let theme = cx.theme;
    for folder in cx.folders {
        ui.separator();
        ui.horizontal(|ui| {
            let name = folder.dir.file_name().map_or_else(
                || folder.dir.display().to_string(),
                |n| n.to_string_lossy().into(),
            );
            ui.label(
                RichText::new(format!("{} {name}", icon::FOLDER_OPEN))
                    .strong()
                    .color(theme.text),
            )
            .on_hover_text(folder.dir.display().to_string());
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
        let Some(entries) = &folder.entries else {
            ui.label(
                RichText::new("reading the folder…")
                    .small()
                    .color(theme.text_dim),
            );
            continue;
        };
        if let Some(e) = &folder.error {
            ui.label(RichText::new(e).small().color(theme.error));
            continue;
        }
        if entries.is_empty() {
            ui.label(
                RichText::new("no AFNI or NIfTI datasets here")
                    .small()
                    .color(theme.text_faint),
            );
            continue;
        }
        // A filter box for long lists.
        let filter_id = ui.id().with(("folder_filter", &folder.dir));
        let mut filter: String = ui.data(|d| d.get_temp(filter_id)).unwrap_or_default();
        if entries.len() > 8 {
            ui.add(
                egui::TextEdit::singleline(&mut filter)
                    .hint_text("filter")
                    .desired_width(f32::INFINITY),
            );
            ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
        }
        let needle = filter.to_lowercase();
        egui::ScrollArea::vertical()
            .id_salt(("folder_scroll", &folder.dir))
            .max_height(170.0)
            .auto_shrink([false, true])
            .show(ui, |ui| {
                for entry in entries
                    .iter()
                    .filter(|e| needle.is_empty() || e.label.to_lowercase().contains(&needle))
                {
                    let loaded = cx.session.store.iter().any(|(_, d)| {
                        matches!(&d.source, Source::File(p)
                            if p.to_string_lossy().starts_with(&*entry.path.to_string_lossy()))
                    });
                    ui.horizontal(|ui| {
                        let ink = if loaded { theme.text_faint } else { theme.text };
                        ui.add(
                            Label::new(RichText::new(&entry.label).monospace().small().color(ink))
                                .truncate(),
                        )
                        .on_hover_text(entry.path.display().to_string());
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui
                                .small_button("+ Ovl")
                                .on_hover_text("Add as a new overlay layer")
                                .clicked()
                            {
                                actions.push(Action::LoadDataset(
                                    entry.path.clone(),
                                    LoadRole::Overlay,
                                ));
                            }
                            if ui
                                .small_button("ULay")
                                .on_hover_text("Show as the underlay")
                                .clicked()
                            {
                                actions.push(Action::LoadDataset(
                                    entry.path.clone(),
                                    LoadRole::Underlay,
                                ));
                            }
                        });
                    });
                }
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
            ui.menu_button(format!("{} Add overlay", icon::PLUS), |ui| {
                for (id, d) in cx.session.store.iter() {
                    if ui.button(&d.name).clicked() {
                        actions.push(Action::AddOverlay(id));
                        ui.close();
                    }
                }
            });
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
