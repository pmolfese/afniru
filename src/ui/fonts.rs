//! Fonts: egui's defaults plus the Phosphor icon font (regular weight), so
//! icons can be used in any label as `egui_phosphor::regular::NAME`.

use egui::{Context, FontDefinitions, Id};

/// Install the icon font into `ctx` once (idempotent, cheap to call every
/// frame). The font takes effect from the next frame.
pub fn ensure(ctx: &Context) {
    let flag = Id::new("afniru_fonts_installed");
    if ctx.data(|d| d.get_temp::<bool>(flag)).unwrap_or(false) {
        return;
    }
    let mut fonts = FontDefinitions::default();
    egui_phosphor::add_to_fonts(&mut fonts, egui_phosphor::Variant::Regular);
    ctx.set_fonts(fonts);
    ctx.data_mut(|d| d.insert_temp(flag, true));
}
