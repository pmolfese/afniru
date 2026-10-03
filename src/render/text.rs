//! Smooth text for saved images: slice numbers and orientation letters,
//! rasterized from the monospace font egui already carries, with
//! anti-aliased edges and an optional outline, at any pixel height.

use std::sync::OnceLock;

use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};

use super::export::Rgba8Image;

/// The font: egui's default monospace face.
fn font() -> Option<&'static FontArc> {
    static FONT: OnceLock<Option<FontArc>> = OnceLock::new();
    FONT.get_or_init(|| {
        let defs = egui::FontDefinitions::default();
        let data = defs.font_data.get("Hack")?;
        FontArc::try_from_vec(data.font.to_vec()).ok()
    })
    .as_ref()
}

/// The size `text` takes at `px` pixels high: width, height (ascent plus
/// descent, rounded up).
pub fn measure(text: &str, px: f32) -> (usize, usize) {
    let Some(font) = font() else {
        return (0, 0);
    };
    let scaled = font.as_scaled(PxScale::from(px));
    let width: f32 = text
        .chars()
        .map(|c| scaled.h_advance(font.glyph_id(c)))
        .sum();
    (
        width.ceil() as usize,
        (scaled.ascent() - scaled.descent()).ceil() as usize,
    )
}

/// Draw `text` with its top left corner at (`x`, `y`), `px` pixels high, in
/// `color`, blending its anti-aliased edges into the picture. With `outline`
/// the text gets a border of that color, `outline_px` pixels wide.
pub fn draw(
    img: &mut Rgba8Image,
    text: &str,
    (x, y): (i64, i64),
    px: f32,
    color: [u8; 3],
    outline: Option<([u8; 3], f32)>,
) {
    let Some(font) = font() else {
        return;
    };
    let scaled = font.as_scaled(PxScale::from(px));
    // Glyph coverage once, then painted at each offset of the outline.
    let mut glyphs = Vec::new();
    let mut pen = 0.0_f32;
    for c in text.chars() {
        let id = font.glyph_id(c);
        let glyph = id.with_scale_and_position(PxScale::from(px), point(pen, scaled.ascent()));
        pen += scaled.h_advance(id);
        if let Some(outlined) = font.outline_glyph(glyph) {
            let bounds = outlined.px_bounds();
            let mut coverage = Vec::new();
            outlined.draw(|gx, gy, c| coverage.push((gx as i64, gy as i64, c)));
            glyphs.push((
                bounds.min.x.floor() as i64,
                bounds.min.y.floor() as i64,
                coverage,
            ));
        }
    }
    let mut paint = |dx: i64, dy: i64, rgb: [u8; 3]| {
        for (ox, oy, coverage) in &glyphs {
            for &(gx, gy, c) in coverage {
                img.blend(x + ox + gx + dx, y + oy + gy + dy, rgb, c);
            }
        }
    };
    if let Some((rgb, width)) = outline {
        let r = width.max(0.0);
        let steps = (r.ceil() as i64).max(1);
        // A ring of offsets, so the border is round and the same all around.
        for dy in -steps..=steps {
            for dx in -steps..=steps {
                if ((dx * dx + dy * dy) as f32).sqrt() <= r + 0.25 && (dx, dy) != (0, 0) {
                    paint(dx, dy, rgb);
                }
            }
        }
    }
    paint(0, 0, color);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_has_a_size_that_grows_with_the_height_and_the_length() {
        let (w1, h1) = measure("7", 20.0);
        let (w2, _) = measure("77", 20.0);
        let (w3, h3) = measure("7", 40.0);
        assert!(w1 > 5 && h1 > 10);
        assert!((w2 as i64 - 2 * w1 as i64).abs() <= 1);
        assert!(w3 > w1 && h3 > h1);
    }

    #[test]
    fn drawing_blends_smoothly_instead_of_leaving_blocks() {
        let mut img = Rgba8Image::filled(60, 40, [0, 0, 0]);
        draw(&mut img, "96", (4, 4), 30.0, [255, 255, 255], None);
        // Pure white where a glyph covers a pixel fully, pure black far away, and
        // many in-between grays on the edges: anti-aliasing.
        let mut grays = std::collections::HashSet::new();
        let (mut white, mut black) = (0, 0);
        for y in 0..40 {
            for x in 0..60 {
                let p = img.get(x, y)[0];
                match p {
                    255 => white += 1,
                    0 => black += 1,
                    g => {
                        grays.insert(g);
                    }
                }
            }
        }
        assert!(white > 20 && black > 500);
        assert!(grays.len() > 20, "{} distinct grays", grays.len());
    }

    #[test]
    fn an_outline_surrounds_the_text_with_its_own_color() {
        let mut img = Rgba8Image::filled(60, 40, [128, 128, 128]);
        draw(
            &mut img,
            "8",
            (10, 6),
            28.0,
            [255, 255, 255],
            Some(([0, 0, 0], 2.0)),
        );
        let dark = (0..40)
            .flat_map(|y| (0..60).map(move |x| (x, y)))
            .filter(|&(x, y)| img.get(x, y)[0] < 40)
            .count();
        let bright = (0..40)
            .flat_map(|y| (0..60).map(move |x| (x, y)))
            .filter(|&(x, y)| img.get(x, y)[0] > 230)
            .count();
        assert!(dark > 30 && bright > 30);
    }

    #[test]
    fn drawing_outside_the_picture_is_clipped() {
        let mut img = Rgba8Image::filled(10, 10, [0, 0, 0]);
        draw(&mut img, "123456", (-30, 5), 24.0, [255, 255, 255], None);
        draw(&mut img, "1", (50, 50), 24.0, [255, 255, 255], None);
        assert_eq!(img.rgba.len(), 400);
    }
}
