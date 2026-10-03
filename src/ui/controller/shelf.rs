//! The tool shelf: a grid of tiles that show and hide cards.
//!
//! Tile states: **open** (gold icon and outline, filled pip), **folded**
//! (ring pip), **off** (dim; the card is hidden but keeps its settings) and
//! **not built yet** (very dim, with a tooltip saying when it comes).

use egui::{Align2, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::workspace::Workspace;
use crate::tools::ToolId;
use crate::ui::theme::Theme;

const COLUMNS: usize = 5;
const GAP: f32 = 6.0;
const HEIGHT: f32 = 54.0;

/// How a tile looks.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TileState {
    /// Card shown and unfolded.
    Open,
    /// Card shown but folded.
    Folded,
    /// Card hidden.
    Off,
    /// The tool does not exist yet.
    Unbuilt,
}

/// The state of `tool`'s tile in `ws`.
pub fn tile_state(ws: &Workspace, tool: ToolId) -> TileState {
    if tool.tool().is_none() {
        return TileState::Unbuilt;
    }
    match ws.state(tool) {
        Some(c) if c.on && c.collapsed => TileState::Folded,
        Some(c) if c.on => TileState::Open,
        _ => TileState::Off,
    }
}

/// What the user did on the shelf this frame.
#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ShelfEvents {
    /// A tile was clicked.
    pub clicked: Option<ToolId>,
    /// The user began dragging a tile of a hookable tool (one that attaches
    /// under another, like Clusterize).
    pub drag_started: Option<ToolId>,
    /// The user released a dragged tile.
    pub drag_stopped: Option<ToolId>,
}

/// Draw the shelf. A tile of a hookable tool can be dragged onto a card to
/// hook it there.
pub fn shelf(ui: &mut Ui, theme: &Theme, ws: &Workspace) -> ShelfEvents {
    let width = (ui.available_width() - GAP * (COLUMNS - 1) as f32) / COLUMNS as f32;
    let mut events = ShelfEvents::default();
    let origin = ui.cursor().min;
    let rows = ToolId::SHELF.len().div_ceil(COLUMNS);
    let (_, _) = ui.allocate_exact_size(
        vec2(
            ui.available_width(),
            rows as f32 * HEIGHT + (rows - 1) as f32 * GAP,
        ),
        Sense::hover(),
    );
    for (n, tool) in ToolId::SHELF.into_iter().enumerate() {
        let (col, row) = (n % COLUMNS, n / COLUMNS);
        let rect = Rect::from_min_size(
            pos2(
                origin.x + col as f32 * (width + GAP),
                origin.y + row as f32 * (HEIGHT + GAP),
            ),
            vec2(width, HEIGHT),
        );
        let state = tile_state(ws, tool);
        let hookable = tool.tool().is_some_and(|t| t.attaches_to().is_some());
        let sense = if state == TileState::Unbuilt {
            Sense::hover()
        } else if hookable {
            Sense::click_and_drag()
        } else {
            Sense::click()
        };
        let response = ui.interact(rect, ui.id().with(("tile", tool)), sense);
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, true, tool.title())
        });
        draw_tile(ui, theme, rect, tool, state, response.hovered());
        let response = if state == TileState::Unbuilt {
            response.on_hover_text(format!(
                "{}: planned for {}",
                tool.title(),
                tool.planned_in()
            ))
        } else if hookable {
            response.on_hover_text(format!(
                "{}: click to hook under the top overlay, or drag onto a card",
                tool.title()
            ))
        } else {
            response.on_hover_text(tool.title())
        };
        if response.clicked() {
            events.clicked = Some(tool);
        }
        if response.drag_started() {
            events.drag_started = Some(tool);
        }
        if response.drag_stopped() {
            events.drag_stopped = Some(tool);
        }
    }
    events
}

fn draw_tile(ui: &Ui, theme: &Theme, rect: Rect, tool: ToolId, state: TileState, hovered: bool) {
    let painter = ui.painter();
    let (fill, outline, ink) = match state {
        TileState::Open => (theme.card_hi, Stroke::new(1.0, theme.accent), theme.accent),
        TileState::Folded => (theme.card_hi, Stroke::new(1.0, theme.border), theme.text),
        TileState::Off => (
            if hovered { theme.card_hi } else { theme.card },
            Stroke::new(1.0, theme.border),
            theme.text_dim,
        ),
        TileState::Unbuilt => (theme.card, Stroke::new(1.0, theme.border), theme.text_faint),
    };
    painter.rect(rect, 6.0, fill, outline, egui::StrokeKind::Inside);
    painter.text(
        pos2(rect.center().x, rect.top() + 20.0),
        Align2::CENTER_CENTER,
        tool.icon(),
        FontId::proportional(20.0),
        ink,
    );
    painter.text(
        pos2(rect.center().x, rect.bottom() - 10.0),
        Align2::CENTER_CENTER,
        tool.label(),
        FontId::proportional(11.0),
        ink,
    );
    let pip = pos2(rect.right() - 8.0, rect.top() + 8.0);
    match state {
        TileState::Open => {
            painter.circle_filled(pip, 3.0, theme.accent);
        }
        TileState::Folded => {
            painter.circle_stroke(pip, 3.0, Stroke::new(1.0, theme.text_dim));
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tile_states_follow_the_workspace() {
        let mut ws = Workspace::standard("x");
        assert_eq!(tile_state(&ws, ToolId::Crosshair), TileState::Open);
        ws.toggle_collapsed(ToolId::Crosshair);
        assert_eq!(tile_state(&ws, ToolId::Crosshair), TileState::Folded);
        ws.toggle(ToolId::Crosshair);
        assert_eq!(tile_state(&ws, ToolId::Crosshair), TileState::Off);
        assert_eq!(tile_state(&ws, ToolId::InstaCorr), TileState::Unbuilt);
    }
}
