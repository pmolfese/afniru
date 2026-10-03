//! Small chips: the attach chips in a card's footer.
//!
//! A **solid** chip is attached (gold outline); a **dashed** chip is
//! available to attach.

use egui::{Align2, FontId, Response, Sense, Shape, Stroke, Ui, WidgetInfo, WidgetType, vec2};

use crate::ui::theme::Theme;

/// A chip with `label`. `attached` draws it solid and gold, otherwise dashed.
pub fn attach_chip(ui: &mut Ui, theme: &Theme, label: &str, attached: bool) -> Response {
    let font = FontId::proportional(12.0);
    let galley = ui
        .painter()
        .layout_no_wrap(label.to_string(), font.clone(), theme.text);
    let size = galley.size() + vec2(16.0, 8.0);
    let (rect, response) = ui.allocate_exact_size(size, Sense::click());
    response.widget_info(|| WidgetInfo::labeled(WidgetType::Button, true, label));
    let (fill, ink) = if attached {
        (theme.card_hi, theme.accent)
    } else if response.hovered() {
        (theme.card_hi, theme.text)
    } else {
        (theme.card, theme.text_dim)
    };
    let painter = ui.painter();
    if attached {
        painter.rect(
            rect,
            10.0,
            fill,
            Stroke::new(1.0, theme.accent),
            egui::StrokeKind::Inside,
        );
    } else {
        painter.rect_filled(rect, 10.0, fill);
        // Dashed outline: egui has no dashed rounded rectangle, so the corners
        // are square and the dashes run along the four sides.
        let stroke = Stroke::new(1.0, theme.text_faint);
        let r = rect.shrink(0.5);
        let corners = [
            r.left_top(),
            r.right_top(),
            r.right_bottom(),
            r.left_bottom(),
            r.left_top(),
        ];
        painter.extend(Shape::dashed_line(&corners, stroke, 3.0, 3.0));
    }
    painter.text(rect.center(), Align2::CENTER_CENTER, label, font, ink);
    response
}
