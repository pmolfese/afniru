//! The controller sidebar: tabs, workspace menu, tool shelf, card stack, and
//! the collapsed rail.
//!
//! [`ControllerUi`] is the sidebar's own state (workspaces and whether it is
//! collapsed); it is saved with the app. The session never sees it: cards
//! only return [`Action`]s.

pub mod card_frame;
pub mod hooks;
pub mod rail;
pub mod shelf;
pub mod workspace;

use egui::{Button, ComboBox, Frame, Margin, Panel, Rect, RichText, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as icon;
use serde::{Deserialize, Serialize};

use card_frame::{CardHeader, show_card};
use workspace::Workspaces;

use crate::session::{Action, OverlayChange, graph};
use crate::tools::{Instance, ToolContext, ToolId};

/// Storage key for the persisted controller state.
pub const STORAGE_KEY: &str = "afniru_controller";

/// The controller sidebar's state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ControllerUi {
    /// Named card arrangements; the current one is edited live.
    pub workspaces: Workspaces,
    /// Collapsed to the icon rail?
    pub rail: bool,
    /// The card open as a pop-over from the rail, with its icon's y position.
    #[serde(skip)]
    pub popover: Option<(ToolId, f32)>,
    /// The card being dragged by its handle.
    #[serde(skip)]
    dragging: Option<ToolId>,
    /// Text box content in the workspace menu.
    #[serde(skip)]
    save_name: String,
    /// Fold state of the cards of multi-card tools (one per overlay layer);
    /// not saved, because layers belong to the session.
    #[serde(skip)]
    pub(crate) instance_collapsed: std::collections::HashMap<(ToolId, u64), bool>,
    /// Groups (a card and the cards hooked under it) folded into one line,
    /// by the parent's tool and instance.
    #[serde(skip)]
    pub(crate) group_folded: std::collections::HashSet<(ToolId, u64)>,
}

impl ControllerUi {
    /// Repair state loaded from an older file.
    pub fn normalized(mut self) -> Self {
        self.workspaces.normalize();
        self
    }

    /// Show the controller as a left panel: the full sidebar, or the rail.
    /// Returns what the cards asked for.
    pub fn panel(&mut self, ui: &mut Ui, cx: &ToolContext) -> Vec<Action> {
        let actions = self.panel_inner(ui, cx);
        // Hooking Clusterize under a layer (from the chip in its card) shows
        // the Clusterize card even if its tile was off.
        if actions
            .iter()
            .any(|a| matches!(a, Action::Layer(_, OverlayChange::Cluster(Some(_)))))
        {
            self.workspaces.current_mut().show(ToolId::Clusterize);
        }
        actions
    }

    fn panel_inner(&mut self, ui: &mut Ui, cx: &ToolContext) -> Vec<Action> {
        let frame = Frame::new()
            .fill(cx.theme.panel)
            .inner_margin(Margin::same(10));
        if self.rail {
            let mut actions = Vec::new();
            Panel::left("controller_rail")
                .resizable(false)
                .exact_size(rail::WIDTH)
                .frame(frame.inner_margin(Margin::symmetric(6, 10)))
                .show(ui, |ui| {
                    let events =
                        rail::rail(ui, cx, self.workspaces.current_mut(), &mut self.popover);
                    actions = events.actions;
                    if events.expand {
                        self.rail = false;
                        self.popover = None;
                    }
                });
            actions
        } else {
            let mut actions = Vec::new();
            Panel::left("controller")
                .resizable(true)
                .default_size(340.0)
                .size_range(280.0..=520.0)
                .frame(frame)
                .show(ui, |ui| actions = self.sidebar(ui, cx));
            actions
        }
    }

    /// The expanded sidebar's contents.
    fn sidebar(&mut self, ui: &mut Ui, cx: &ToolContext) -> Vec<Action> {
        let theme = cx.theme;
        self.tabs_row(ui, cx);
        ui.add_space(8.0);
        self.workspace_row(ui, cx);
        ui.add_space(4.0);
        let mut actions = Vec::new();
        if let Some(tool) = shelf::shelf(ui, theme, self.workspaces.current()) {
            let ws = self.workspaces.current_mut();
            ws.toggle(tool);
            // A hooked tool turned on hooks itself under a layer if none is.
            if ws.state(tool).is_some_and(|c| c.on)
                && let Some(implementation) = tool.tool()
            {
                actions.extend(implementation.opened(cx));
            }
        }
        ui.add_space(10.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| actions.extend(self.card_stack(ui, cx)));
        actions
    }

    /// Controller tabs (only A until Milestone 8), pop-out and collapse.
    fn tabs_row(&mut self, ui: &mut Ui, cx: &ToolContext) {
        let theme = cx.theme;
        ui.horizontal(|ui| {
            let (rect, _) = ui.allocate_exact_size(vec2(30.0, 26.0), egui::Sense::hover());
            ui.painter().rect_filled(rect, 6.0, theme.accent);
            ui.painter().text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                "A",
                egui::FontId::proportional(14.0),
                egui::Color32::BLACK,
            );
            ui.add_enabled(false, Button::new(RichText::new(icon::PLUS)))
                .on_disabled_hover_text("More controllers (B, C, …) arrive in Milestone 8");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(Button::new(RichText::new(icon::CARET_DOUBLE_LEFT)))
                    .on_hover_text("Collapse to the icon rail")
                    .clicked()
                {
                    self.rail = true;
                    self.popover = None;
                }
                ui.add_enabled(false, Button::new(RichText::new(icon::ARROW_UP_RIGHT)))
                    .on_disabled_hover_text(
                        "Pop the controller out into its own window (Milestone 10)",
                    );
            });
        });
    }

    /// "TOOLS", the workspace menu and its gear.
    fn workspace_row(&mut self, ui: &mut Ui, cx: &ToolContext) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("TOOLS").color(cx.theme.text_faint).small());
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                ui.menu_button(icon::GEAR, |ui| {
                    ui.label(RichText::new("Save the current arrangement as").small());
                    ui.text_edit_singleline(&mut self.save_name);
                    if ui
                        .add_enabled(
                            !self.save_name.trim().is_empty(),
                            Button::new("Save workspace"),
                        )
                        .clicked()
                    {
                        self.workspaces.save_as(&self.save_name);
                        self.save_name.clear();
                        ui.close();
                    }
                    ui.separator();
                    let can_delete = self.workspaces.list.len() > 1;
                    if ui
                        .add_enabled(can_delete, Button::new("Delete this workspace"))
                        .clicked()
                    {
                        self.workspaces.delete_current();
                        ui.close();
                    }
                });
                let mut chosen = self.workspaces.current;
                ComboBox::from_id_salt("workspace")
                    .width(ui.available_width().min(160.0))
                    .selected_text(&self.workspaces.current().name)
                    .show_ui(ui, |ui| {
                        for (i, w) in self.workspaces.list.iter().enumerate() {
                            ui.selectable_value(&mut chosen, i, &w.name);
                        }
                    });
                self.workspaces.switch(chosen);
            });
        });
    }

    /// The cards of the current workspace, with drag-to-reorder. A tool with
    /// several cards (Define Overlay, one per layer) shows them together at
    /// the tool's place; a hooked tool (Clusterize) shows its cards under its
    /// parent's card of the same layer.
    fn card_stack(&mut self, ui: &mut Ui, cx: &ToolContext) -> Vec<Action> {
        let mut actions = Vec::new();
        let mut rects: Vec<(ToolId, Rect)> = Vec::new();
        let mut dropped = false;
        let mut fold_groups: Option<bool> = None;
        let mut group_keys: Vec<(ToolId, u64)> = Vec::new();
        let visible = self.workspaces.current().visible();
        let under_parent = |t: ToolId| graph::parent(t).is_some_and(|p| visible.contains(&p));
        for &tool in &visible {
            let Some(implementation) = tool.tool() else {
                continue;
            };
            let Some(entry) = self.workspaces.current().state(tool) else {
                continue;
            };
            let mut group: Option<Rect> = None;
            for (n, instance) in implementation.instances(cx).iter().enumerate() {
                // Cards of a hooked tool are drawn under their parent's.
                if instance.id != 0 && under_parent(tool) {
                    continue;
                }
                let children: Vec<(ToolId, Instance)> = graph::children(tool)
                    .into_iter()
                    .filter(|c| visible.contains(c))
                    .filter_map(|c| {
                        let found = c
                            .tool()?
                            .instances(cx)
                            .into_iter()
                            .find(|i| i.id == instance.id && i.id != 0)?;
                        Some((c, found))
                    })
                    .collect();
                let key = (tool, instance.id);
                if !children.is_empty() {
                    group_keys.push(key);
                }
                let group_folded = !children.is_empty() && self.group_folded.contains(&key);
                let summary = if group_folded {
                    let mut text = implementation.summary(cx, instance);
                    for (c, ci) in &children {
                        if let Some(t) = c.tool() {
                            text.push_str(" · ");
                            text.push_str(&t.summary(cx, ci));
                        }
                    }
                    text
                } else {
                    implementation.summary(cx, instance)
                };
                let (rect, collapsed, events) = self.show_instance(
                    ui,
                    cx,
                    tool,
                    instance,
                    &entry,
                    n == 0,
                    &summary,
                    group_folded,
                    None,
                    &mut actions,
                );
                let mut group_rect = rect;
                if events.toggle && group_folded {
                    self.group_folded.remove(&key); // unfold the group first
                } else {
                    self.apply_events(tool, instance, collapsed, &events, &mut actions, None);
                }
                if let Some(fold) = events.fold_all {
                    fold_groups = Some(fold);
                }
                if events.drag_started {
                    self.dragging = Some(tool);
                }
                dropped |= events.drag_stopped;
                if group_folded {
                    hooks::stacked_edge(ui, cx.theme, rect);
                    ui.add_space(6.0);
                }
                if !children.is_empty() && !group_folded {
                    for (child, child_instance) in &children {
                        let Some(child_tool) = child.tool() else {
                            continue;
                        };
                        ui.add_space(2.0);
                        let (socket, clicked) =
                            hooks::socket(ui, cx.theme, child_tool.link_label());
                        if clicked {
                            self.group_folded.insert(key);
                        }
                        let detach = child_tool.hook_action(instance.id, false);
                        let child_summary = child_tool.summary(cx, child_instance);
                        let child_entry = self.workspaces.current().state(*child).unwrap_or(entry);
                        let mut child_rect = None;
                        ui.horizontal_top(|ui| {
                            ui.add_space(hooks::INDENT);
                            ui.vertical(|ui| {
                                let (r, collapsed, ev) = self.show_instance(
                                    ui,
                                    cx,
                                    *child,
                                    child_instance,
                                    &child_entry,
                                    false,
                                    &child_summary,
                                    false,
                                    detach.as_ref(),
                                    &mut actions,
                                );
                                self.apply_events(
                                    *child,
                                    child_instance,
                                    collapsed,
                                    &ev,
                                    &mut actions,
                                    detach.clone(),
                                );
                                child_rect = Some(r);
                            });
                        });
                        if let Some(r) = child_rect {
                            hooks::spine(ui, cx.theme, socket, r);
                            group_rect = group_rect.union(r);
                        }
                    }
                }
                group = Some(group.map_or(group_rect, |g| g.union(group_rect)));
                ui.add_space(8.0);
            }
            if let Some(g) = group {
                rects.push((tool, g));
            }
        }
        if let Some(fold) = fold_groups {
            // Alt-click folds or unfolds every group too.
            for key in group_keys {
                if fold {
                    self.group_folded.insert(key);
                } else {
                    self.group_folded.remove(&key);
                }
            }
        }
        self.finish_drag(ui, cx, &rects, dropped);
        actions
    }

    /// Draw one card. Returns its rectangle, whether it is folded, and what
    /// happened in its header.
    #[allow(clippy::too_many_arguments)]
    fn show_instance(
        &mut self,
        ui: &mut Ui,
        cx: &ToolContext,
        tool: ToolId,
        instance: &Instance,
        entry: &workspace::CardEntry,
        first: bool,
        summary: &str,
        group_folded: bool,
        detach: Option<&Action>,
        actions: &mut Vec<Action>,
    ) -> (Rect, bool, card_frame::CardEvents) {
        let Some(implementation) = tool.tool() else {
            return (Rect::NOTHING, false, Default::default());
        };
        let single = instance.id == 0;
        let collapsed = if single {
            entry.collapsed
        } else {
            self.instance_collapsed
                .get(&(tool, instance.id))
                .copied()
                .unwrap_or(false)
        };
        let header = CardHeader {
            tool,
            collapsed: collapsed || group_folded,
            pinned: implementation.pinned(),
            summary,
            title: instance.title.as_deref(),
            closable: single || detach.is_some(),
            draggable: first && detach.is_none(),
        };
        let dragging = self.dragging == Some(tool);
        let (rect, events) = show_card(ui, cx.theme, &header, dragging, |ui| {
            // Widgets of different cards of one tool must not share ids.
            ui.push_id((tool, instance.id), |ui| {
                actions.extend(implementation.card_ui(ui, cx, instance));
            });
        });
        (rect, collapsed, events)
    }

    /// Carry out what the user did in a card's header (apart from dragging).
    /// `detach` is the action of the × button of a hooked card.
    fn apply_events(
        &mut self,
        tool: ToolId,
        instance: &Instance,
        collapsed: bool,
        events: &card_frame::CardEvents,
        actions: &mut Vec<Action>,
        detach: Option<Action>,
    ) {
        let single = instance.id == 0;
        if events.toggle {
            if single {
                self.workspaces.current_mut().toggle_collapsed(tool);
            } else {
                self.instance_collapsed
                    .insert((tool, instance.id), !collapsed);
            }
        }
        if let Some(fold) = events.fold_all {
            self.workspaces.current_mut().set_all_collapsed(fold);
            for v in self.instance_collapsed.values_mut() {
                *v = fold;
            }
            if !single {
                self.instance_collapsed.insert((tool, instance.id), fold);
            }
        }
        if events.close {
            match detach {
                Some(action) => actions.push(action),
                None => self.workspaces.current_mut().close(tool),
            }
        }
    }

    /// Show where a dragged card would land, and move it on release.
    fn finish_drag(
        &mut self,
        ui: &mut Ui,
        cx: &ToolContext,
        rects: &[(ToolId, Rect)],
        dropped: bool,
    ) {
        let Some(dragged) = self.dragging else {
            return;
        };
        let others: Vec<Rect> = rects
            .iter()
            .filter(|(t, _)| *t != dragged)
            .map(|(_, r)| *r)
            .collect();
        let pointer = ui.ctx().pointer_latest_pos();
        let index = pointer.map_or(0, |p| drop_index(&others, p.y));
        if let (Some(line_y), Some(first)) = (
            insertion_y(&others, index),
            others.first().or(rects.first().map(|(_, r)| r)),
        ) {
            let (left, right) = (first.left(), first.right());
            ui.painter().line_segment(
                [pos2(left, line_y), pos2(right, line_y)],
                Stroke::new(2.0, cx.theme.accent),
            );
        }
        if dropped || !ui.ctx().input(|i| i.pointer.any_down()) {
            // `index` counts the cards drawn in their own place; hooked cards
            // follow their parent, so translate to the workspace's order.
            let slots: Vec<ToolId> = rects
                .iter()
                .map(|(t, _)| *t)
                .filter(|t| *t != dragged)
                .collect();
            let shown: Vec<ToolId> = self
                .workspaces
                .current()
                .visible()
                .into_iter()
                .filter(|t| *t != dragged)
                .collect();
            let at = slots
                .get(index)
                .and_then(|t| shown.iter().position(|s| s == t))
                .unwrap_or(shown.len());
            self.workspaces.current_mut().move_card(dragged, at);
            self.dragging = None;
        }
    }
}

/// Which slot among the other cards a pointer at `y` is over: the number of
/// other cards whose center is above it.
pub fn drop_index(others: &[Rect], y: f32) -> usize {
    others.iter().filter(|r| r.center().y < y).count()
}

/// The y of the insertion line for `index` among `others`.
fn insertion_y(others: &[Rect], index: usize) -> Option<f32> {
    match others.get(index) {
        Some(r) => Some(r.top() - 4.0),
        None => others.last().map(|r| r.bottom() + 4.0),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rect(top: f32, bottom: f32) -> Rect {
        Rect::from_min_max(pos2(0.0, top), pos2(100.0, bottom))
    }

    #[test]
    fn drop_index_counts_cards_above_the_pointer() {
        let others = [rect(0.0, 40.0), rect(48.0, 88.0), rect(96.0, 136.0)];
        assert_eq!(drop_index(&others, -5.0), 0);
        assert_eq!(drop_index(&others, 30.0), 1);
        assert_eq!(drop_index(&others, 70.0), 2);
        assert_eq!(drop_index(&others, 500.0), 3);
        assert_eq!(drop_index(&[], 10.0), 0);
    }

    #[test]
    fn insertion_line_sits_between_cards() {
        let others = [rect(0.0, 40.0), rect(48.0, 88.0)];
        assert_eq!(insertion_y(&others, 0), Some(-4.0));
        assert_eq!(insertion_y(&others, 1), Some(44.0));
        assert_eq!(insertion_y(&others, 2), Some(92.0));
        assert_eq!(insertion_y(&[], 0), None);
    }

    #[test]
    fn controller_ui_normalizes_loaded_state() {
        let mut c = ControllerUi::default();
        c.workspaces.list[0].cards.truncate(1);
        let c = c.normalized();
        assert_eq!(c.workspaces.list[0].cards.len(), 10);
    }
}
