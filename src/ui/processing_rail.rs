//! The Processing rail: a compact inspector on the right showing how the
//! viewed data moved through an `afni_proc.py` pipeline.
//!
//! * Steps are drawn top to bottom as a subway line. **Selection** (blue node
//!   and ring) and **health** (the pill at the far right) are independent.
//! * Health is never color alone: each pill has an icon, a tooltip and a
//!   screen-reader label.
//! * The selected step expands to one reason line and an artifact count with
//!   **View**; the full evidence and file list open in a detail window.
//! * The rail takes about 15% of the window. It collapses to a slim strip, and
//!   below [`NARROW`] points of window width it stays a strip and opens as a
//!   temporary drawer instead.
//! * Keyboard: Tab to the list, Up/Down to move, Enter to select.

use egui::{
    Align2, Area, Button, Color32, FontId, Frame, Id, Key, Margin, Order, Panel, Rect, RichText,
    Sense, Stroke, Ui, WidgetInfo, WidgetType, pos2, vec2,
};
use egui_phosphor::regular as icon;
use serde::{Deserialize, Serialize};

use super::theme::Theme;
use crate::processing::model::ProcessingModel;
use crate::processing::{Health, ProcessingStep, StepId};

/// Window width below which the rail is a strip plus drawer.
pub const NARROW: f32 = 1000.0;
/// Width of the collapsed strip.
const STRIP: f32 = 44.0;
/// Width of the drawer content.
const DRAWER: f32 = 270.0;
/// Height of one step row.
const ROW: f32 = 30.0;
/// x of the nodes inside the content.
const NODE_X: f32 = 10.0;

/// What the user asked for in the rail.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RailEvent {
    /// "View" on this step.
    View(StepId),
    /// Reload the run from disk.
    Refresh,
}

/// The rail's own (saved) state.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ProcessingRail {
    /// Collapsed to the strip?
    pub collapsed: bool,
    /// Temporary drawer open (narrow windows).
    #[serde(skip)]
    pub drawer_open: bool,
    /// Detail window open for the selected step.
    #[serde(skip)]
    pub detail_open: bool,
    /// The step whose status pop-up is showing (the pointer is over its
    /// traffic light or over the pop-up).
    #[serde(skip)]
    status_popup: Option<usize>,
    /// Where the pop-up was drawn last frame: it stays open while the pointer
    /// is over it (or crossing the gap to it from the traffic light).
    #[serde(skip)]
    popup_rect: Option<Rect>,
    /// Width taken on the right (panel, strip, drawer), so the detail window
    /// sits beside it instead of on top.
    #[serde(skip)]
    edge: f32,
}

/// The color of a health state.
pub fn health_color(theme: &Theme, h: Health) -> Color32 {
    match h {
        Health::Good => theme.good,
        Health::Caution => theme.caution,
        Health::Failed => theme.danger,
        Health::Unknown => theme.unknown,
    }
}

/// The icon of a health state (so color is never the only signal).
pub fn health_icon(h: Health) -> &'static str {
    match h {
        Health::Good => icon::CHECK,
        Health::Caution => icon::WARNING,
        Health::Failed => icon::X,
        Health::Unknown => icon::QUESTION,
    }
}

/// A filled circle with the health icon, with tooltip and accessible label.
fn pill(ui: &mut Ui, theme: &Theme, h: Health, tip: &str) {
    let (rect, response) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::hover());
    let color = health_color(theme, h);
    ui.painter().circle_filled(rect.center(), 9.0, color);
    ui.painter().text(
        rect.center(),
        Align2::CENTER_CENTER,
        health_icon(h),
        FontId::proportional(11.0),
        // Dark ink on the bright states, white on the dark ones.
        if matches!(h, Health::Caution | Health::Good) && theme.dark {
            Color32::BLACK
        } else {
            Color32::WHITE
        },
    );
    response.widget_info(|| {
        WidgetInfo::labeled(WidgetType::Label, true, format!("{}: {tip}", h.label()))
    });
    response.on_hover_text(format!("{}: {tip}", h.label()));
}

/// Is the pointer inside `rect`?
fn pointer_in(ctx: &egui::Context, rect: Rect) -> bool {
    ctx.pointer_hover_pos().is_some_and(|p| rect.contains(p))
}

/// The pop-up under a traffic light: the step, its state and why, and an
/// invitation to click. Returns its rectangle and whether it was clicked.
///
/// It opens to the *left* of the rail, level with the light, so it never covers
/// the steps below or above it.
fn status_popup(
    ctx: &egui::Context,
    theme: &Theme,
    step: &ProcessingStep,
    anchor: Rect,
    rail_left: f32,
) -> (Rect, bool) {
    let health = step.assessment.health;
    let mut clicked = false;
    let area = egui::Area::new(Id::new("processing_status_popup"))
        .order(egui::Order::Tooltip)
        .pivot(Align2::RIGHT_CENTER)
        .fixed_pos(pos2(rail_left - 8.0, anchor.center().y))
        .show(ctx, |ui| {
            let frame = egui::Frame::popup(ui.style()).show(ui, |ui| {
                ui.set_max_width(300.0);
                ui.horizontal(|ui| {
                    let (dot, _) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                    ui.painter()
                        .circle_filled(dot.center(), 6.0, health_color(theme, health));
                    ui.label(
                        RichText::new(format!("{} · {}", step.label, health.label())).strong(),
                    );
                });
                ui.add(egui::Label::new(step.assessment.reason()).wrap());
                // What is not fine, beyond the one-line reason.
                for e in step
                    .assessment
                    .evidence
                    .iter()
                    .filter(|e| e.health != Health::Good)
                    .take(3)
                {
                    ui.add(
                        egui::Label::new(
                            RichText::new(format!("• {}: {}", e.title, e.finding))
                                .small()
                                .color(health_color(theme, e.health)),
                        )
                        .wrap(),
                    );
                }
                ui.add_space(2.0);
                ui.label(
                    RichText::new(format!("{} Click for every check and file", icon::INFO))
                        .small()
                        .color(theme.accent),
                );
            });
            let r = ui.interact(
                frame.response.rect,
                Id::new("processing_status_popup_hit"),
                Sense::CLICK,
            );
            clicked = r.clicked();
            if r.hovered() {
                ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
            }
        });
    (area.response.rect, clicked)
}

impl ProcessingRail {
    /// Show the rail as a right-hand panel (and the drawer when narrow).
    pub fn panel(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        model: &mut ProcessingModel,
    ) -> Vec<RailEvent> {
        let mut events = Vec::new();
        let width = ui.ctx().content_rect().width();
        let narrow = width < NARROW;
        let frame = Frame::new()
            .fill(theme.panel)
            .inner_margin(Margin::same(10));

        if self.collapsed || narrow {
            Panel::right("processing_strip")
                .resizable(false)
                .exact_size(STRIP)
                .frame(frame.inner_margin(Margin::symmetric(6, 10)))
                .show(ui, |ui| self.strip(ui, theme, model, narrow));
            self.edge = STRIP
                + if narrow && self.drawer_open {
                    DRAWER + 8.0
                } else {
                    0.0
                };
            if narrow && self.drawer_open {
                self.drawer(ui.ctx(), theme, model, &mut events);
            }
        } else {
            self.drawer_open = false;
            let shown = Panel::right("processing")
                .resizable(true)
                .default_size((width * 0.15).clamp(190.0, 260.0))
                .size_range(170.0..=340.0)
                .frame(frame)
                .show(ui, |ui| self.content(ui, theme, model, &mut events));
            self.edge = shown.response.rect.width();
        }
        if self.detail_open {
            self.detail_window(ui.ctx(), theme, model);
        }
        events
    }

    /// The slim strip: expand button and one health dot per step.
    fn strip(&mut self, ui: &mut Ui, theme: &Theme, model: &ProcessingModel, narrow: bool) {
        ui.vertical_centered(|ui| {
            let tip = if narrow {
                "Show the Processing drawer"
            } else {
                "Expand the Processing rail"
            };
            if ui
                .add(Button::new(RichText::new(icon::FLOW_ARROW).size(18.0)))
                .on_hover_text(tip)
                .clicked()
            {
                if narrow {
                    self.drawer_open = !self.drawer_open;
                } else {
                    self.collapsed = false;
                }
            }
            ui.add_space(8.0);
            for step in &model.run.steps {
                let h = step.assessment.health;
                let (rect, response) = ui.allocate_exact_size(vec2(14.0, 14.0), Sense::hover());
                let selected = model.selected.as_ref() == Some(&step.id);
                if selected {
                    ui.painter()
                        .circle_stroke(rect.center(), 6.5, Stroke::new(1.5, theme.select));
                }
                ui.painter()
                    .circle_filled(rect.center(), 4.5, health_color(theme, h));
                let tip = format!("{}: {}", step.label, h.label());
                response.widget_info(|| WidgetInfo::labeled(WidgetType::Label, true, tip.clone()));
                response.on_hover_text(tip);
                ui.add_space(4.0);
            }
        });
    }

    /// The temporary drawer for narrow windows.
    fn drawer(
        &mut self,
        ctx: &egui::Context,
        theme: &Theme,
        model: &mut ProcessingModel,
        events: &mut Vec<RailEvent>,
    ) {
        let area = Area::new(Id::new("processing_drawer"))
            .order(Order::Foreground)
            .anchor(Align2::RIGHT_TOP, vec2(-STRIP - 8.0, 78.0))
            .show(ctx, |ui| {
                Frame::popup(ui.style())
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        ui.set_width(DRAWER);
                        ui.set_max_height((ctx.content_rect().height() - 110.0).max(200.0));
                        self.content(ui, theme, model, events);
                    });
            });
        let outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| {
                    !area.response.rect.contains(p)
                        && p.x < ctx.content_rect().right() - STRIP - 4.0
                })
        });
        if outside || ctx.input(|i| i.key_pressed(Key::Escape)) {
            self.drawer_open = false;
        }
    }

    /// Header, subtitle and the steps.
    fn content(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        model: &mut ProcessingModel,
        events: &mut Vec<RailEvent>,
    ) {
        ui.horizontal(|ui| {
            ui.label(RichText::new(icon::FLOW_ARROW).color(theme.select));
            ui.label(RichText::new("Processing").strong().color(theme.text));
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                if ui
                    .add(Button::new(RichText::new(icon::CARET_DOUBLE_RIGHT)).frame(false))
                    .on_hover_text("Collapse to a strip")
                    .clicked()
                {
                    self.collapsed = true;
                    self.drawer_open = false;
                }
                ui.menu_button(icon::INFO, |ui| legend(ui, theme));
                if ui
                    .add(Button::new(RichText::new(icon::ARROWS_CLOCKWISE)).frame(false))
                    .on_hover_text("Reload from disk")
                    .clicked()
                {
                    events.push(RailEvent::Refresh);
                }
            });
        });
        ui.label(
            RichText::new(format!("{} · {}", model.run.tool, model.run.name))
                .small()
                .color(theme.text_dim),
        )
        .on_hover_text(model.run.results_dir.display().to_string());
        if let Some(note) = model.run.notes.first() {
            ui.label(RichText::new(note).small().color(theme.caution));
        }
        ui.add_space(8.0);
        egui::ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                self.steps(ui, theme, model, events);
            });
    }

    /// The subway line.
    fn steps(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        model: &mut ProcessingModel,
        events: &mut Vec<RailEvent>,
    ) {
        let ids: Vec<StepId> = model.run.steps.iter().map(|s| s.id.clone()).collect();
        let row_ids: Vec<Id> = ids.iter().map(|i| Id::new(("proc_step", &i.0))).collect();
        let mut clicked = None;
        let mut focused = None;
        let mut show_details: Option<StepId> = None;
        let mut rail_left = ui.max_rect().left();
        let mut pill_rects: Vec<Rect> = Vec::new();
        let n = ids.len();
        for (i, step) in model.run.steps.iter().enumerate() {
            let selected = model.selected.as_ref() == Some(&step.id);
            let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), ROW), Sense::hover());
            rail_left = rail_left.min(rect.left());
            let response = ui.interact(rect, row_ids[i], Sense::click());
            let health = step.assessment.health;
            let line = Stroke::new(1.5, theme.border);
            let x = rect.left() + NODE_X;
            let (top, bottom) = (
                if i == 0 { rect.center().y } else { rect.top() },
                if i + 1 == n && !selected {
                    rect.center().y
                } else {
                    rect.bottom()
                },
            );
            ui.painter()
                .line_segment([pos2(x, top), pos2(x, bottom)], line);
            node(ui.painter(), theme, pos2(x, rect.center().y), selected);
            if response.hovered() {
                ui.painter().rect_filled(
                    Rect::from_min_max(
                        pos2(x + 12.0, rect.top() + 2.0),
                        pos2(rect.right(), rect.bottom() - 2.0),
                    ),
                    4.0,
                    theme.card_hi,
                );
            }
            if response.has_focus() {
                focused = Some(i);
                ui.painter().rect_stroke(
                    rect.shrink(1.0),
                    4.0,
                    Stroke::new(1.5, theme.accent),
                    egui::StrokeKind::Inside,
                );
            }
            ui.painter().text(
                pos2(x + 20.0, rect.center().y),
                Align2::LEFT_CENTER,
                truncate(&step.label, ui.painter(), rect.width() - 58.0),
                FontId::proportional(13.0),
                if selected { theme.text } else { theme.text_dim },
            );
            // The pill sits at the far right.
            let pill_rect = Rect::from_center_size(
                pos2(rect.right() - 12.0, rect.center().y),
                vec2(18.0, 18.0),
            );
            ui.painter()
                .circle_filled(pill_rect.center(), 9.0, health_color(theme, health));
            ui.painter().text(
                pill_rect.center(),
                Align2::CENTER_CENTER,
                health_icon(health),
                FontId::proportional(11.0),
                if matches!(health, Health::Caution | Health::Good) && theme.dark {
                    Color32::BLACK
                } else {
                    Color32::WHITE
                },
            );
            let label = format!("{}, {}", step.label, health.label());
            response.widget_info(|| {
                WidgetInfo::selected(WidgetType::Button, true, selected, label.clone())
            });
            // The traffic light: hover for the status, click for the details.
            let pill_hit = ui.interact(
                pill_rect.expand(3.0),
                Id::new(("proc_pill", &step.id.0)),
                // Not focusable: the arrow keys must stay with the step rows.
                Sense::CLICK,
            );
            if pill_hit.hovered() {
                self.status_popup = Some(i);
            }
            pill_rects.push(pill_rect);
            if pill_hit.clicked() {
                show_details = Some(step.id.clone());
            }
            if response.clicked() && !pill_hit.hovered() {
                clicked = Some(step.id.clone());
                // So the arrow keys work right after a click.
                ui.ctx().memory_mut(|m| m.request_focus(row_ids[i]));
            }
            if selected {
                self.expanded(ui, theme, step, i + 1 == n, events);
            }
        }
        // Keyboard: Up/Down move the selection (and focus) while the list has it.
        if let Some(f) = focused {
            let (up, down) = ui
                .ctx()
                .input(|i| (i.key_pressed(Key::ArrowUp), i.key_pressed(Key::ArrowDown)));
            let target = if up && f > 0 {
                Some(f - 1)
            } else if down && f + 1 < n {
                Some(f + 1)
            } else {
                None
            };
            if let Some(t) = target {
                ui.ctx().memory_mut(|m| m.request_focus(row_ids[t]));
                clicked = Some(ids[t].clone());
            }
        }
        // The status pop-up of the traffic light under the pointer.
        if let Some(i) = self.status_popup {
            let pill = pill_rects.get(i).map(|r| r.expand(5.0));
            // Over the light, or over the pop-up as it was last frame (and the
            // gap between them).
            let keep = pill.is_some_and(|p| {
                pointer_in(ui.ctx(), p)
                    || self.popup_rect.is_some_and(|r| {
                        // The pop-up, and the strip between it and the light.
                        let corridor = Rect::from_min_max(
                            pos2(r.right(), p.top()),
                            pos2(p.left(), p.bottom()),
                        );
                        pointer_in(ui.ctx(), r.expand(4.0)) || pointer_in(ui.ctx(), corridor)
                    })
            });
            match model.run.steps.get(i) {
                Some(step) if keep => {
                    let (rect, click) =
                        status_popup(ui.ctx(), theme, step, pill_rects[i], rail_left);
                    self.popup_rect = Some(rect);
                    if click {
                        show_details = Some(step.id.clone());
                    }
                }
                _ => {
                    self.status_popup = None;
                    self.popup_rect = None;
                }
            }
        }
        if let Some(id) = show_details {
            // A click on a traffic light or its pop-up: select the step and
            // open its checks and files.
            model.select(Some(id));
            self.detail_open = true;
            self.status_popup = None;
            self.popup_rect = None;
        } else if let Some(id) = clicked {
            if model.selected.as_ref() == Some(&id) {
                // Clicking the selected step again leaves it selected but
                // closes the expansion's detail window.
                self.detail_open = false;
            }
            model.select(Some(id));
        }
    }

    /// Under the selected row: reason, artifact count, View, details.
    fn expanded(
        &mut self,
        ui: &mut Ui,
        theme: &Theme,
        step: &ProcessingStep,
        last: bool,
        events: &mut Vec<RailEvent>,
    ) {
        let health = step.assessment.health;
        let top = ui.cursor().top();
        ui.horizontal(|ui| {
            ui.add_space(NODE_X + 20.0);
            ui.vertical(|ui| {
                let reason = step.assessment.reason();
                let color = if health == Health::Good {
                    theme.text_dim
                } else {
                    health_color(theme, health)
                };
                ui.add(egui::Label::new(RichText::new(reason).small().color(color)).wrap());
                ui.horizontal(|ui| {
                    let n = step.viewable().len();
                    ui.label(
                        RichText::new(format!("{n} dataset{} ·", if n == 1 { "" } else { "s" }))
                            .small()
                            .color(theme.text_dim),
                    );
                    if ui
                        .add_enabled(
                            n > 0,
                            Button::new(RichText::new(format!("{} View", icon::EYE)).small())
                                .small(),
                        )
                        .clicked()
                    {
                        events.push(RailEvent::View(step.id.clone()));
                    }
                    if ui
                        .add(Button::new(RichText::new(icon::INFO)).frame(false))
                        .on_hover_text("Why? Show every check and file")
                        .clicked()
                    {
                        self.detail_open = !self.detail_open;
                    }
                });
            });
        });
        ui.add_space(6.0);
        if !last {
            let bottom = ui.cursor().top();
            ui.painter().line_segment(
                [
                    pos2(ui.min_rect().left() + NODE_X, top),
                    pos2(ui.min_rect().left() + NODE_X, bottom),
                ],
                Stroke::new(1.5, theme.border),
            );
        }
    }

    /// Every check and every file of the selected step.
    fn detail_window(&mut self, ctx: &egui::Context, theme: &Theme, model: &ProcessingModel) {
        let Some(step) = model.selected.as_ref().and_then(|id| model.run.step(id)) else {
            self.detail_open = false;
            return;
        };
        let mut open = self.detail_open;
        egui::Window::new(format!(
            "{}: {}",
            step.label,
            step.assessment.health.label()
        ))
        .id(Id::new("processing_detail"))
        .open(&mut open)
        .default_width(380.0)
        .anchor(Align2::RIGHT_TOP, vec2(-(self.edge + 12.0), 78.0))
        .show(ctx, |ui| {
            egui::ScrollArea::vertical()
                .max_height(560.0)
                .show(ui, |ui| {
                    ui.label(RichText::new("Checks").strong());
                    for e in &step.assessment.evidence {
                        ui.horizontal(|ui| {
                            pill(ui, theme, e.health, &e.finding);
                            ui.label(RichText::new(&e.title).strong());
                        });
                        ui.indent(("ev", e.check), |ui| {
                            ui.label(&e.finding);
                            ui.label(
                                RichText::new(format!("rule: {}", e.rule))
                                    .small()
                                    .color(theme.text_dim),
                            );
                            ui.label(
                                RichText::new(format!("source: {}", e.source.detail))
                                    .small()
                                    .color(theme.text_dim),
                            );
                        });
                        ui.add_space(4.0);
                    }
                    ui.separator();
                    ui.label(
                        RichText::new(format!("Artifacts ({})", step.artifacts.len())).strong(),
                    );
                    for a in step.artifacts.iter().take(60) {
                        let state = match (a.exists, a.openable) {
                            (false, _) => "missing".to_string(),
                            (true, Some(false)) => {
                                format!("cannot open: {}", a.open_error.as_deref().unwrap_or("?"))
                            }
                            _ => "ok".to_string(),
                        };
                        let color = if state == "ok" {
                            theme.text_dim
                        } else {
                            theme.error
                        };
                        ui.horizontal_wrapped(|ui| {
                            ui.label(RichText::new(&a.name).monospace());
                            ui.label(
                                RichText::new(format!(
                                    "{} · {} · {state}",
                                    a.role.label(),
                                    a.space.label()
                                ))
                                .small()
                                .color(color),
                            );
                        });
                    }
                    if step.artifacts.len() > 60 {
                        ui.label(
                            RichText::new(format!("… and {} more", step.artifacts.len() - 60))
                                .small(),
                        );
                    }
                });
        });
        self.detail_open = open;
    }
}

/// A step node: gray ring, or blue fill with a focus ring when selected.
fn node(painter: &egui::Painter, theme: &Theme, at: egui::Pos2, selected: bool) {
    if selected {
        painter.circle_stroke(at, 9.0, Stroke::new(1.5, theme.select));
        painter.circle_filled(at, 5.5, theme.select);
    } else {
        painter.circle_filled(at, 5.5, theme.panel);
        painter.circle_stroke(at, 5.5, Stroke::new(1.5, theme.text_faint));
    }
}

/// Cut `text` with an ellipsis so it fits in `width` points.
fn truncate(text: &str, painter: &egui::Painter, width: f32) -> String {
    let font = FontId::proportional(13.0);
    let fits = |s: &str| {
        painter
            .layout_no_wrap(s.to_string(), font.clone(), Color32::WHITE)
            .size()
            .x
            <= width
    };
    if fits(text) {
        return text.to_string();
    }
    let mut chars: Vec<char> = text.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let candidate: String = chars.iter().collect::<String>() + "…";
        if fits(&candidate) {
            return candidate;
        }
    }
    "…".to_string()
}

/// What the colors and shapes mean.
fn legend(ui: &mut Ui, theme: &Theme) {
    ui.set_min_width(240.0);
    ui.label(RichText::new("Step marker").strong());
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(22.0, 16.0), Sense::hover());
        node(ui.painter(), theme, r.center(), true);
        ui.label("selected step (blue)");
    });
    ui.horizontal(|ui| {
        let (r, _) = ui.allocate_exact_size(vec2(22.0, 16.0), Sense::hover());
        node(ui.painter(), theme, r.center(), false);
        ui.label("other step (gray)");
    });
    ui.add_space(4.0);
    ui.label(RichText::new("Health (separate from selection)").strong());
    for (h, text) in [
        (Health::Good, "every check passed"),
        (Health::Caution, "a check raised a caution"),
        (Health::Failed, "a check failed"),
        (Health::Unknown, "not measured, or not run yet"),
    ] {
        ui.horizontal(|ui| {
            pill(ui, theme, h, text);
            ui.label(format!("{}: {text}", h.label()));
        });
    }
    ui.add_space(4.0);
    ui.label(RichText::new("Keys: Tab to the list, ↑ ↓ move, Enter selects.").small());
}

#[cfg(test)]
mod tests {
    use std::fs;

    use egui_kittest::Harness;
    use egui_kittest::kittest::Queryable;

    use super::*;
    use crate::processing::discover::testkit::{GOOD_REVIEW, build, script_text};
    use crate::testutil::TempDir;

    /// A run with all four health states: Inputs unknown (no warning files),
    /// Alignment caution (Dice 0.7), Regression failed (no DOF left), the
    /// rest good.
    fn mixed() -> (TempDir, ProcessingModel) {
        let (tmp, results) = build("complete", &[], None);
        let review = GOOD_REVIEW
            .replace(
                "anat/EPI mask Dice coef   : 0.94",
                "anat/EPI mask Dice coef   : 0.70",
            )
            .replace(
                "degrees of freedom left   : 270",
                "degrees of freedom left   : 0",
            );
        fs::write(results.join("out.ss_review.sub-01.txt"), review).unwrap();
        for w in ["out.pre_ss_warn.txt", "out.4095_warn.txt"] {
            fs::remove_file(results.join(w)).unwrap();
        }
        let model = ProcessingModel::open(tmp.path()).unwrap();
        (tmp, model)
    }

    struct State {
        rail: ProcessingRail,
        model: ProcessingModel,
        theme: Theme,
        events: Vec<RailEvent>,
    }

    fn harness(size: egui::Vec2, state: State) -> Harness<'static, State> {
        Harness::builder().with_size(size).build_ui_state(
            |ui, st: &mut State| {
                crate::ui::fonts::ensure(ui.ctx());
                st.theme.apply(ui.ctx());
                let events = st.rail.panel(ui, &st.theme, &mut st.model);
                st.events.extend(events);
                egui::CentralPanel::default().show(ui, |ui| {
                    ui.label("views");
                });
            },
            state,
        )
    }

    fn state(dark: bool, model: ProcessingModel) -> State {
        State {
            rail: ProcessingRail::default(),
            model,
            theme: if dark { Theme::dark() } else { Theme::light() },
            events: Vec::new(),
        }
    }

    fn select(s: &mut State, block: &str) {
        s.model.select(Some(StepId(block.into())));
    }

    #[test]
    fn mixed_run_has_all_four_states() {
        let (_tmp, model) = mixed();
        let states: Vec<_> = model
            .run
            .steps
            .iter()
            .map(|s| (s.block.as_str(), s.assessment.health))
            .collect();
        let has = |h| states.iter().any(|(_, x)| *x == h);
        assert!(
            has(Health::Good)
                && has(Health::Caution)
                && has(Health::Failed)
                && has(Health::Unknown),
            "{states:?}"
        );
    }

    #[test]
    fn snapshot_compact_rail_with_a_selected_step() {
        let (_tmp, model) = mixed();
        let mut s = state(true, model);
        select(&mut s, "align");
        let mut h = harness(vec2(1300.0, 640.0), s);
        h.run();
        h.snapshot("processing_rail_selected");
    }

    #[test]
    fn snapshot_compact_rail_light() {
        let (_tmp, model) = mixed();
        let mut s = state(false, model);
        select(&mut s, "regress");
        let mut h = harness(vec2(1300.0, 640.0), s);
        h.run();
        h.snapshot("processing_rail_light");
    }

    #[test]
    fn snapshot_collapsed_strip() {
        let (_tmp, model) = mixed();
        let mut s = state(true, model);
        s.rail.collapsed = true;
        select(&mut s, "align");
        let mut h = harness(vec2(1300.0, 640.0), s);
        h.run();
        h.snapshot("processing_rail_collapsed");
    }

    #[test]
    fn snapshot_narrow_window_drawer() {
        let (_tmp, model) = mixed();
        let mut s = state(true, model);
        s.rail.drawer_open = true;
        select(&mut s, "align");
        let mut h = harness(vec2(820.0, 640.0), s);
        h.run();
        h.snapshot("processing_rail_drawer");
    }

    #[test]
    fn snapshot_detail_window() {
        let (_tmp, model) = mixed();
        let mut s = state(true, model);
        s.rail.detail_open = true;
        select(&mut s, "regress");
        let mut h = harness(vec2(1400.0, 700.0), s);
        h.run();
        h.snapshot("processing_rail_detail");
    }

    #[test]
    fn snapshot_long_and_custom_labels() {
        let tmp = TempDir::new("longlabels");
        let text = "#!/bin/tcsh -xef\n\necho \"auto-generated by afni_proc.py, today\"\nset subj = s1\n\
            # ===================== a_remarkably_long_custom_block_name_for_the_test =====================\n\
            3dcalc -prefix special.$subj x\n\
            # ================================ ricor =================================\n\
            3dcalc -prefix ricor.$subj x\n";
        fs::write(tmp.path().join("proc.s1"), text).unwrap();
        let model = ProcessingModel::open(tmp.path()).unwrap();
        let mut s = state(true, model);
        select(&mut s, "a_remarkably_long_custom_block_name_for_the_test");
        let mut h = harness(vec2(1300.0, 400.0), s);
        h.run();
        h.snapshot("processing_rail_long_labels");
    }

    #[test]
    fn clicking_a_step_selects_it_and_view_asks_for_it() {
        let (_tmp, model) = mixed();
        let mut h = harness(vec2(1300.0, 640.0), state(true, model));
        h.run();
        h.get_by_label("Alignment, Caution").click();
        h.run();
        assert_eq!(h.state().model.selected, Some(StepId("align".into())));
        // Alignment has no dataset outputs in the script, so View is disabled;
        // Smoothing has some.
        h.get_by_label("Smoothing, Good").click();
        h.run();
        h.get_by_label_contains("View").click();
        h.run();
        assert_eq!(h.state().events, [RailEvent::View(StepId("blur".into()))]);
    }

    #[test]
    fn arrow_keys_move_the_selection_from_a_focused_step() {
        let (_tmp, model) = mixed();
        let mut h = harness(vec2(1300.0, 640.0), state(true, model));
        h.run();
        h.get_by_label("Slice timing, Good").click();
        h.run();
        h.key_press(Key::ArrowDown);
        h.run();
        assert_eq!(h.state().model.selected, Some(StepId("align".into())));
        h.key_press(Key::ArrowUp);
        h.run();
        h.key_press(Key::ArrowUp);
        h.run();
        assert_eq!(h.state().model.selected, Some(StepId("outcount".into())));
    }

    #[test]
    fn every_step_exposes_a_health_label_for_screen_readers() {
        let (_tmp, model) = mixed();
        let mut h = harness(vec2(1300.0, 640.0), state(true, model));
        h.run();
        for (label, _) in [
            ("Inputs, Unknown", ()),
            ("Alignment, Caution", ()),
            ("Regression, Failed", ()),
            ("Smoothing, Good", ()),
        ] {
            h.get_by_label(label);
        }
    }

    #[test]
    fn collapse_button_makes_a_strip_and_the_strip_button_expands_it() {
        let (_tmp, model) = mixed();
        let mut h = harness(vec2(1300.0, 640.0), state(true, model));
        h.run();
        h.get_by_label(icon::CARET_DOUBLE_RIGHT).click();
        h.run();
        assert!(h.state().rail.collapsed);
        h.get_by_label(icon::FLOW_ARROW).click();
        h.run();
        assert!(!h.state().rail.collapsed);
    }

    #[test]
    fn refresh_button_asks_for_a_reload() {
        let (_tmp, model) = mixed();
        let mut h = harness(vec2(1300.0, 640.0), state(true, model));
        h.run();
        h.get_by_label(icon::ARROWS_CLOCKWISE).click();
        h.run();
        assert_eq!(h.state().events, [RailEvent::Refresh]);
    }

    #[test]
    fn script_text_helper_is_reachable() {
        assert!(script_text("complete").contains("afni_proc.py"));
    }
}
