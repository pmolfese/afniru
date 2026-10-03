//! Crosshair tool: the crosshair's coordinates (editable), voxel index and the
//! underlay value there. Editing a coordinate asks the session to jump.

use egui::{Color32, DragValue, RichText, Sense, Ui, vec2};

use super::{Instance, Tool, ToolContext};
use crate::geom::coords::{ijk_to_ras, magnitude_and_letter};
use crate::session::Action;
use crate::ui::theme;
use crate::ui::widgets::readout::format_value;

/// The Crosshair tool.
pub struct CrosshairTool;

/// The plane color of the crosshair line along each RAS axis: x is the
/// sagittal plane (orange), y coronal (green), z axial (blue).
const AXIS_COLORS: [Color32; 3] = [theme::SAGITTAL, theme::CORONAL, theme::AXIAL];

impl Tool for CrosshairTool {
    fn card_ui(&self, ui: &mut Ui, cx: &ToolContext, _instance: &Instance) -> Vec<Action> {
        let mut actions = Vec::new();
        let Some(ds) = cx.dataset else {
            ui.label(RichText::new("no dataset").color(cx.theme.text_faint));
            return actions;
        };
        let ijk = cx.controller.cursor.ijk;
        let ras = ijk_to_ras(&ds.ijk_to_ras, ijk);
        // No "-0.0": values that round to zero show as 0.0.
        let mut coords = cx
            .coord_orient
            .ras_to_coords(ras)
            .map(|v| if v.abs() < 0.05 { 0.0 } else { v });

        let mut edited = false;
        for axis in 0..3 {
            ui.horizontal(|ui| {
                let (bar, _) = ui.allocate_exact_size(vec2(3.0, 18.0), Sense::hover());
                ui.painter().rect_filled(bar, 1.0, AXIS_COLORS[axis]);
                ui.label(
                    RichText::new(["x", "y", "z"][axis])
                        .color(cx.theme.text_dim)
                        .monospace(),
                );
                let before = coords[axis];
                ui.add(
                    DragValue::new(&mut coords[axis])
                        .speed(0.5)
                        .fixed_decimals(1)
                        .suffix(" mm"),
                );
                edited |= coords[axis] != before;
                // The letter says which side of the origin the value is on.
                let (_, side) = magnitude_and_letter(cx.coord_orient.coords_to_ras(coords))[axis];
                ui.label(
                    RichText::new(side.to_string())
                        .color(cx.theme.text)
                        .monospace(),
                );
            });
        }
        if edited {
            actions.push(Action::JumpToRas(cx.coord_orient.coords_to_ras(coords)));
        }

        ui.add_space(2.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("ijk").color(cx.theme.text_dim).monospace());
            let mut new = ijk;
            for (a, v) in new.iter_mut().enumerate() {
                ui.add(
                    DragValue::new(v)
                        .range(0..=ds.dims[a].saturating_sub(1))
                        .speed(0.2),
                );
            }
            if new != ijk {
                actions.push(Action::MoveCrosshair(new));
            }
        });
        let value = cx.value.map_or("--".to_string(), format_value);
        ui.label(
            RichText::new(format!("ULay  {value}"))
                .color(cx.theme.text)
                .monospace(),
        );
        // Every overlay layer's values here, top layer first, each with a
        // swatch of the color it is drawn in (a ring when it is not drawn).
        for o in cx.overlays.iter().rev() {
            let Some((olay, thr)) = o.values else {
                continue;
            };
            let color = o.drawn;
            ui.horizontal(|ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(12.0, 12.0), Sense::hover());
                if let Some(c) = color {
                    let c = c.to_u8();
                    ui.painter()
                        .rect_filled(rect, 2.0, Color32::from_rgb(c[0], c[1], c[2]));
                } else {
                    ui.painter().rect_stroke(
                        rect,
                        2.0,
                        egui::Stroke::new(1.0, cx.theme.text_faint),
                        egui::StrokeKind::Inside,
                    );
                }
                ui.label(
                    RichText::new(format!(
                        "Overlay {}  OLay {}  Thr {}",
                        o.layer.id.0,
                        format_value(olay),
                        format_value(thr)
                    ))
                    .color(cx.theme.text)
                    .monospace(),
                );
            });
        }
        actions
    }

    fn summary(&self, cx: &ToolContext, _instance: &Instance) -> String {
        let Some(ds) = cx.dataset else {
            return "no dataset".into();
        };
        let ras = ijk_to_ras(&ds.ijk_to_ras, cx.controller.cursor.ijk);
        magnitude_and_letter(ras)
            .iter()
            .map(|(m, l)| format!("{m:.1}{l}"))
            .collect::<Vec<_>>()
            .join(" ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::data::synthetic;
    use crate::geom::CoordOrient;
    use crate::session::Session;
    use crate::ui::theme::Theme;

    #[test]
    fn summary_is_magnitude_and_letter_per_axis() {
        let mut s = Session::new();
        s.add_dataset(synthetic::phantom());
        s.apply(Action::MoveCrosshair([0, 0, 0]));
        let theme = Theme::dark();
        let ds = s.underlay().cloned();
        let cx = ToolContext {
            theme: &theme,
            session: &s,
            controller: s.controller(),
            dataset: ds.as_deref(),
            coord_orient: CoordOrient::Rai,
            value: None,
            loading: &[],
            folders: &[],
            overlays: Vec::new(),
        };
        assert_eq!(
            CrosshairTool.summary(&cx, &Instance::single()),
            "75.0R 90.0A 75.0I"
        );
    }
}
