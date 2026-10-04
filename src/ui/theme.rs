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
    /// The "classic" look: AFNI's black and orange, squarer and a bit larger.
    pub classic: bool,
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
    #[expect(dead_code, reason = "tool shelf tiles and card chrome (M3)")]
    pub accent_dim: Color32,
    /// Error text.
    pub error: Color32,
    /// Health: all good.
    pub good: Color32,
    /// Health: a caution.
    pub caution: Color32,
    /// Health: failed.
    pub danger: Color32,
    /// Health: unknown or not assessed.
    pub unknown: Color32,
    /// Selection (the selected processing step); independent of health.
    pub select: Color32,
    /// Behind the slice image.
    pub canvas: Color32,
}

impl Theme {
    /// The dark theme.
    pub fn dark() -> Self {
        Self {
            dark: true,
            classic: false,
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
            good: Color32::from_rgb(63, 185, 120),
            caution: Color32::from_rgb(240, 180, 50),
            danger: Color32::from_rgb(240, 90, 80),
            unknown: Color32::from_rgb(120, 128, 140),
            select: Color32::from_rgb(86, 156, 255),
            canvas: Color32::BLACK,
        }
    }

    /// The classic theme, a nod to the original AFNI: near-black panels, white
    /// text, and AFNI's orange for everything that is selected or active.
    pub fn classic() -> Self {
        Self {
            dark: true,
            classic: true,
            bg: Color32::from_rgb(8, 10, 16),
            panel: Color32::from_rgb(17, 19, 27),
            card: Color32::from_rgb(11, 13, 20),
            card_hi: Color32::from_rgb(30, 33, 44),
            border: Color32::from_rgb(58, 62, 76),
            text: Color32::from_rgb(240, 242, 246),
            text_dim: Color32::from_rgb(176, 182, 194),
            text_faint: Color32::from_rgb(116, 122, 136),
            accent: Color32::from_rgb(255, 176, 0),
            accent_dim: Color32::from_rgb(96, 66, 0),
            error: Color32::from_rgb(255, 110, 100),
            good: Color32::from_rgb(70, 200, 120),
            caution: Color32::from_rgb(255, 176, 0),
            danger: Color32::from_rgb(240, 80, 70),
            unknown: Color32::from_rgb(126, 132, 146),
            select: Color32::from_rgb(255, 176, 0),
            canvas: Color32::BLACK,
        }
    }

    /// The light theme.
    pub fn light() -> Self {
        Self {
            dark: false,
            classic: false,
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
            good: Color32::from_rgb(30, 150, 85),
            caution: Color32::from_rgb(200, 130, 0),
            danger: Color32::from_rgb(210, 50, 45),
            unknown: Color32::from_rgb(130, 138, 150),
            select: Color32::from_rgb(40, 110, 230),
            canvas: Color32::BLACK,
        }
    }

    /// Pick the theme for this frame: the preference, or the OS appearance.
    pub fn resolve(prefs: &Prefs, system_dark: bool) -> Self {
        let mut t = match prefs.theme {
            ThemeChoice::Classic => Self::classic(),
            ThemeChoice::Dark => Self::dark(),
            ThemeChoice::Light => Self::light(),
            ThemeChoice::System if system_dark => Self::dark(),
            ThemeChoice::System => Self::light(),
        };
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
        let r = CornerRadius::same(if self.classic { 2 } else { 5 });
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
            let body = if self.classic { 14.0 } else { 13.0 };
            s.text_styles
                .insert(egui::TextStyle::Body, FontId::proportional(body));
            s.text_styles
                .insert(egui::TextStyle::Button, FontId::proportional(body));
            s.text_styles
                .insert(egui::TextStyle::Small, FontId::proportional(11.0));
            s.text_styles
                .insert(egui::TextStyle::Monospace, FontId::monospace(12.0));
        });
    }
}
