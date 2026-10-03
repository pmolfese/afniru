//! The collapsed controller: a narrow icon rail. Clicking an icon opens that
//! card as a pop-over next to the rail.

use egui::{
    Align2, Area, Button, FontId, Frame, Id, Key, Margin, Order, Rect, RichText, Sense, Stroke, Ui,
    pos2, vec2,
};
use egui_phosphor::regular as icon;

use super::card_frame::{CardHeader, show_card};
use super::workspace::Workspace;
use crate::session::{Action, Session};
use crate::tools::{ToolContext, ToolId};

/// Width of the rail panel.
pub const WIDTH: f32 = 64.0;

/// What happened in the rail this frame.
#[derive(Debug, Default)]
pub struct RailEvents {
    /// The user pressed the expand button.
    pub expand: bool,
    /// Actions requested by the pop-over card.
    pub actions: Vec<Action>,
}

/// The rail's contents, and its pop-over. `popover` is the open card and the
/// y position of the icon that opened it.
pub fn rail(
    ui: &mut Ui,
    cx: &ToolContext,
    ws: &mut Workspace,
    popover: &mut Option<(ToolId, f32)>,
) -> RailEvents {
    let theme = cx.theme;
    let mut events = RailEvents::default();
    let rail_rect = ui.max_rect();

    // The controller chip (A) and a placeholder for more controllers.
    ui.vertical_centered(|ui| {
        let (rect, _) = ui.allocate_exact_size(vec2(30.0, 26.0), Sense::hover());
        ui.painter().rect_filled(rect, 6.0, theme.accent);
        ui.painter().text(
            rect.center(),
            Align2::CENTER_CENTER,
            Session::controller_name(cx.session.active).to_string(),
            FontId::proportional(14.0),
            egui::Color32::BLACK,
        );
        ui.separator();

        for tool in ws.visible() {
            let open = popover.is_some_and(|(t, _)| t == tool);
            let ink = if open { theme.accent } else { theme.text_dim };
            let (rect, response) = ui.allocate_exact_size(vec2(WIDTH - 16.0, 44.0), Sense::click());
            if open {
                ui.painter().rect_filled(rect, 6.0, theme.card_hi);
                ui.painter().rect_stroke(
                    rect,
                    6.0,
                    Stroke::new(1.0, theme.accent),
                    egui::StrokeKind::Inside,
                );
            } else if response.hovered() {
                ui.painter().rect_filled(rect, 6.0, theme.card_hi);
            }
            ui.painter().text(
                pos2(rect.center().x, rect.top() + 16.0),
                Align2::CENTER_CENTER,
                tool.icon(),
                FontId::proportional(18.0),
                ink,
            );
            ui.painter().text(
                pos2(rect.center().x, rect.bottom() - 8.0),
                Align2::CENTER_CENTER,
                tool.label(),
                FontId::proportional(10.0),
                ink,
            );
            if response.on_hover_text(tool.title()).clicked() {
                *popover = if open { None } else { Some((tool, rect.top())) };
            }
        }
    });

    ui.with_layout(egui::Layout::bottom_up(egui::Align::Center), |ui| {
        if ui
            .add(Button::new(RichText::new(icon::CARET_DOUBLE_RIGHT)))
            .on_hover_text("Expand the controller")
            .clicked()
        {
            events.expand = true;
        }
    });

    if let Some((tool, y)) = *popover {
        events.actions = popover_card(
            ui,
            cx,
            ws,
            tool,
            pos2(rail_rect.right() + 6.0, y),
            rail_rect,
            popover,
        );
    }
    events
}

/// The pop-over card. Closes on Escape, on its × button, or on a click
/// outside it and the rail.
fn popover_card(
    ui: &mut Ui,
    cx: &ToolContext,
    ws: &mut Workspace,
    tool: ToolId,
    pos: egui::Pos2,
    rail_rect: Rect,
    popover: &mut Option<(ToolId, f32)>,
) -> Vec<Action> {
    let Some(implementation) = tool.tool() else {
        *popover = None;
        return Vec::new();
    };
    let ctx = ui.ctx().clone();
    let mut actions = Vec::new();
    let area = Area::new(Id::new("rail_popover"))
        .order(Order::Foreground)
        .fixed_pos(pos)
        .show(&ctx, |ui| {
            Frame::popup(ui.style())
                .inner_margin(Margin::same(0))
                .show(ui, |ui| {
                    ui.set_width(320.0);
                    for instance in implementation.instances(cx) {
                        let summary = implementation.summary(cx, &instance);
                        let single = instance.id == 0;
                        let header = CardHeader {
                            tool,
                            collapsed: false,
                            pinned: implementation.pinned(),
                            summary: &summary,
                            title: instance.title.as_deref(),
                            closable: single,
                            draggable: false,
                        };
                        let (_, events) = show_card(ui, cx.theme, &header, false, |ui| {
                            ui.push_id((tool, instance.id), |ui| {
                                actions.extend(implementation.card_ui(ui, cx, &instance));
                            });
                        });
                        if events.close {
                            ws.close(tool);
                            *popover = None;
                        }
                        ui.add_space(6.0);
                    }
                });
        });
    let outside_click = ctx.input(|i| {
        i.pointer.any_pressed()
            && i.pointer
                .interact_pos()
                .is_some_and(|p| !area.response.rect.contains(p) && !rail_rect.contains(p))
    });
    if outside_click || ctx.input(|i| i.key_pressed(Key::Escape)) {
        *popover = None;
    }
    actions
}
