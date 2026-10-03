//! Hooked cards: the spine and socket that link a card to its parent, and the
//! stacked edge of a folded group.
//!
//! A hooked card (Clusterize under a Define Overlay card) is indented under
//! its parent. A **spine** runs from the parent down to the child, with a ⛓
//! **socket** on it whose label says what passes along the link ("clusters the
//! threshold"). Clicking the socket folds the whole group into one line.

use egui::{Align2, FontId, Pos2, Rect, RichText, Sense, Stroke, Ui, pos2, vec2};
use egui_phosphor::regular as icon;

use crate::ui::theme::Theme;

/// How far a hooked card is indented.
pub const INDENT: f32 = 22.0;

/// Radius of the socket.
const SOCKET: f32 = 9.0;

/// Draw the socket row (the ⛓ and its label). Returns the socket's center and
/// whether it was clicked.
pub fn socket(ui: &mut Ui, theme: &Theme, label: &str) -> (Pos2, bool) {
    let mut center = Pos2::ZERO;
    let mut clicked = false;
    ui.horizontal(|ui| {
        ui.add_space(INDENT - SOCKET - 2.0);
        let (rect, response) =
            ui.allocate_exact_size(vec2(SOCKET * 2.0, SOCKET * 2.0), Sense::click());
        center = rect.center();
        let ring = if response.hovered() {
            theme.text
        } else {
            theme.accent
        };
        ui.painter()
            .circle(center, SOCKET, theme.card_hi, Stroke::new(1.0, ring));
        ui.painter().text(
            center,
            Align2::CENTER_CENTER,
            icon::LINK,
            FontId::proportional(11.0),
            ring,
        );
        clicked = response
            .on_hover_text("Fold this card and the cards hooked under it into one line")
            .clicked();
        ui.label(RichText::new(label).small().color(theme.accent));
    });
    (center, clicked)
}

/// The line from a socket down and across to the top-left of a hooked card.
pub fn spine(ui: &Ui, theme: &Theme, socket: Pos2, child: Rect) {
    let stroke = Stroke::new(1.5, theme.accent);
    let y = child.top() + 18.0;
    let from = pos2(socket.x, socket.y + SOCKET);
    ui.painter().line_segment([from, pos2(socket.x, y)], stroke);
    ui.painter()
        .line_segment([pos2(socket.x, y), pos2(child.left(), y)], stroke);
    ui.painter()
        .circle_filled(pos2(socket.x, y), 2.5, theme.accent);
}

/// The strip under a folded group's card: the hooked cards, stacked behind it.
pub fn stacked_edge(ui: &Ui, theme: &Theme, card: Rect) {
    let edge = Rect::from_min_max(
        pos2(card.left() + 6.0, card.bottom() - 1.0),
        pos2(card.right() - 6.0, card.bottom() + 5.0),
    );
    ui.painter().rect_filled(edge, 3.0, theme.card_hi);
    ui.painter().rect_stroke(
        edge,
        3.0,
        Stroke::new(1.0, theme.border),
        egui::StrokeKind::Inside,
    );
}
