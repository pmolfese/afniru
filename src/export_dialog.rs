//! The "Save images" dialog: what to save (the slice, the three views, a
//! montage), how it looks, and the montage's rows, columns and slices.

use egui::{DragValue, RichText};

use crate::geom::Plane;
use crate::render::export::{ExportOptions, ExportWhat, MontageSpec, ViewsLayout};
use crate::ui::theme::Theme;

/// Which kind of picture.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// The slice shown in one plane.
    Slice,
    /// The three views.
    Views,
    /// Many slices of one plane.
    Montage,
}

/// The dialog's state.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportDialog {
    /// The plane the slice or montage is of.
    pub plane: Plane,
    /// What to save.
    pub kind: Kind,
    /// How the three views are arranged.
    pub layout: ViewsLayout,
    /// Montage rows.
    pub rows: usize,
    /// Montage columns.
    pub cols: usize,
    /// First montage slice.
    pub first: usize,
    /// Last montage slice.
    pub last: usize,
    /// Step between montage slices.
    pub step: usize,
    /// How many slices the plane has.
    pub count: usize,
    /// The look of the pictures.
    pub options: ExportOptions,
}

/// What the user decided this frame.
#[derive(Debug, PartialEq)]
pub enum Outcome {
    /// Still open.
    Open,
    /// Closed without saving.
    Cancel,
    /// Save this, with these options.
    Save(ExportWhat, ExportOptions),
}

impl ExportDialog {
    /// A dialog for `plane`, which has `count` slices: a montage of all of
    /// them in 4 rows of 6 starts selected-ready, stepping to fit.
    pub fn new(plane: Plane, count: usize, options: ExportOptions) -> Self {
        let (rows, cols) = (4, 6);
        let last = count.saturating_sub(1);
        Self {
            plane,
            kind: Kind::Slice,
            layout: ViewsLayout::Row,
            rows,
            cols,
            first: 0,
            last,
            step: MontageSpec::step_to_fit(0, last, rows, cols),
            count,
            options,
        }
    }

    /// What the settings ask to save.
    pub fn what(&self) -> ExportWhat {
        match self.kind {
            Kind::Slice => ExportWhat::Slice(self.plane),
            Kind::Views => ExportWhat::Views(self.layout),
            Kind::Montage => ExportWhat::Montage(MontageSpec {
                plane: self.plane,
                rows: self.rows,
                cols: self.cols,
                first: self.first.min(self.last),
                last: self.last,
                step: self.step.max(1),
            }),
        }
    }

    /// How many tiles the montage would have.
    pub fn montage_tiles(&self) -> usize {
        match self.what() {
            ExportWhat::Montage(m) => m.slices(self.count).len(),
            _ => 0,
        }
    }

    /// Draw the dialog's contents.
    pub fn ui(&mut self, ui: &mut egui::Ui, theme: &Theme) -> Outcome {
        let mut outcome = Outcome::Open;
        ui.set_width(380.0);
        ui.heading("Save images");
        ui.add_space(6.0);
        let plane = self.plane.name().to_lowercase();
        ui.radio_value(&mut self.kind, Kind::Slice, format!("This {plane} slice"));
        ui.radio_value(&mut self.kind, Kind::Views, "The three views");
        if self.kind == Kind::Views {
            ui.horizontal(|ui| {
                ui.add_space(20.0);
                for layout in ViewsLayout::ALL {
                    ui.selectable_value(&mut self.layout, layout, layout.label());
                }
            });
            if self.layout == ViewsLayout::Grid {
                ui.label(
                    RichText::new("The 2×2 grid leaves the Graph's cell empty.")
                        .small()
                        .color(theme.text_dim),
                );
            }
        }
        ui.radio_value(
            &mut self.kind,
            Kind::Montage,
            format!("A montage of {plane} slices"),
        );
        if self.kind == Kind::Montage {
            let max = self.count.saturating_sub(1);
            egui::Grid::new("montage_grid")
                .num_columns(4)
                .show(ui, |ui| {
                    ui.label("rows");
                    ui.add(DragValue::new(&mut self.rows).range(1..=20));
                    ui.label("columns");
                    ui.add(DragValue::new(&mut self.cols).range(1..=20));
                    ui.end_row();
                    ui.label("first slice");
                    ui.add(DragValue::new(&mut self.first).range(0..=max));
                    ui.label("last slice");
                    ui.add(DragValue::new(&mut self.last).range(0..=max));
                    ui.end_row();
                    ui.label("step");
                    ui.add(DragValue::new(&mut self.step).range(1..=max.max(1)));
                    if ui
                        .small_button("fit")
                        .on_hover_text("Choose the step that fits the range into the tiles")
                        .clicked()
                    {
                        self.step = MontageSpec::step_to_fit(
                            self.first.min(self.last),
                            self.last,
                            self.rows,
                            self.cols,
                        );
                    }
                    ui.end_row();
                });
            ui.label(
                RichText::new(format!(
                    "{} slices (of {})",
                    self.montage_tiles(),
                    self.count
                ))
                .small()
                .color(theme.text_dim),
            );
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label("Size");
            ui.add(
                DragValue::new(&mut self.options.zoom)
                    .range(1..=8)
                    .prefix("×"),
            )
            .on_hover_text("Pixels per voxel (along the smallest voxel edge)");
            ui.checkbox(&mut self.options.letters, "orientation letters");
        });
        ui.horizontal(|ui| {
            ui.checkbox(&mut self.options.label.show, "slice number")
                .on_hover_text("Where and how big: right-click an image");
            ui.add_enabled(
                self.kind != Kind::Montage,
                egui::Checkbox::new(&mut self.options.crosshair, "crosshair"),
            );
        });
        ui.add_space(8.0);
        ui.horizontal(|ui| {
            if ui.button("Save…").clicked() {
                outcome = Outcome::Save(self.what(), self.options);
            }
            if ui.button("Cancel").clicked() {
                outcome = Outcome::Cancel;
            }
        });
        outcome
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_dialog_starts_with_a_fitting_montage_ready() {
        let d = ExportDialog::new(Plane::Axial, 150, ExportOptions::default());
        assert_eq!(d.what(), ExportWhat::Slice(Plane::Axial));
        let mut d = d;
        d.kind = Kind::Montage;
        match d.what() {
            ExportWhat::Montage(m) => {
                assert_eq!((m.rows, m.cols, m.first, m.last, m.step), (4, 6, 0, 149, 7));
            }
            other => panic!("{other:?}"),
        }
        assert_eq!(d.montage_tiles(), 22); // 0, 7, ..., 147
    }

    #[test]
    fn kinds_map_to_what_is_saved() {
        let mut d = ExportDialog::new(Plane::Coronal, 10, ExportOptions::default());
        d.kind = Kind::Views;
        d.layout = ViewsLayout::Grid;
        assert_eq!(d.what(), ExportWhat::Views(ViewsLayout::Grid));
        // A first slice after the last is made to fit.
        d.kind = Kind::Montage;
        d.first = 8;
        d.last = 3;
        if let ExportWhat::Montage(m) = d.what() {
            assert!(m.first <= m.last);
        }
    }
}
