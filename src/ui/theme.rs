//! Dark and light color tokens, layered over egui's stock visuals.
//!
//! Values come from the mockups (`mockups/ui_mockup/src/ui.rs`). The image
//! canvas is separate from the chrome: it is black in both themes unless the
//! user asks for white (publication figures).

use egui::{Color32, CornerRadius, FontId, Stroke, vec2};

use crate::prefs::{CanvasBackground, Prefs, ThemeChoice};

/// Axial plane color (sliders, plane dots, crosshair lines).
pub const AXIAL: Color32 = Color32::from_rgb(86, 156, 255);
/// Coronal plane color.
pub const CORONAL: Color32 = Color32::from_rgb(64, 201, 140);
/// Sagittal plane color.
pub const SAGITTAL: Color32 = Color32::from_rgb(242, 140, 72);

/// A resolved set of colors for one theme.
#[derive(Debug, Clone, Copy)]
pub struct Theme {
    /// True for the dark variant.
    pub dark: bool,
    /// Window background behind cards.
    pub bg: Color32,
    /// Controller sidebar.
    pub panel: Color32,
    /// View cards and sections.
    pub card: Color32,
    /// Inputs inside cards.
    pub card_hi: Color32,
    /// Hairlines and outlines.
    pub border: Color32,
    /// Primary text.
    pub text: Color32,
    /// Secondary text.
    pub text_dim: Color32,
    /// Tertiary text.
    pub text_faint: Color32,
    /// AFNI gold.
    pub accent: Color32,
    /// Muted accent for fills.
    pub accent_dim: Color32,
    /// Error text.
    pub error: Color32,
    /// Behind the slice image.
    pub canvas: Color32,
}

impl Theme {
    /// The dark theme.
    pub fn dark() -> Self {
        Self {
            dark: true,
            bg: Color32::from_rgb(17, 19, 23),
            panel: Color32::from_rgb(24, 27, 32),
            card: Color32::from_rgb(29, 32, 38),
            card_hi: Color32::from_rgb(38, 42, 50),
            border: Color32::from_rgb(46, 51, 60),
            text: Color32::from_rgb(226, 230, 236),
            text_dim: Color32::from_rgb(150, 158, 170),
            text_faint: Color32::from_rgb(98, 106, 118),
            accent: Color32::from_rgb(245, 196, 66),
            accent_dim: Color32::from_rgb(92, 76, 34),
            error: Color32::from_rgb(255, 120, 110),
            canvas: Color32::BLACK,
        }
    }

    /// The light theme.
    pub fn light() -> Self {
        Self {
            dark: false,
            bg: Color32::from_rgb(232, 235, 240),
            panel: Color32::from_rgb(246, 247, 249),
            card: Color32::from_rgb(255, 255, 255),
            card_hi: Color32::from_rgb(238, 241, 245),
            border: Color32::from_rgb(214, 219, 227),
            text: Color32::from_rgb(28, 32, 40),
            text_dim: Color32::from_rgb(92, 100, 114),
            text_faint: Color32::from_rgb(146, 153, 165),
            accent: Color32::from_rgb(214, 152, 10),
            accent_dim: Color32::from_rgb(252, 238, 200),
            error: Color32::from_rgb(190, 40, 30),
            canvas: Color32::BLACK,
        }
    }

    /// Pick the theme for this frame: the preference, or the OS appearance.
    pub fn resolve(prefs: &Prefs, system_dark: bool) -> Self {
        let dark = match prefs.theme {
            ThemeChoice::Dark => true,
            ThemeChoice::Light => false,
            ThemeChoice::System => system_dark,
        };
        let mut t = if dark { Self::dark() } else { Self::light() };
        t.canvas = match prefs.canvas {
            CanvasBackground::Black => Color32::BLACK,
            CanvasBackground::White => Color32::WHITE,
        };
        t
    }

    /// Install these colors, spacing and text sizes into the egui context.
    pub fn apply(&self, ctx: &egui::Context) {
        let mut v = if self.dark {
            egui::Visuals::dark()
        } else {
            egui::Visuals::light()
        };
        v.panel_fill = self.panel;
        v.window_fill = self.card;
        v.extreme_bg_color = self.card_hi;
        v.faint_bg_color = self.card_hi;
        v.override_text_color = Some(self.text);
        v.selection.bg_fill = self.accent;
        v.selection.stroke = Stroke::new(1.0, self.accent);
        v.slider_trailing_fill = true;
        let r = CornerRadius::same(5);
        for w in [
            &mut v.widgets.noninteractive,
            &mut v.widgets.inactive,
            &mut v.widgets.hovered,
            &mut v.widgets.active,
            &mut v.widgets.open,
        ] {
            w.corner_radius = r;
        }
        v.widgets.noninteractive.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.inactive.bg_fill = self.card_hi;
        v.widgets.inactive.weak_bg_fill = self.card_hi;
        v.widgets.inactive.bg_stroke = Stroke::new(1.0, self.border);
        v.widgets.inactive.fg_stroke = Stroke::new(1.0, self.text);
        v.widgets.hovered.weak_bg_fill = self.card_hi;
        ctx.set_visuals(v);
        ctx.global_style_mut(|s| {
            s.spacing.item_spacing = vec2(6.0, 6.0);
            s.spacing.button_padding = vec2(8.0, 3.0);
            s.spacing.interact_size.y = 22.0;
            s.text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(13.0));
            s.text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(13.0));
            s.text_styles
                .insert(egui::TextStyle::Small, FontId::proportional(11.0));
            s.text_styles
                .insert(egui::TextStyle::Monospace, FontId::monospace(12.0));
        });
    }
}
