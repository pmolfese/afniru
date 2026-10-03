//! The Graph view: the time series at the crosshair, in the fourth cell of
//! the 2×2 layout.
//!
//! One large plot, or a 3×3 / 5×5 matrix of the neighboring voxels' series
//! (laid out like the slice in the active plane, one shared y range). The
//! series can have a fit drawn over it, stimulus blocks shaded behind it, and
//! a marker at the current time point of the underlay; clicking or dragging in
//! the plot moves that time point, and clicking a graph of the matrix moves
//! the crosshair there. The arithmetic is `tools::graph::series`.

use egui::{Color32, Rect, RichText, Stroke, Ui, UiBuilder, Vec2b, pos2, vec2};
use egui_plot::{Line, Plot, PlotPoints, Polygon, VLine};

use crate::data::Dataset;
use crate::geom::Plane;
use crate::geom::coords::{ijk_to_ras, ras_to_ijk};
use crate::session::SeriesSettings;
use crate::tools::graph::series::{self, Stats};
use crate::ui::theme::Theme;
use crate::ui::widgets::readout::format_value;

/// What the view needs to draw.
pub struct GraphInput<'a> {
    /// Colors.
    pub theme: &'a Theme,
    /// The underlay (the crosshair lives on its grid).
    pub under: &'a Dataset,
    /// The dataset plotted (the underlay itself, or another).
    pub source: &'a Dataset,
    /// Is `source` the underlay? Then its current sub-brick is marked.
    pub source_is_under: bool,
    /// The fit drawn over the series.
    pub fit: Option<&'a Dataset>,
    /// Matrix, ignore, detrend, stimulus.
    pub settings: &'a SeriesSettings,
    /// The crosshair voxel on the underlay's grid.
    pub cursor: [usize; 3],
    /// The plane whose layout the matrix follows.
    pub plane: Plane,
    /// Neurological display?
    pub left_is_left: bool,
    /// The underlay's displayed sub-brick.
    pub current_tr: usize,
}

/// What the user did in the graph.
#[derive(Debug, Default, Clone, PartialEq)]
pub struct GraphEvents {
    /// Show this time point of the underlay.
    pub set_tr: Option<usize>,
    /// Move the crosshair to this voxel of the underlay.
    pub move_to: Option<[usize; 3]>,
}

/// The colors of the traces, chosen for the theme: the data in the text
/// color (dark on the light theme, light on the dark one), the fit in orange
/// and the time marker in gold, each darker on a light background.
struct Ink {
    data: Color32,
    fit: Color32,
    marker: Color32,
    stimulus: Color32,
}

fn ink(theme: &Theme) -> Ink {
    if theme.dark {
        Ink {
            data: Color32::from_rgb(235, 235, 240),
            fit: Color32::from_rgb(242, 140, 72),
            marker: Color32::from_rgb(250, 204, 21),
            stimulus: Color32::from_rgba_unmultiplied(250, 204, 21, 36),
        }
    } else {
        Ink {
            data: Color32::from_rgb(25, 28, 36),
            fit: Color32::from_rgb(214, 88, 8),
            marker: Color32::from_rgb(180, 120, 0),
            stimulus: Color32::from_rgba_unmultiplied(235, 170, 0, 56),
        }
    }
}

/// The voxel of `to` that covers the center of voxel `ijk` of `from`.
fn map_voxel(from: &Dataset, to: &Dataset, ijk: [usize; 3]) -> Option<[usize; 3]> {
    if from.dims == to.dims && from.ijk_to_ras == to.ijk_to_ras {
        return Some(ijk);
    }
    ras_to_ijk(&to.ijk_to_ras, to.dims, ijk_to_ras(&from.ijk_to_ras, ijk))
}

/// One voxel's plotted series.
struct Cell {
    ijk: [usize; 3],
    first: usize,
    values: Vec<f64>,
    fit: Option<Vec<f64>>,
}

fn cell(input: &GraphInput, ijk: [usize; 3]) -> Option<Cell> {
    let raw = input.source.series(ijk)?;
    let fit_raw = input
        .fit
        .and_then(|f| map_voxel(input.source, f, ijk).and_then(|v| f.series(v)));
    let (first, values, fit) = series::plotted_with_fit(&raw, fit_raw.as_deref(), input.settings);
    Some(Cell {
        ijk,
        first,
        values,
        fit,
    })
}

fn points(first: usize, values: &[f64]) -> PlotPoints<'static> {
    PlotPoints::new(
        values
            .iter()
            .enumerate()
            .filter(|(_, v)| v.is_finite())
            .map(|(i, v)| [(first + i) as f64, *v])
            .collect(),
    )
}

/// Draw the Graph view into `ui` (the cell's contents).
pub fn graph_view(ui: &mut Ui, input: &GraphInput) -> GraphEvents {
    let theme = input.theme;
    let mut events = GraphEvents::default();
    ui.horizontal(|ui| {
        let (dot, _) = ui.allocate_exact_size(vec2(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(dot.center(), 4.0, theme.accent);
        ui.label(RichText::new("Graph").color(theme.text).strong());
    });

    // Where the crosshair is, in the plotted dataset's grid.
    let Some(center) = map_voxel(input.under, input.source, input.cursor) else {
        message(
            ui,
            theme,
            &format!("the crosshair is outside {}", input.source.name),
        );
        return events;
    };
    if input.source.nvols < 2 {
        message(
            ui,
            theme,
            "no time series: open a 4D dataset, or pick one in the Graph card",
        );
        return events;
    }
    let n = input.settings.matrix.clamp(1, 5);
    let layout = series::matrix_voxels(
        input.source.dims,
        &input.source.orient,
        input.plane,
        input.left_is_left,
        center,
        if n % 2 == 1 { n } else { 1 },
    );
    let cells: Vec<Vec<Option<Cell>>> = layout
        .iter()
        .map(|row| row.iter().map(|v| v.and_then(|v| cell(input, v))).collect())
        .collect();
    let Some(mid) = cells.iter().flatten().flatten().find(|c| c.ijk == center) else {
        message(ui, theme, "this voxel has no readable series");
        return events;
    };

    // The footer's height is reserved first.
    let footer = 34.0;
    let area = ui.available_rect_before_wrap();
    let plot_area = Rect::from_min_max(
        area.min,
        pos2(area.right(), (area.bottom() - footer).max(area.top())),
    );
    let marker = input.source_is_under.then_some(input.current_tr as f64);

    if n <= 1 {
        let mut child = ui.new_child(UiBuilder::new().max_rect(plot_area));
        big_plot(&mut child, input, mid, marker, &mut events);
    } else {
        // One y range for the whole matrix.
        let all: Vec<f64> = cells
            .iter()
            .flatten()
            .flatten()
            .flat_map(|c| c.values.iter().chain(c.fit.iter().flatten()).copied())
            .filter(|v| v.is_finite())
            .collect();
        let (lo, hi) = all
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
                (l.min(*v), h.max(*v))
            });
        let rows = cells.len();
        let cols = cells.first().map_or(0, Vec::len);
        let (w, h) = (
            plot_area.width() / cols.max(1) as f32,
            plot_area.height() / rows.max(1) as f32,
        );
        for (r, row) in cells.iter().enumerate() {
            for (c, cell) in row.iter().enumerate() {
                let rect = Rect::from_min_size(
                    plot_area.min + vec2(c as f32 * w, r as f32 * h),
                    vec2(w, h),
                )
                .shrink(1.5);
                let Some(cell) = cell else {
                    continue;
                };
                let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
                let is_center = cell.ijk == center;
                let clicked = small_plot(&mut child, input, cell, (lo, hi), marker, (r, c));
                if is_center {
                    ui.painter().rect_stroke(
                        rect,
                        2.0,
                        Stroke::new(1.5, theme.accent),
                        egui::StrokeKind::Inside,
                    );
                }
                if clicked && !is_center {
                    events.move_to = if input.source_is_under {
                        Some(cell.ijk)
                    } else {
                        ras_to_ijk(
                            &input.under.ijk_to_ras,
                            input.under.dims,
                            ijk_to_ras(&input.source.ijk_to_ras, cell.ijk),
                        )
                    };
                }
            }
        }
    }

    // Footer.
    let foot = Rect::from_min_max(pos2(area.left(), plot_area.bottom()), area.max);
    let mut child = ui.new_child(UiBuilder::new().max_rect(foot));
    footer_text(&mut child, input, mid, center);
    ui.allocate_rect(area, egui::Sense::hover());
    events
}

fn message(ui: &mut Ui, theme: &Theme, text: &str) {
    ui.centered_and_justified(|ui| {
        ui.label(RichText::new(text).color(theme.text_faint));
    });
}

/// Shade the stimulus blocks between `lo` and `hi`.
fn stimulus(plot_ui: &mut egui_plot::PlotUi, input: &GraphInput, lo: f64, hi: f64) {
    let Some(stim) = &input.settings.stim else {
        return;
    };
    for (a, b) in series::stim_blocks(&stim.on) {
        let (x0, x1) = (a as f64 - 0.5, b as f64 - 0.5);
        plot_ui.polygon(
            Polygon::new(
                "stimulus",
                PlotPoints::new(vec![[x0, lo], [x1, lo], [x1, hi], [x0, hi]]),
            )
            .fill_color(ink(input.theme).stimulus)
            .stroke(Stroke::NONE),
        );
    }
}

fn range(cell: &Cell) -> (f64, f64) {
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    for v in cell.values.iter().chain(cell.fit.iter().flatten()) {
        if v.is_finite() {
            lo = lo.min(*v);
            hi = hi.max(*v);
        }
    }
    if lo > hi {
        (0.0, 1.0)
    } else if lo == hi {
        (lo - 1.0, hi + 1.0)
    } else {
        let pad = (hi - lo) * 0.06;
        (lo - pad, hi + pad)
    }
}

fn big_plot(
    ui: &mut Ui,
    input: &GraphInput,
    cell: &Cell,
    marker: Option<f64>,
    events: &mut GraphEvents,
) {
    let theme = input.theme;
    let (lo, hi) = range(cell);
    let last = (cell.first + cell.values.len()).saturating_sub(1) as f64;
    let response = Plot::new("graph_main")
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_grid(Vec2b::new(false, true))
        .show_x(false)
        .show_y(false)
        .include_y(lo)
        .include_y(hi)
        .include_x(cell.first as f64 - 0.5)
        .include_x(last + 0.5)
        .set_margin_fraction(egui::vec2(0.0, 0.0))
        .height(ui.available_height())
        .show(ui, |plot_ui| {
            stimulus(plot_ui, input, lo, hi);
            plot_ui.line(
                Line::new("series", points(cell.first, &cell.values))
                    .color(ink(input.theme).data)
                    .width(1.6),
            );
            if let Some(fit) = &cell.fit {
                plot_ui.line(
                    Line::new("fit", points(cell.first, fit))
                        .color(ink(input.theme).fit)
                        .width(1.8),
                );
            }
            if let Some(tr) = marker {
                plot_ui.vline(
                    VLine::new("TR", tr)
                        .color(ink(input.theme).marker)
                        .width(1.2),
                );
            }
            let r = plot_ui.response();
            if (r.clicked() || r.dragged())
                && let Some(p) = plot_ui.pointer_coordinate()
            {
                Some(p.x)
            } else {
                None
            }
        });
    if input.source_is_under
        && let Some(x) = response.inner
    {
        let last_tr = input.source.nvols - 1;
        events.set_tr = Some((x.round().max(0.0) as usize).min(last_tr));
    }
    let _ = theme;
}

/// A graph of the matrix. Returns whether it was clicked.
fn small_plot(
    ui: &mut Ui,
    input: &GraphInput,
    cell: &Cell,
    (lo, hi): (f64, f64),
    marker: Option<f64>,
    (r, c): (usize, usize),
) -> bool {
    let (lo, hi) = if lo.is_finite() && hi > lo {
        let pad = (hi - lo) * 0.06;
        (lo - pad, hi + pad)
    } else {
        range(cell)
    };
    let last = (cell.first + cell.values.len()).saturating_sub(1) as f64;
    let response = Plot::new(("graph_cell", r, c))
        .allow_zoom(false)
        .allow_drag(false)
        .allow_scroll(false)
        .allow_boxed_zoom(false)
        .allow_double_click_reset(false)
        .show_axes(false)
        .show_grid(false)
        .show_x(false)
        .show_y(false)
        .include_y(lo)
        .include_y(hi)
        .include_x(cell.first as f64 - 0.5)
        .include_x(last + 0.5)
        .set_margin_fraction(egui::vec2(0.0, 0.0))
        .show(ui, |plot_ui| {
            stimulus(plot_ui, input, lo, hi);
            plot_ui.line(
                Line::new("series", points(cell.first, &cell.values))
                    .color(ink(input.theme).data)
                    .width(1.0),
            );
            if let Some(fit) = &cell.fit {
                plot_ui.line(
                    Line::new("fit", points(cell.first, fit))
                        .color(ink(input.theme).fit)
                        .width(1.2),
                );
            }
            if let Some(tr) = marker {
                plot_ui.vline(
                    VLine::new("TR", tr)
                        .color(ink(input.theme).marker)
                        .width(1.0),
                );
            }
        });
    response.response.clicked()
}

fn footer_text(ui: &mut Ui, input: &GraphInput, cell: &Cell, center: [usize; 3]) {
    let theme = input.theme;
    let [i, j, k] = center;
    let mut head = format!("{} [{i}, {j}, {k}]", input.source.name);
    if input.fit.is_some() {
        head.push_str(" · fit");
    }
    let mut parts = Vec::new();
    if let Some(Stats { mean, sd, min, max }) = series::stats(&cell.values) {
        parts.push(format!("mean {}", format_value(mean as f32)));
        parts.push(format!("sd {}", format_value(sd as f32)));
        parts.push(format!(
            "range {} … {}",
            format_value(min as f32),
            format_value(max as f32)
        ));
    }
    if input.source_is_under
        && let Some(v) = cell
            .values
            .get(input.current_tr.saturating_sub(cell.first))
            .filter(|_| input.current_tr >= cell.first)
    {
        parts.push(format!(
            "TR {} = {}",
            input.current_tr,
            format_value(*v as f32)
        ));
    }
    if let Some(r2) = cell
        .fit
        .as_ref()
        .and_then(|f| series::r_squared(&cell.values, f))
    {
        parts.push(format!("R² {r2:.2}"));
    }
    if let Some(tr) = input.source.tr {
        parts.push(format!("TR {tr} s"));
    }
    ui.label(RichText::new(head).small().color(theme.text_dim));
    ui.label(
        RichText::new(parts.join("  ·  "))
            .small()
            .monospace()
            .color(theme.text),
    );
}
