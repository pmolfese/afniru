//! Graph tool: the settings of the Graph view (the fourth cell of the 2×2
//! layout): which dataset is plotted, an optional fit, the matrix of
//! neighboring voxels, how many time points to ignore, detrending, percent
//! of the mean, and a stimulus to shade. The plot itself is `ui::graph_view`;
//! the arithmetic is [`series`].

pub mod series;

use egui::{ComboBox, DragValue, RichText, Ui};

use super::{Instance, Tool, ToolContext};
use crate::session::Action;
use crate::session::series::{Detrend, SeriesChange};

/// The Graph tool.
pub struct GraphTool;

impl GraphTool {
    /// The dataset plotted and its number of time points.
    fn plotted<'a>(cx: &'a ToolContext) -> Option<(&'a str, usize)> {
        match cx.controller.series.source {
            Some(id) => cx.session.store.get(id).map(|d| (d.name.as_str(), d.nvols)),
            None => cx.dataset.map(|d| (d.name.as_str(), d.nvols)),
        }
    }
}

impl Tool for GraphTool {
    fn card_ui(&self, ui: &mut Ui, cx: &ToolContext, _instance: &Instance) -> Vec<Action> {
        let theme = cx.theme;
        let s = &cx.controller.series;
        let mut actions = Vec::new();
        let change = |c: SeriesChange| Action::Series(c);
        let (name, len) = Self::plotted(cx).unwrap_or(("none", 0));

        egui::Grid::new("graph_grid").num_columns(2).show(ui, |ui| {
            ui.label(RichText::new("Dataset").color(theme.text_dim));
            ComboBox::from_id_salt("graph_source")
                .width(ui.available_width())
                .selected_text(if s.source.is_none() {
                    format!("ULay · {name}")
                } else {
                    name.to_string()
                })
                .show_ui(ui, |ui| {
                    if ui
                        .selectable_label(s.source.is_none(), "ULay (the underlay)")
                        .clicked()
                    {
                        actions.push(change(SeriesChange::Source(None)));
                    }
                    for (id, d) in cx.session.store.iter() {
                        if d.nvols > 1
                            && ui
                                .selectable_label(s.source == Some(id), &d.name)
                                .on_hover_text(format!("{} time points", d.nvols))
                                .clicked()
                        {
                            actions.push(change(SeriesChange::Source(Some(id))));
                        }
                    }
                });
            ui.end_row();

            ui.label(RichText::new("Fit").color(theme.text_dim));
            let fit_name = s
                .fit
                .and_then(|id| cx.session.store.get(id))
                .map_or("none", |d| d.name.as_str());
            ComboBox::from_id_salt("graph_fit")
                .width(ui.available_width())
                .selected_text(fit_name)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(s.fit.is_none(), "none").clicked() {
                        actions.push(change(SeriesChange::Fit(None)));
                    }
                    for (id, d) in cx.session.store.iter() {
                        // A fit needs a point for every point of the series.
                        if d.nvols == len
                            && len > 1
                            && ui.selectable_label(s.fit == Some(id), &d.name).clicked()
                        {
                            actions.push(change(SeriesChange::Fit(Some(id))));
                        }
                    }
                });
            ui.end_row();

            ui.label(RichText::new("Matrix").color(theme.text_dim));
            ui.horizontal(|ui| {
                for n in [1u8, 3, 5] {
                    if ui
                        .selectable_label(s.matrix == n, format!("{n}×{n}"))
                        .on_hover_text("Graphs of the voxels around the crosshair")
                        .clicked()
                    {
                        actions.push(change(SeriesChange::Matrix(n)));
                    }
                }
            });
            ui.end_row();

            ui.label(RichText::new("Ignore").color(theme.text_dim));
            let mut ignore = s.ignore;
            if ui
                .add(
                    DragValue::new(&mut ignore)
                        .range(0..=len.saturating_sub(1))
                        .suffix(" TRs"),
                )
                .on_hover_text("Leave out the first time points")
                .changed()
            {
                actions.push(change(SeriesChange::Ignore(ignore)));
            }
            ui.end_row();

            ui.label(RichText::new("Detrend").color(theme.text_dim));
            ui.horizontal(|ui| {
                ComboBox::from_id_salt("graph_detrend")
                    .width(90.0)
                    .selected_text(s.detrend.label())
                    .show_ui(ui, |ui| {
                        for d in Detrend::ALL {
                            if ui.selectable_label(s.detrend == d, d.label()).clicked() {
                                actions.push(change(SeriesChange::Detrend(d)));
                            }
                        }
                    });
                let mut percent = s.percent;
                if ui
                    .checkbox(&mut percent, "% of mean")
                    .on_hover_text("Plot percent change from the mean")
                    .changed()
                {
                    actions.push(change(SeriesChange::Percent(percent)));
                }
            });
            ui.end_row();

            ui.label(RichText::new("Stimulus").color(theme.text_dim));
            ui.horizontal(|ui| match &s.stim {
                Some(stim) => {
                    ui.label(RichText::new(&stim.name).small().monospace());
                    if ui.small_button("Clear").clicked() {
                        actions.push(change(SeriesChange::Stim(None)));
                    }
                }
                None => {
                    if ui
                        .button("Load 1D…")
                        .on_hover_text(
                            "A .1D column of 0/1 (or any regressor): where it is on is shaded",
                        )
                        .clicked()
                    {
                        actions.push(Action::LoadStim);
                    }
                }
            });
            ui.end_row();
        });
        if len < 2 {
            ui.add_space(2.0);
            ui.label(
                RichText::new("Open or choose a dataset with several time points to plot.")
                    .small()
                    .color(theme.text_faint),
            );
        }
        actions
    }

    fn summary(&self, cx: &ToolContext, _instance: &Instance) -> String {
        let s = &cx.controller.series;
        let (name, len) = Self::plotted(cx).unwrap_or(("none", 0));
        if len < 2 {
            return "no time series".into();
        }
        let mut text = format!("{name} · {} TRs", len);
        if s.matrix > 1 {
            text.push_str(&format!(" · {0}×{0}", s.matrix));
        }
        if s.fit.is_some() {
            text.push_str(" · fit");
        }
        text
    }
}
