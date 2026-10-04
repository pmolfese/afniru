//! Define Overlay tool: the color bar with a threshold slider, the color
//! scale, ± or positive-only, AFNI's A (fade) and B (boxed) buttons, the
//! range, the threshold with its p-value and FDR q-value, and opacity. One
//! card per overlay layer, top layer first, titled "Overlay N · dataset".

use afni_core::afni_colors::AfniColorScale;
use afni_core::calc::Expr;
use egui::{ComboBox, DragValue, RichText, Slider, TextEdit, Ui};
use egui_phosphor::regular as icon;

use super::datasets::{Picks, dataset_combo, dataset_menu, sub_brick_combo};
use super::{Instance, Tool, ToolContext, ToolId};
use crate::render::overlay::range_top;
use crate::session::overlay::{Binding, Coord, MaskRule, OverlayChange};
use crate::session::{Action, LayerId, OverlayLayer, graph};
use crate::ui::widgets::chips::attach_chip;
use crate::ui::widgets::pbar::{self, Pbar};
use crate::ui::widgets::readout::format_p;

/// The Define Overlay tool.
pub struct OverlayTool;

impl Tool for OverlayTool {
    fn instances(&self, cx: &ToolContext) -> Vec<Instance> {
        if cx.overlays.is_empty() {
            return vec![Instance::single()];
        }
        // Top layer first, as in the layer list.
        cx.overlays
            .iter()
            .rev()
            .map(|o| Instance {
                id: o.layer.id.0,
                title: Some(format!("Overlay {} · {}", o.layer.id.0, o.dataset.name)),
            })
            .collect()
    }

    fn card_ui(&self, ui: &mut Ui, cx: &ToolContext, instance: &Instance) -> Vec<Action> {
        let mut actions = Vec::new();
        let Some(o) = cx.overlay(LayerId(instance.id)) else {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Dataset").color(cx.theme.text_dim));
                dataset_combo(
                    ui,
                    cx,
                    "olay_ds_none",
                    ui.available_width(),
                    "no overlay: choose a dataset",
                    None,
                    &Picks::new_overlay(),
                    &mut actions,
                );
            });
            return actions;
        };
        let layer = o.layer;
        let change = |c: OverlayChange| Action::Layer(layer.id, c);
        self.pickers(ui, cx, o, &mut actions);
        let theme = cx.theme;
        let auto_range = o.frames.map_or(0.0, |f| f.auto_range);
        let thr_max = o.frames.map_or(0.0, |f| f.thr_max);
        let top = range_top(layer, auto_range);
        let shared_scale = layer.olay_sub == layer.thr_sub;

        // Color map or on/off mask.
        ui.horizontal(|ui| {
            ui.label(RichText::new("Show as").color(theme.text_dim));
            if ui.selectable_label(!layer.as_mask, "Color map").clicked() {
                actions.push(change(OverlayChange::MaskMode(false)));
            }
            if ui
                .selectable_label(layer.as_mask, "Mask")
                .on_hover_text("On/off: every voxel that passes the rule gets the same color")
                .clicked()
            {
                actions.push(change(OverlayChange::MaskMode(true)));
            }
        });
        ui.add_space(2.0);

        if layer.as_mask {
            self.mask_section(ui, cx, o, &mut actions);
        }
        let by_threshold = !layer.as_mask || layer.mask.rule == MaskRule::Threshold;
        if !layer.as_mask {
            ui.horizontal_top(|ui| {
                // The bar, with the threshold slider beside it.
                if let Ok(map) = layer.colorscale.to_color_map(256) {
                    pbar::pbar(
                        ui,
                        theme,
                        &Pbar {
                            colormap: &map,
                            signed: layer.signed,
                            top,
                            threshold: shared_scale.then_some(layer.threshold),
                        },
                    );
                }
                let mut t = layer.threshold;
                let slider_top = if thr_max > 0.0 { thr_max } else { top };
                ui.spacing_mut().slider_width = pbar::HEIGHT;
                if ui
                    .add(
                        Slider::new(&mut t, 0.0..=slider_top)
                            .vertical()
                            .show_value(false),
                    )
                    .on_hover_text("Threshold")
                    .changed()
                {
                    actions.push(change(OverlayChange::Threshold(t)));
                }

                ui.vertical(|ui| {
                    ui.set_min_width(130.0);
                    ComboBox::from_id_salt("colorscale")
                        .width(ui.available_width())
                        .selected_text(layer.colorscale.name())
                        .show_ui(ui, |ui| {
                            for s in AfniColorScale::ALL_NAMED {
                                if ui
                                    .selectable_label(layer.colorscale == s, s.name())
                                    .clicked()
                                {
                                    actions.push(change(OverlayChange::ColorScale(s)));
                                }
                            }
                        });
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(layer.signed, "±")
                            .on_hover_text("Color positive and negative values")
                            .clicked()
                        {
                            actions.push(change(OverlayChange::Signed(true)));
                        }
                        if ui
                            .selectable_label(!layer.signed, "+")
                            .on_hover_text("Color positive values only")
                            .clicked()
                        {
                            actions.push(change(OverlayChange::Signed(false)));
                        }
                        ui.add_space(4.0);
                        let mut fade = layer.fade;
                        if ui
                            .toggle_value(&mut fade, "A")
                            .on_hover_text("Alpha: fade values below the threshold")
                            .changed()
                        {
                            actions.push(change(OverlayChange::Fade(fade)));
                        }
                        let mut boxed = layer.boxed;
                        if ui
                            .toggle_value(&mut boxed, "B")
                            .on_hover_text("Boxed: add an outline around the suprathreshold regions (they stay filled)")
                            .changed()
                        {
                            actions.push(change(OverlayChange::Boxed(boxed)));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Range").color(theme.text_dim));
                        let mut r = top;
                        let manual = layer.range.is_some();
                        if manual {
                            if ui
                                .add(
                                    DragValue::new(&mut r)
                                        .speed(top / 100.0)
                                        .range(1e-9..=f64::MAX)
                                        .max_decimals(3),
                                )
                                .changed()
                            {
                                actions.push(change(OverlayChange::Range(Some(r))));
                            }
                        } else {
                            ui.label(
                                RichText::new(crate::ui::widgets::readout::format_value(
                                    top as f32,
                                ))
                                .monospace(),
                            );
                        }
                        let mut auto = !manual;
                        if ui.checkbox(&mut auto, "auto").changed() {
                            actions.push(change(OverlayChange::Range(if auto {
                                None
                            } else {
                                Some(top)
                            })));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Opacity").color(theme.text_dim));
                        let mut op = layer.opacity;
                        ui.spacing_mut().slider_width = 70.0;
                        if ui
                            .add(Slider::new(&mut op, 0.0..=1.0).show_value(false))
                            .changed()
                        {
                            actions.push(change(OverlayChange::Opacity(op)));
                        }
                    });
                });
            });
        }
        if by_threshold {
            ui.add_space(4.0);
            let stat = o.dataset.stats.get(layer.thr_sub).and_then(Option::as_ref);
            ui.label(
                RichText::new(threshold_caption(layer, stat.map(|s| s.to_statsym())))
                    .small()
                    .color(theme.text_dim),
            );
            ui.horizontal(|ui| {
                let mut t = layer.threshold;
                if ui
                    .add(
                        DragValue::new(&mut t)
                            .speed((thr_max.max(top) / 200.0).max(1e-6))
                            .range(0.0..=f64::MAX)
                            .max_decimals(4),
                    )
                    .changed()
                {
                    actions.push(change(OverlayChange::Threshold(t)));
                }
                let mut visible = layer.visible;
                if ui.checkbox(&mut visible, "show").changed() {
                    actions.push(change(OverlayChange::Visible(visible)));
                }
            });
            let p = layer.p_value(o.dataset);
            let q = layer.q_value(o.dataset);
            ui.horizontal(|ui| {
                ui.label(RichText::new("p =").color(theme.text_dim).monospace());
                match p {
                    Some(pv) => {
                        if let Some(v) = p_box(ui, pv) {
                            actions.push(change(OverlayChange::ThresholdByP(v)));
                        }
                    }
                    None => {
                        ui.label(RichText::new("--").monospace());
                    }
                }
                ui.label(RichText::new("q =").color(theme.text_dim).monospace());
                ui.label(RichText::new(q.map_or("--".to_string(), format_p)).monospace());
            });
            if let Some(spec) = stat {
                let sided = match layer.tail(spec) {
                    Some(afni_core::stats::Tail::TwoSided) => "2-sided",
                    _ => "1-sided",
                };
                ui.label(
                    RichText::new(format!("{}  ·  {sided}", spec.to_statsym()))
                        .small()
                        .color(theme.text_dim),
                );
            } else {
                ui.label(
                    RichText::new("not a statistic: no p or q")
                        .small()
                        .color(theme.text_faint),
                );
            }
        } else {
            // A rule has no threshold: just visibility and opacity.
            ui.horizontal(|ui| {
                let mut visible = layer.visible;
                if ui.checkbox(&mut visible, "show").changed() {
                    actions.push(change(OverlayChange::Visible(visible)));
                }
                ui.label(RichText::new("Opacity").color(theme.text_dim));
                let mut op = layer.opacity;
                ui.spacing_mut().slider_width = 70.0;
                if ui
                    .add(Slider::new(&mut op, 0.0..=1.0).show_value(false))
                    .changed()
                {
                    actions.push(change(OverlayChange::Opacity(op)));
                }
            });
        }
        self.attach_row(ui, cx, layer, &mut actions);
        actions
    }

    fn summary(&self, cx: &ToolContext, instance: &Instance) -> String {
        match cx.overlay(LayerId(instance.id)) {
            Some(o) => {
                let l = o.layer;
                let hidden = if l.visible { "" } else { " (hidden)" };
                if l.as_mask {
                    let rule = match &l.mask.rule {
                        MaskRule::Threshold => format!(
                            "{} ≥ {}",
                            if l.signed { "|thr|" } else { "thr" },
                            crate::ui::widgets::readout::format_value(l.threshold as f32)
                        ),
                        MaskRule::Expression(t) => t.clone(),
                    };
                    return format!("mask: {rule}{hidden}");
                }
                let sign = if l.signed { "|" } else { "" };
                format!(
                    "{}{}{} ≥ {}{}",
                    sign,
                    o.dataset.name,
                    sign,
                    crate::ui::widgets::readout::format_value(l.threshold as f32),
                    hidden
                )
            }
            None => "none".into(),
        }
    }
}

impl OverlayTool {
    /// The footer: a chip for each tool that can hook under this card. A solid
    /// gold chip is hooked (click to unhook); a dashed one is available.
    fn attach_row(
        &self,
        ui: &mut Ui,
        cx: &ToolContext,
        layer: &OverlayLayer,
        actions: &mut Vec<Action>,
    ) {
        let hookable: Vec<_> = graph::children(ToolId::Overlay)
            .into_iter()
            .filter_map(|id| Some((id, id.tool()?)))
            .collect();
        if hookable.is_empty() {
            return;
        }
        ui.add_space(6.0);
        ui.horizontal(|ui| {
            ui.label(RichText::new("ATTACH").small().color(cx.theme.text_faint));
            for (id, tool) in hookable {
                let hooked = tool.is_hooked(cx, layer.id.0);
                let chip = attach_chip(
                    ui,
                    cx.theme,
                    &format!("{} {}", id.icon(), id.label()),
                    hooked,
                )
                .on_hover_text(if hooked {
                    format!("Detach {} from this layer", id.title())
                } else {
                    format!("Attach {} under this layer", id.title())
                });
                if chip.clicked()
                    && let Some(action) = tool.hook_action(layer.id.0, !hooked)
                {
                    actions.push(action);
                }
            }
        });
    }

    /// Controls of a mask layer: its color, its rule, and what the rule's
    /// letters stand for.
    fn mask_section(
        &self,
        ui: &mut Ui,
        cx: &ToolContext,
        o: &super::OverlayContext,
        actions: &mut Vec<Action>,
    ) {
        let (layer, theme) = (o.layer, cx.theme);
        let change = |c: OverlayChange| Action::Layer(layer.id, c);
        ui.horizontal(|ui| {
            let mut rgb = layer.mask.color;
            if ui.color_edit_button_srgb(&mut rgb).changed() {
                actions.push(change(OverlayChange::MaskColor(rgb)));
            }
            ui.label(RichText::new("is on where").color(theme.text_dim));
            let by_rule = matches!(layer.mask.rule, MaskRule::Expression(_));
            if ui.selectable_label(!by_rule, "threshold").clicked() {
                actions.push(change(OverlayChange::MaskRule(MaskRule::Threshold)));
            }
            if ui
                .selectable_label(by_rule, "rule")
                .on_hover_text("A 3dcalc expression; on where it is not zero")
                .clicked()
            {
                let text = match &layer.mask.rule {
                    MaskRule::Expression(t) => t.clone(),
                    MaskRule::Threshold => format!("step(a-{})", layer.threshold),
                };
                actions.push(change(OverlayChange::MaskRule(MaskRule::Expression(text))));
            }
        });
        let MaskRule::Expression(text) = &layer.mask.rule else {
            return;
        };

        let mut edited = text.clone();
        ui.add(
            TextEdit::singleline(&mut edited)
                .font(egui::TextStyle::Monospace)
                .hint_text("a>3 && b>2   or   step(a-3)*step(b-2)")
                .desired_width(f32::INFINITY),
        );
        if &edited != text {
            actions.push(change(OverlayChange::MaskRule(MaskRule::Expression(
                edited.clone(),
            ))));
        }
        match Expr::parse(&edited) {
            Err(e) => {
                let msg = match e {
                    afni_core::Error::InvalidParameter { reason, .. } => reason,
                    other => other.to_string(),
                };
                ui.label(RichText::new(msg).small().color(theme.error));
            }
            Ok(expr) => {
                if let Some(problem) = &o.problem {
                    ui.label(RichText::new(problem).small().color(theme.error));
                }
                self.bindings(ui, cx, o, &expr, actions);
            }
        }
        ui.label(
            RichText::new("3dcalc language, plus a>3, a<=b, &&, ||, !, c ? x : y")
                .small()
                .color(theme.text_faint),
        );
        ui.add_space(2.0);
    }

    /// One row per letter the rule uses: what it stands for.
    fn bindings(
        &self,
        ui: &mut Ui,
        cx: &ToolContext,
        o: &super::OverlayContext,
        expr: &Expr,
        actions: &mut Vec<Action>,
    ) {
        let (layer, theme) = (o.layer, cx.theme);
        egui::Grid::new(("bindings", layer.id.0))
            .num_columns(2)
            .show(ui, |ui| {
                for letter in expr.variables() {
                    ui.label(
                        RichText::new(letter.to_string())
                            .monospace()
                            .color(theme.accent),
                    );
                    let current = layer.binding_for(letter);
                    let name = current.map_or("choose…".to_string(), |b| binding_name(cx, b));
                    let color = if current.is_some() {
                        theme.text
                    } else {
                        theme.error
                    };
                    let chosen = std::cell::Cell::new(None);
                    ui.menu_button(RichText::new(name).color(color), |ui| {
                        let pick = |ui: &mut Ui, label: &str, b: Binding| {
                            if ui.button(label).clicked() {
                                chosen.set(Some(b));
                                ui.close();
                            }
                        };
                        pick(ui, "this layer's OLay", Binding::Olay);
                        pick(ui, "this layer's Thr", Binding::Thr);
                        ui.separator();
                        for other in cx.overlays.iter().filter(|x| x.layer.id != layer.id) {
                            let n = other.layer.id.0;
                            pick(
                                ui,
                                &format!("Overlay {n}: where it is drawn (0/1)"),
                                Binding::LayerMask(other.layer.id),
                            );
                            pick(
                                ui,
                                &format!("Overlay {n}: its OLay value"),
                                Binding::LayerValue(other.layer.id),
                            );
                        }
                        ui.separator();
                        for (id, d) in cx.session.store.iter() {
                            ui.menu_button(&d.name, |ui| {
                                for t in 0..d.nvols {
                                    let label = super::datasets::sub_brick_text(d, t);
                                    pick(
                                        ui,
                                        &label,
                                        Binding::Sub {
                                            dataset: id,
                                            sub: t,
                                        },
                                    );
                                }
                            });
                        }
                        ui.separator();
                        for c in [Coord::X, Coord::Y, Coord::Z, Coord::I, Coord::J, Coord::K] {
                            pick(ui, &coord_name(c), Binding::Coord(c));
                        }
                    });
                    if let Some(b) = chosen.get() {
                        actions.push(Action::Layer(
                            layer.id,
                            OverlayChange::Bind(letter, Some(b)),
                        ));
                    }
                    ui.end_row();
                }
            });
    }

    /// Dataset and sub-brick pickers at the top of a layer's card.
    fn pickers(
        &self,
        ui: &mut Ui,
        cx: &ToolContext,
        o: &super::OverlayContext,
        actions: &mut Vec<Action>,
    ) {
        let layer = o.layer;
        // Room for the label column and the + button, fixed so the grid settles.
        let dataset_width = (ui.available_width() - 100.0).max(80.0);
        egui::Grid::new(("overlay_pickers", layer.id.0))
            .num_columns(2)
            .show(ui, |ui| {
                ui.label(RichText::new("Dataset").color(cx.theme.text_dim));
                ui.horizontal(|ui| {
                    // The dataset of this layer, from those loaded or listed in a
                    // folder; and a + to add another overlay layer the same way.
                    dataset_combo(
                        ui,
                        cx,
                        ("olay_ds", layer.id.0),
                        dataset_width,
                        &o.dataset.name,
                        Some(layer.dataset),
                        &Picks::layer(layer.id),
                        actions,
                    );
                    dataset_menu(ui, cx, icon::PLUS, &Picks::new_overlay(), actions)
                        .on_hover_text("Add another overlay layer");
                });
                ui.end_row();
                for (label, current, is_olay) in [
                    ("OLay", layer.olay_sub, true),
                    ("Thr", layer.thr_sub, false),
                ] {
                    ui.label(RichText::new(label).color(cx.theme.text_dim));
                    if let Some(t) = sub_brick_combo(
                        ui,
                        (label, layer.id.0),
                        o.dataset,
                        current,
                        ui.available_width(),
                    ) {
                        let (olay, thr) = if is_olay {
                            (t, layer.thr_sub)
                        } else {
                            (layer.olay_sub, t)
                        };
                        actions.push(Action::Layer(
                            layer.id,
                            OverlayChange::SubBricks { olay, thr },
                        ));
                    }
                    ui.end_row();
                }
            });
        ui.add_space(4.0);
    }
}

/// What a binding is called in the interface.
fn binding_name(cx: &ToolContext, b: Binding) -> String {
    match b {
        Binding::Olay => "this layer's OLay".into(),
        Binding::Thr => "this layer's Thr".into(),
        Binding::Sub { dataset, sub } => match cx.session.store.get(dataset) {
            Some(d) => format!("{} {}", d.name, super::datasets::sub_brick_text(d, sub)),
            None => "missing dataset".into(),
        },
        Binding::LayerMask(l) => format!("Overlay {}: drawn (0/1)", l.0),
        Binding::LayerValue(l) => format!("Overlay {}: OLay value", l.0),
        Binding::Coord(c) => coord_name(c),
    }
}

/// `x: mm, left` and so on, as 3dcalc defines them.
fn coord_name(c: Coord) -> String {
    match c {
        Coord::X => "x (mm, DICOM)".into(),
        Coord::Y => "y (mm, DICOM)".into(),
        Coord::Z => "z (mm)".into(),
        Coord::I => "i (voxel index)".into(),
        Coord::J => "j (voxel index)".into(),
        Coord::K => "k (voxel index)".into(),
    }
}

/// `Threshold |t| ≥` / `Threshold t ≥`, from the statistic's symbol.
fn threshold_caption(layer: &OverlayLayer, statsym: Option<String>) -> String {
    let name = match statsym.as_deref().and_then(|s| s.split('(').next()) {
        Some("Ttest") => "t",
        Some("Zscore") => "z",
        Some("Ftest") => "F",
        Some("Correl") => "r",
        Some("Chisq") => "χ²",
        _ => "value",
    };
    if layer.signed {
        format!("Threshold |{name}| ≥")
    } else {
        format!("Threshold {name} ≥")
    }
}

/// Parse what the user typed in the p box; accepts `.05`, `0.05`, `5e-3`.
fn parse_p(text: &str) -> Option<f64> {
    let v = text.trim().parse::<f64>().ok()?;
    (v.is_finite() && v > 0.0 && v <= 1.0).then_some(v)
}

/// A p-value text box: shows the formatted value, edits as plain text, and
/// commits on Enter or when focus leaves.
fn p_box(ui: &mut egui::Ui, pv: f64) -> Option<f64> {
    let id = ui.id().with("p_box");
    let editing: Option<String> = ui.data(|d| d.get_temp(id));
    let mut text = editing.clone().unwrap_or_else(|| format_p(pv));
    let out = ui.add(
        egui::TextEdit::singleline(&mut text)
            .desired_width(64.0)
            .font(egui::TextStyle::Monospace),
    );
    out.clone()
        .on_hover_text("Type a p-value (e.g. .05) and press Enter");
    if out.gained_focus()
        && let Some(mut st) = egui::TextEdit::load_state(ui.ctx(), out.id)
    {
        st.cursor.set_char_range(Some(egui::text::CCursorRange::two(
            egui::text::CCursor::new(0),
            egui::text::CCursor::new(text.chars().count()),
        )));
        st.store(ui.ctx(), out.id);
    }
    if out.has_focus() {
        ui.data_mut(|d| d.insert_temp(id, text.clone()));
        if ui.input(|i| i.key_pressed(egui::Key::Enter)) {
            ui.data_mut(|d| d.remove_temp::<String>(id));
            return parse_p(&text);
        }
        None
    } else {
        let had = editing.is_some();
        ui.data_mut(|d| d.remove_temp::<String>(id));
        if had { parse_p(&text) } else { None }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_p_value_can_be_typed_without_a_leading_zero() {
        assert_eq!(super::parse_p(".05"), Some(0.05));
        assert_eq!(super::parse_p(" 5e-3 "), Some(0.005));
        assert_eq!(super::parse_p("0"), None);
        assert_eq!(super::parse_p("2"), None);
        assert_eq!(super::parse_p("abc"), None);
    }

    use afni_core::afni_colors::AfniColorScale;

    use super::*;
    use crate::session::store::DatasetId;

    #[test]
    fn caption_names_the_statistic_and_the_sign_mode() {
        let mut l = OverlayLayer::new(DatasetId(0), AfniColorScale::SpectrumRedToBlue);
        assert_eq!(
            threshold_caption(&l, Some("Ttest(118)".into())),
            "Threshold |t| ≥"
        );
        l.signed = false;
        assert_eq!(
            threshold_caption(&l, Some("Ftest(2,40)".into())),
            "Threshold F ≥"
        );
        assert_eq!(threshold_caption(&l, None), "Threshold value ≥");
    }
}
