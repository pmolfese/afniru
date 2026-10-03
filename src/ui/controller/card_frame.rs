//! Shared card chrome: drag handle, fold chevron, icon and title, summary
//! when folded, pop-out (placeholder) and close (or pin) buttons.

use egui::{Button, Frame, Label, Margin, Rect, RichText, Sense, Stroke, Ui};
use egui_phosphor::regular as icon;

use crate::tools::ToolId;
use crate::ui::theme::Theme;

/// What the header shows for one card.
pub struct CardHeader<'a> {
    /// The card's tool.
    pub tool: ToolId,
    /// Folded to one line?
    pub collapsed: bool,
    /// Pinned cards show a pin instead of a close button.
    pub pinned: bool,
    /// One line shown while folded.
    pub summary: &'a str,
    /// A title that replaces the tool's (for one of several cards of a tool).
    pub title: Option<&'a str>,
    /// Does the card have a × button? (Only a tool's single card does: the
    /// cards of a multi-card tool are removed where they are managed.)
    pub closable: bool,
    /// Does the card have a drag handle? (Only the first card of a tool.)
    pub draggable: bool,
}

/// What the user did in the header this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct CardEvents {
    /// Fold or unfold this card.
    pub toggle: bool,
    /// Alt-click: fold (`true`) or unfold (`false`) every card.
    pub fold_all: Option<bool>,
    /// The × button.
    pub close: bool,
    /// The user began dragging the handle.
    pub drag_started: bool,
    /// The user released the handle.
    pub drag_stopped: bool,
}

/// Draw one card; `body` runs only when it is not folded. Returns the card's
/// rectangle (for drop-target math) and what happened in the header.
pub fn show_card(
    ui: &mut Ui,
    theme: &Theme,
    header: &CardHeader,
    dragging: bool,
    body: impl FnOnce(&mut Ui),
) -> (Rect, CardEvents) {
    let mut events = CardEvents::default();
    let stroke = Stroke::new(1.0, if dragging { theme.accent } else { theme.border });
    let response = Frame::new()
        .fill(theme.card)
        .stroke(stroke)
        .corner_radius(6)
        .inner_margin(Margin::symmetric(8, 6))
        .show(ui, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                if header.draggable {
                    let handle = ui
                        .add(
                            Label::new(
                                RichText::new(icon::DOTS_SIX_VERTICAL).color(theme.text_faint),
                            )
                            .sense(Sense::drag()),
                        )
                        .on_hover_cursor(egui::CursorIcon::Grab);
                    events.drag_started = handle.drag_started();
                    events.drag_stopped = handle.drag_stopped();
                } else {
                    // Keep the title aligned with the first card's.
                    ui.add_space(18.0);
                }

                let caret = if header.collapsed {
                    icon::CARET_RIGHT
                } else {
                    icon::CARET_DOWN
                };
                let title = RichText::new(format!(
                    "{caret}  {} {}",
                    header.tool.icon(),
                    header.title.unwrap_or(header.tool.title())
                ))
                .color(theme.text)
                .strong();
                let click = ui.add(Button::new(title).frame(false));
                if click.clicked() {
                    if ui.input(|i| i.modifiers.alt) {
                        events.fold_all = Some(!header.collapsed);
                    } else {
                        events.toggle = true;
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if header.pinned {
                        ui.add(Label::new(
                            RichText::new(icon::PUSH_PIN).color(theme.text_faint),
                        ))
                        .on_hover_text("Always shown");
                    } else if header.closable
                        && ui
                            .add(
                                Button::new(RichText::new(icon::X).color(theme.text_dim))
                                    .frame(false),
                            )
                            .on_hover_text("Hide this card (its settings are kept)")
                            .clicked()
                    {
                        events.close = true;
                    }
                    ui.add_enabled(
                        false,
                        Button::new(RichText::new(icon::ARROW_UP_RIGHT)).frame(false),
                    )
                    .on_disabled_hover_text("Pop out into its own window (Milestone 10)");
                    if header.collapsed {
                        ui.add(
                            Label::new(RichText::new(header.summary).color(theme.text_dim).small())
                                .truncate(),
                        );
                    }
                });
            });
            if !header.collapsed {
                ui.add_space(4.0);
                body(ui);
            }
        });
    (response.response.rect, events)
}
