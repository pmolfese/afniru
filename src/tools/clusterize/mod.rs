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

use egui::{Button, ComboBox, DragValue, Label, RichText, ScrollArea, Sense, Ui, vec2};
use egui_phosphor::regular as icon;

use super::{Instance, Tool, ToolContext, ToolId};
use crate::geom::coords::ijk_to_ras;
use compute::SortBy;

use crate::session::{Action, ClusterSettings, LayerId, OverlayChange, OverlayLayer, SizeUnit};
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
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(format!(
                                "{} · {} voxels{stale}",
                                plural(out.rows.len(), "cluster"),
                                out.total_voxels
                            ))
                            .color(theme.text_dim)
                            .small(),
                        );
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            let any = !out.rows.is_empty();
                            if ui
                                .add_enabled(any, Button::new(RichText::new("Save…").small()))
                                .on_hover_text("Write the table to a text file")
                                .clicked()
                            {
                                actions.push(Action::SaveClusters(layer.id));
                            }
                            if ui
                                .add_enabled(any, Button::new(RichText::new("Copy").small()))
                                .on_hover_text("Copy the table (tab separated) to the clipboard")
                                .clicked()
                            {
                                let name = o.dataset.name.as_str();
                                let text = compute::report_text(
                                    out,
                                    cx.coord_orient,
                                    &heading(layer, name, &settings),
                                );
                                ui.ctx().copy_text(text);
                            }
                        });
                    });
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

/// The first line of a saved or copied table: what was clustered, and how.
pub fn heading(layer: &OverlayLayer, dataset: &str, s: &ClusterSettings) -> String {
    let what = if layer.as_mask {
        format!("overlay {} ({dataset}) as a mask", layer.id.0)
    } else {
        format!(
            "overlay {} ({dataset}) {}thr >= {}",
            layer.id.0,
            if layer.signed { "|" } else { "" },
            layer.threshold
        )
    };
    format!(
        "afniru clusters of {what}; NN{}, at least {} {}, {}",
        s.nn,
        s.min_size,
        match s.unit {
            SizeUnit::Voxels => "voxels",
            SizeUnit::Microliters => "uL",
        },
        if s.bisided { "bisided" } else { "not bisided" }
    )
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

/// Where a click on a cluster takes the crosshair. The first click goes to the
/// peak, a second click on the same cluster to its center of mass, and further
/// clicks alternate.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum JumpTo {
    /// The voxel of largest absolute value.
    Peak,
    /// The center of mass (weighted by absolute value).
    Center,
}

impl JumpTo {
    fn other(self) -> Self {
        match self {
            JumpTo::Peak => JumpTo::Center,
            JumpTo::Center => JumpTo::Peak,
        }
    }

    /// The tag shown on the row the crosshair was sent to.
    fn tag(self) -> String {
        match self {
            JumpTo::Peak => format!("{} peak", icon::TARGET),
            JumpTo::Center => format!("{} center", icon::CROSSHAIR_SIMPLE),
        }
    }

    fn ras(self, row: &compute::ClusterRow) -> [f64; 3] {
        match self {
            JumpTo::Peak => row.peak_ras,
            JumpTo::Center => row.center_ras,
        }
    }
}

/// The last jump from the table: which cluster, to what, and the voxel the
/// crosshair landed on (the tag is shown only while it is still there).
type Landed = (u32, JumpTo, [usize; 3]);

/// The cluster table: rank, voxels, peak and where it is. A click jumps to
/// the peak; clicking the same cluster again goes to its center of mass, and
/// so on; the row shows which one the crosshair is at. (A mask has no peak:
/// its clusters are always visited at their center.)
fn table(
    ui: &mut Ui,
    cx: &ToolContext,
    is_mask: bool,
    out: &compute::ClusterOutcome,
    here: Option<u32>,
) -> Vec<Action> {
    let theme = cx.theme;
    let mut actions = Vec::new();
    // Where the last click took the crosshair, if it is still there.
    let landed_id = ui.id().with("cluster_landed");
    let cursor = cx.controller.cursor.ijk;
    let landed: Option<Landed> = ui
        .data(|d| d.get_temp::<Landed>(landed_id))
        .filter(|(_, _, ijk)| *ijk == cursor);
    // Sorting: click a heading to order by it, again to reverse.
    let sort_id = ui.id().with("cluster_sort");
    let (by, descending): (SortBy, bool) = ui.data(|d| d.get_temp(sort_id)).unwrap_or_default();
    let mut next_sort = (by, descending);
    // Sort arrows are Phosphor glyphs (the text font has none).
    let (caret_up, caret_down) = (
        format!(" {}", icon::CARET_UP),
        format!(" {}", icon::CARET_DOWN),
    );
    let mut head = |ui: &mut Ui, text: &str, column: Option<SortBy>| {
        let arrow = match column {
            Some(c) if c == by => {
                if descending {
                    caret_up.as_str()
                } else {
                    caret_down.as_str()
                }
            }
            _ => "",
        };
        let label = Label::new(RichText::new(format!("{text}{arrow}")).small().color(
            if column == Some(by) {
                theme.accent
            } else {
                theme.text_faint
            },
        ));
        match column {
            Some(c) => {
                if ui
                    .add(label.sense(Sense::click()))
                    .on_hover_text("Sort by this column")
                    .clicked()
                {
                    next_sort = if c == by {
                        (by, !descending)
                    } else {
                        (c, false)
                    };
                }
            }
            None => {
                ui.add(label);
            }
        }
    };
    ScrollArea::vertical()
        .max_height(180.0)
        .auto_shrink([false, true])
        .show(ui, |ui| {
            egui::Grid::new("cluster_table")
                .num_columns(5)
                .spacing(vec2(10.0, 2.0))
                .min_col_width(24.0)
                .show(ui, |ui| {
                    head(ui, "#", Some(SortBy::Rank));
                    head(ui, "vox", Some(SortBy::Voxels));
                    if is_mask {
                        head(ui, "", None);
                        head(ui, "center", None);
                    } else {
                        head(ui, "peak", Some(SortBy::Peak));
                        head(ui, "x y z", None);
                    }
                    head(ui, "at", None);
                    ui.end_row();
                    for row in compute::sorted(&out.rows, by, descending)
                        .into_iter()
                        .take(MAX_ROWS)
                    {
                        // What a click on this row does now: the peak, or (the
                        // crosshair being at this cluster's peak) its center.
                        let mode_here = landed.filter(|(r, _, _)| *r == row.rank).map(|l| l.1);
                        let next = if is_mask {
                            JumpTo::Center
                        } else {
                            mode_here.map_or(JumpTo::Peak, JumpTo::other)
                        };
                        // The row shows where the crosshair is, or where a click
                        // would go.
                        let shown = mode_here.unwrap_or(next);
                        let at = shown.ras(row);
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
                            mode_here.map_or_else(String::new, JumpTo::tag),
                        ];
                        let mut clicked = false;
                        for cell in cells {
                            clicked |= ui
                                .add(
                                    Label::new(RichText::new(cell).monospace().color(ink))
                                        .sense(Sense::click()),
                                )
                                .on_hover_text(format!(
                                    "{} µL · click: go to the {}{}",
                                    mm(row.volume_ul),
                                    if next == JumpTo::Peak {
                                        "peak"
                                    } else {
                                        "center of mass"
                                    },
                                    if is_mask {
                                        ""
                                    } else {
                                        " · click again: the other"
                                    }
                                ))
                                .clicked();
                        }
                        ui.end_row();
                        if clicked {
                            let target = next.ras(row);
                            actions.push(Action::JumpToRas(target));
                            // Remember the voxel it lands on, to label the row.
                            if let Some(d) = cx.dataset
                                && let Some(ijk) =
                                    crate::geom::coords::ras_to_ijk(&d.ijk_to_ras, d.dims, target)
                            {
                                ui.data_mut(|m| {
                                    m.insert_temp::<Landed>(landed_id, (row.rank, next, ijk))
                                });
                            }
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
    if next_sort != (by, descending) {
        ui.data_mut(|d| d.insert_temp(sort_id, next_sort));
    }
    actions
}
