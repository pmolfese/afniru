//! The color bar (pbar): the overlay's color scale as a vertical bar with
//! value ticks and the threshold marked on it.

use afni_core::color::ContinuousColorMap;
use egui::{Align2, Color32, FontId, Rect, Sense, Stroke, Ui, pos2, vec2};

use super::readout::format_value;
use crate::ui::theme::Theme;

/// Height of the bar in points.
pub const HEIGHT: f32 = 150.0;
/// Width of the colored bar.
const BAR: f32 = 22.0;
/// Width of the tick labels to its left.
const LABELS: f32 = 34.0;
/// How many color steps are drawn.
const STEPS: usize = 48;

/// What to draw.
pub struct Pbar<'a> {
    /// The color scale; position 0 is the bottom of the bar.
    pub colormap: &'a ContinuousColorMap,
    /// ± (bar covers `[-top, top]`) or positive only (`[0, top]`).
    pub signed: bool,
    /// The top of the range.
    pub top: f64,
    /// Where to mark the threshold, when it is on the same scale as the bar.
    pub threshold: Option<f64>,
}

/// The values labeled on the bar, top to bottom.
pub fn ticks(signed: bool, top: f64) -> Vec<f64> {
    if signed {
        vec![top, top / 2.0, 0.0, -top / 2.0, -top]
    } else {
        vec![top, top / 2.0, 0.0]
    }
}

/// Fraction of the bar's height, from the top, at which `value` sits.
pub fn y_fraction(signed: bool, top: f64, value: f64) -> f64 {
    let span = if signed { 2.0 * top } else { top };
    ((top - value) / span).clamp(0.0, 1.0)
}

/// Draw the pbar; returns the rectangle of the colored bar.
pub fn pbar(ui: &mut Ui, theme: &Theme, p: &Pbar) -> Rect {
    let (rect, _) = ui.allocate_exact_size(vec2(LABELS + BAR + 4.0, HEIGHT), Sense::hover());
    let bar = Rect::from_min_size(pos2(rect.left() + LABELS, rect.top()), vec2(BAR, HEIGHT));
    let painter = ui.painter();
    let step = HEIGHT / STEPS as f32;
    for i in 0..STEPS {
        // Top step is the highest color.
        let position = 1.0 - (i as f64 + 0.5) / STEPS as f64;
        let c = p.colormap.sample(position).to_u8();
        painter.rect_filled(
            Rect::from_min_size(
                pos2(bar.left(), bar.top() + i as f32 * step),
                vec2(BAR, step + 0.5),
            ),
            0.0,
            Color32::from_rgb(c[0], c[1], c[2]),
        );
    }
    painter.rect_stroke(
        bar,
        0.0,
        Stroke::new(1.0, theme.border),
        egui::StrokeKind::Outside,
    );
    for v in ticks(p.signed, p.top) {
        let y = bar.top() + y_fraction(p.signed, p.top, v) as f32 * HEIGHT;
        painter.line_segment(
            [pos2(bar.left() - 3.0, y), pos2(bar.left(), y)],
            Stroke::new(1.0, theme.text_faint),
        );
        let sign = if p.signed && v > 0.0 { "+" } else { "" };
        painter.text(
            pos2(bar.left() - 5.0, y),
            Align2::RIGHT_CENTER,
            format!("{sign}{}", format_value_short(v)),
            FontId::monospace(10.0),
            theme.text_dim,
        );
    }
    if let Some(t) = p.threshold.filter(|t| *t > 0.0 && *t <= p.top) {
        let marks = if p.signed { vec![t, -t] } else { vec![t] };
        for v in marks {
            let y = bar.top() + y_fraction(p.signed, p.top, v) as f32 * HEIGHT;
            painter.line_segment(
                [pos2(bar.left() - 2.0, y), pos2(bar.right() + 3.0, y)],
                Stroke::new(1.5, theme.accent),
            );
        }
    }
    bar
}

/// A tick label: at most one decimal, no trailing zero.
fn format_value_short(v: f64) -> String {
    if v == 0.0 {
        return "0".to_string();
    }
    let s = format_value(v as f32);
    // `format_value` keeps four decimals; ticks need fewer.
    match s.parse::<f64>() {
        Ok(x) if x.abs() >= 10.0 => format!("{x:.0}"),
        Ok(x) => format!("{x:.1}").trim_end_matches(".0").to_string(),
        Err(_) => s,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ticks_follow_the_sign_mode() {
        assert_eq!(ticks(true, 8.0), [8.0, 4.0, 0.0, -4.0, -8.0]);
        assert_eq!(ticks(false, 8.0), [8.0, 4.0, 0.0]);
    }

    #[test]
    fn y_fraction_runs_from_the_top_down() {
        assert_eq!(y_fraction(true, 8.0, 8.0), 0.0);
        assert_eq!(y_fraction(true, 8.0, 0.0), 0.5);
        assert_eq!(y_fraction(true, 8.0, -8.0), 1.0);
        assert_eq!(y_fraction(false, 8.0, 0.0), 1.0);
        assert_eq!(y_fraction(false, 8.0, 2.0), 0.75);
        assert_eq!(y_fraction(true, 8.0, 100.0), 0.0); // clamped
    }

    #[test]
    fn tick_labels_are_short() {
        assert_eq!(format_value_short(7.0), "7");
        assert_eq!(format_value_short(3.5), "3.5");
        assert_eq!(format_value_short(12.4), "12");
        assert_eq!(format_value_short(0.0), "0");
        assert_eq!(format_value_short(-3.26), "-3.3");
    }
}
