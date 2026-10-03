//! Clusterize tool: group the voxels that pass an overlay layer's threshold
//! into clusters, drop the small ones, and list the rest.
//!
//! It is **hooked under Define Overlay**, one card per layer ("hooks are per
//! layer"): attach it with the Clusterize chip in a layer's card (or its tile
//! on the shelf). The settings are the layer's ([`ClusterSettings`]); the work
//! is in [`compute`] (matches `3dClusterize`) and [`engine`] (when to rerun).
//! A row of the table jumps the crosshair to the cluster's peak.

pub mod compute;
pub mod engine;

use egui::{ComboBox, DragValue, Label, RichText, ScrollArea, Sense, Ui, vec2};

use super::{Instance, Tool, ToolContext, ToolId};
use crate::geom::coords::ijk_to_ras;
use crate::session::{Action, ClusterSettings, LayerId, OverlayChange, SizeUnit};
use crate::ui::widgets::readout::format_value;

/// The Clusterize tool.
pub struct ClusterizeTool;

/// Rows shown before the table says there are more.
const MAX_ROWS: usize = 200;

impl Tool for ClusterizeTool {
    fn attaches_to(&self) -> Option<ToolId> {
        Some(ToolId::Overlay)
    }

    fn link_label(&self) -> &'static str {
        "clusters the threshold"
    }

    fn is_hooked(&self, cx: &ToolContext, parent: u64) -> bool {
        cx.overlay(LayerId(parent))
            .is_some_and(|o| o.layer.cluster.is_some())
    }

    fn hook_action(&self, parent: u64, hooked: bool) -> Option<Action> {
        Some(Action::Layer(
            LayerId(parent),
            OverlayChange::Cluster(hooked.then(ClusterSettings::default)),
        ))
    }

    fn opened(&self, cx: &ToolContext) -> Vec<Action> {
        if cx.overlays.iter().any(|o| o.layer.cluster.is_some()) {
            return Vec::new();
        }
        // The top layer, as the most recently added is the one being worked on.
        cx.overlays
            .last()
            .and_then(|o| self.hook_action(o.layer.id.0, true))
            .into_iter()
            .collect()
    }

    fn instances(&self, cx: &ToolContext) -> Vec<Instance> {
        let hooked: Vec<_> = cx
            .overlays
            .iter()
            .rev()
            .filter(|o| o.layer.cluster.is_some())
            .collect();
        if hooked.is_empty() {
            return vec![Instance::single()];
        }
        let many = hooked.len() > 1;
        hooked
            .into_iter()
            .map(|o| Instance {
                id: o.layer.id.0,
                title: many.then(|| format!("Clusterize · Overlay {}", o.layer.id.0)),
            })
            .collect()
    }

    fn card_ui(&self, ui: &mut Ui, cx: &ToolContext, instance: &Instance) -> Vec<Action> {
        let theme = cx.theme;
        let Some(o) = cx.overlay(LayerId(instance.id)) else {
            ui.label(
                RichText::new(
                    "Not attached. Use the Clusterize chip in a Define Overlay card, \
                     or this tile with an overlay loaded.",
                )
                .color(theme.text_dim),
            );
            return Vec::new();
        };
        let Some(settings) = o.layer.cluster else {
            return Vec::new();
        };
        let layer = o.layer;
        let mut actions = Vec::new();
        let mut next = settings;

        ui.horizontal(|ui| {
            ui.label(RichText::new("NN").color(theme.text_dim));
            for nn in 1..=3u8 {
                if ui
                    .selectable_label(settings.nn == nn, nn.to_string())
                    .on_hover_text(match nn {
                        1 => "Voxels touch by a face",
                        2 => "Voxels touch by a face or an edge",
                        _ => "Voxels touch by a face, an edge or a corner",
                    })
                    .clicked()
                {
                    next.nn = nn;
                }
            }
            ui.add_space(6.0);
            ui.label(RichText::new("min").color(theme.text_dim));
            ui.add(
                DragValue::new(&mut next.min_size)
                    .range(0.0..=f64::MAX)
                    .speed(0.5)
                    .max_decimals(1),
            )
            .on_hover_text("Smallest cluster kept");
            ComboBox::from_id_salt("cluster_unit")
                .width(52.0)
                .selected_text(match settings.unit {
                    SizeUnit::Voxels => "vox",
                    SizeUnit::Microliters => "µL",
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut next.unit, SizeUnit::Voxels, "vox");
                    ui.selectable_value(&mut next.unit, SizeUnit::Microliters, "µL");
                });
        });
        ui.horizontal_wrapped(|ui| {
            if layer.signed && !layer.as_mask {
                ui.checkbox(&mut next.bisided, "bisided")
                    .on_hover_text("Cluster positive and negative values separately");
            }
            let on = if layer.as_mask {
                "on the mask".to_string()
            } else {
                format!(
                    "on {}≥ {}",
                    if layer.signed { "|thr| " } else { "thr " },
                    format_value(layer.threshold as f32)
                )
            };
            ui.label(RichText::new(on).color(theme.accent).small());
            ui.checkbox(&mut next.only_clusters, "only clusters")
                .on_hover_text("Draw only the voxels inside the surviving clusters");
        });
        if next != settings {
            actions.push(Action::Layer(layer.id, OverlayChange::Cluster(Some(next))));
        }

        ui.add_space(2.0);
        match o.cluster {
            None => {
                ui.label(RichText::new("clustering…").color(theme.text_dim).small());
            }
            Some(entry) => match &entry.result {
                Err(e) => {
                    ui.label(RichText::new(e).color(theme.error).small());
                }
                Ok(out) => {
                    let stale = if entry.stale { "  (updating)" } else { "" };
                    ui.label(
                        RichText::new(format!(
                            "{} · {} voxels{stale}",
                            plural(out.rows.len(), "cluster"),
                            out.total_voxels
                        ))
                        .color(theme.text_dim)
                        .small(),
                    );
                    if !out.rows.is_empty() {
                        let here = cx.dataset.and_then(|d| {
                            out.rank_at(ijk_to_ras(&d.ijk_to_ras, cx.controller.cursor.ijk))
                        });
                        actions.extend(table(ui, cx, !out.has_values, out, here));
                    }
                }
            },
        }
        actions
    }

    fn summary(&self, cx: &ToolContext, instance: &Instance) -> String {
        let Some(s) = cx
            .overlay(LayerId(instance.id))
            .and_then(|o| o.layer.cluster)
        else {
            return "not attached".into();
        };
        let found = match cx.overlay(LayerId(instance.id)).and_then(|o| o.cluster) {
            Some(e) => match &e.result {
                Ok(out) => plural(out.rows.len(), "cluster"),
                Err(_) => "error".into(),
            },
            None => "…".into(),
        };
        format!(
            "NN{} · ≥ {} {} · {found}",
            s.nn,
            s.min_size,
            match s.unit {
                SizeUnit::Voxels => "vox",
                SizeUnit::Microliters => "µL",
            }
        )
    }
}

fn plural(n: usize, word: &str) -> String {
    format!("{n} {word}{}", if n == 1 { "" } else { "s" })
}

/// A coordinate in millimeters: whole numbers without a decimal point.
fn mm(v: f64) -> String {
    let v = if v.abs() < 0.05 { 0.0 } else { v };
    if (v - v.round()).abs() < 0.05 {
        format!("{:.0}", v.round())
    } else {
        format!("{v:.1}")
    }
}

/// The cluster table: rank, voxels, peak and where it is. A click jumps to
/// the peak (the center, for a mask, which has no peak).
fn table(
    ui: &mut Ui,
    cx: &ToolContext,
    is_mask: bool,
    out: &compute::ClusterOutcome,
    here: Option<u32>,
) -> Vec<Action> {
    let theme = cx.theme;
    let mut actions = Vec::new();
    let head = |ui: &mut Ui, text: &str| {
        ui.label(RichText::new(text).small().color(theme.text_faint));
    };
    ScrollArea::vertical()
        .max_height(180.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("cluster_table")
                .num_columns(4)
                .spacing(vec2(10.0, 2.0))
                .min_col_width(24.0)
                .show(ui, |ui| {
                    head(ui, "#");
                    head(ui, "vox");
                    head(ui, if is_mask { "" } else { "peak" });
                    head(ui, if is_mask { "center" } else { "x y z" });
                    ui.end_row();
                    for row in out.rows.iter().take(MAX_ROWS) {
                        let at = if is_mask {
                            row.center_ras
                        } else {
                            row.peak_ras
                        };
                        let [x, y, z] = cx.coord_orient.ras_to_coords(at);
                        let ink = if here == Some(row.rank) {
                            theme.accent
                        } else {
                            theme.text
                        };
                        let cells = [
                            row.rank.to_string(),
                            row.voxels.to_string(),
                            if is_mask {
                                String::new()
                            } else {
                                format_value(row.peak as f32)
                            },
                            format!("{} {} {}", mm(x), mm(y), mm(z)),
                        ];
                        let mut clicked = false;
                        for cell in cells {
                            clicked |= ui
                                .add(
                                    Label::new(RichText::new(cell).monospace().color(ink))
                                        .sense(Sense::click()),
                                )
                                .on_hover_text(format!(
                                    "{} µL · click to go to the {}",
                                    mm(row.volume_ul),
                                    if is_mask { "center" } else { "peak" }
                                ))
                                .clicked();
                        }
                        ui.end_row();
                        if clicked {
                            actions.push(Action::JumpToRas(at));
                        }
                    }
                });
            if out.rows.len() > MAX_ROWS {
                ui.label(
                    RichText::new(format!("… {} more", out.rows.len() - MAX_ROWS))
                        .small()
                        .color(theme.text_faint),
                );
            }
        });
    actions
}
