//! The Graph as a picture, for saved images: the time series (and its fit)
//! over shaded stimulus blocks, with a frame, grid, tick labels and the current
//! time point, drawn with smooth lines on a plain background.

use super::export::{Rgba8Image, ink_for};
use super::text;

/// What to draw.
#[derive(Debug, Clone, Copy)]
pub struct GraphPicture<'a> {
    /// Time point of the first value.
    pub first: usize,
    /// The series.
    pub values: &'a [f64],
    /// The fit, same length.
    pub fit: Option<&'a [f64]>,
    /// Stimulus blocks as `[start, end)` time points and their color.
    pub stim: &'a [(usize, usize, [u8; 3])],
    /// The current time point, marked with a vertical line.
    pub marker: Option<usize>,
}

/// A "nice" step (1, 2 or 5 × 10ⁿ) giving about `target` steps across `span`.
pub fn nice_step(span: f64, target: usize) -> f64 {
    let raw = span / target.max(1) as f64;
    if !raw.is_finite() || raw <= 0.0 {
        return 1.0;
    }
    let mag = 10f64.powf(raw.log10().floor());
    [1.0, 2.0, 5.0, 10.0]
        .into_iter()
        .map(|m| m * mag)
        .find(|s| *s >= raw)
        .unwrap_or(mag * 10.0)
}

/// Tick values in `[lo, hi]`, multiples of `step`.
pub fn ticks(lo: f64, hi: f64, step: f64) -> Vec<f64> {
    let mut out = Vec::new();
    let mut v = (lo / step).ceil() * step;
    while v <= hi + step * 1e-9 && out.len() < 100 {
        out.push(v);
        v += step;
    }
    out
}

/// A label for a tick: no decimals when the step is whole.
fn tick_label(v: f64, step: f64) -> String {
    if step >= 1.0 {
        format!("{v:.0}")
    } else {
        let decimals = (-step.log10().floor()).max(0.0) as usize;
        format!("{v:.decimals$}")
    }
}

/// An anti-aliased line of `width` pixels.
fn line(
    img: &mut Rgba8Image,
    (x0, y0): (f64, f64),
    (x1, y1): (f64, f64),
    width: f64,
    rgb: [u8; 3],
) {
    let r = width / 2.0 + 1.0;
    let (min_x, max_x) = (x0.min(x1) - r, x0.max(x1) + r);
    let (min_y, max_y) = (y0.min(y1) - r, y0.max(y1) + r);
    let (dx, dy) = (x1 - x0, y1 - y0);
    let len2 = dx * dx + dy * dy;
    for py in (min_y.floor() as i64).max(0)..=(max_y.ceil() as i64).min(img.height as i64 - 1) {
        for px in (min_x.floor() as i64).max(0)..=(max_x.ceil() as i64).min(img.width as i64 - 1) {
            let (cx, cy) = (px as f64 + 0.5, py as f64 + 0.5);
            let t = if len2 > 0.0 {
                (((cx - x0) * dx + (cy - y0) * dy) / len2).clamp(0.0, 1.0)
            } else {
                0.0
            };
            let (nx, ny) = (x0 + t * dx, y0 + t * dy);
            let dist = ((cx - nx).powi(2) + (cy - ny).powi(2)).sqrt();
            let coverage = (width / 2.0 + 0.5 - dist).clamp(0.0, 1.0);
            img.blend(px, py, rgb, coverage as f32);
        }
    }
}

/// Draw the Graph in a `width × height` picture on `background`. `text_px` is
/// the height of the tick labels.
pub fn render(
    (width, height): (usize, usize),
    g: &GraphPicture,
    background: [u8; 3],
    text_px: f32,
) -> Rgba8Image {
    let mut img = Rgba8Image::filled(width, height, background);
    let ink = ink_for(background);
    let light = ink[0] < 100; // dark ink: a light background
    let n = g.values.len();
    if n == 0 {
        return img;
    }
    // Ranges, with a little air above and below.
    let finite = |v: &&f64| v.is_finite();
    let all = g.values.iter().chain(g.fit.into_iter().flatten());
    let (mut lo, mut hi) = all
        .filter(finite)
        .fold((f64::INFINITY, f64::NEG_INFINITY), |(l, h), v| {
            (l.min(*v), h.max(*v))
        });
    if lo > hi {
        (lo, hi) = (0.0, 1.0);
    }
    if (hi - lo).abs() < 1e-12 {
        (lo, hi) = (lo - 1.0, hi + 1.0);
    }
    let pad = (hi - lo) * 0.06;
    let (lo, hi) = (lo - pad, hi + pad);
    let last = g.first + n - 1;
    let (x_lo, x_hi) = (g.first as f64 - 0.5, last as f64 + 0.5);

    // The plot area, leaving room for the labels.
    let y_step = nice_step(hi - lo, 5);
    let y_ticks = ticks(lo, hi, y_step);
    let label_w = y_ticks
        .iter()
        .map(|t| text::measure(&tick_label(*t, y_step), text_px).0)
        .max()
        .unwrap_or(0);
    let left = label_w as f64 + text_px as f64 * 0.8;
    let right = text_px as f64 * 0.6;
    let top = text_px as f64 * 0.6;
    let bottom = text_px as f64 * 1.9;
    let (pw, ph) = (width as f64 - left - right, height as f64 - top - bottom);
    if pw < 10.0 || ph < 10.0 {
        return img;
    }
    let to_x = |t: f64| left + (t - x_lo) / (x_hi - x_lo) * pw;
    let to_y = |v: f64| top + (hi - v) / (hi - lo) * ph;

    // Stimulus blocks.
    let alpha = if light { 0.22 } else { 0.16 };
    for &(a, b, color) in g.stim {
        let (xa, xb) = (
            to_x(a as f64 - 0.5).max(left),
            to_x(b as f64 - 0.5).min(left + pw),
        );
        for x in xa.floor() as i64..xb.ceil() as i64 {
            for y in top as i64..(top + ph) as i64 {
                img.blend(x, y, color, alpha);
            }
        }
    }
    // Grid and tick labels.
    let grid = 0.16;
    for t in &y_ticks {
        let y = to_y(*t);
        line(
            &mut img,
            (left, y),
            (left + pw, y),
            1.0,
            blend_rgb(background, ink, grid),
        );
        let s = tick_label(*t, y_step);
        let (w, h) = text::measure(&s, text_px);
        text::draw(
            &mut img,
            &s,
            (
                (left - text_px as f64 * 0.4) as i64 - w as i64,
                (y - h as f64 / 2.0) as i64,
            ),
            text_px,
            ink,
            None,
        );
    }
    let x_step = nice_step((x_hi - x_lo).max(1.0), 6).max(1.0);
    for t in ticks(g.first as f64, last as f64, x_step) {
        let x = to_x(t);
        line(
            &mut img,
            (x, top),
            (x, top + ph),
            1.0,
            blend_rgb(background, ink, grid * 0.6),
        );
        let s = tick_label(t, x_step);
        let (w, _) = text::measure(&s, text_px);
        text::draw(
            &mut img,
            &s,
            (
                (x - w as f64 / 2.0) as i64,
                (top + ph + text_px as f64 * 0.3) as i64,
            ),
            text_px,
            ink,
            None,
        );
    }
    // Frame.
    let frame = blend_rgb(background, ink, 0.55);
    for (a, b) in [
        ((left, top), (left + pw, top)),
        ((left + pw, top), (left + pw, top + ph)),
        ((left + pw, top + ph), (left, top + ph)),
        ((left, top + ph), (left, top)),
    ] {
        line(&mut img, a, b, 1.2, frame);
    }
    // The current time point.
    if let Some(m) = g.marker.filter(|m| (g.first..=last).contains(m)) {
        let x = to_x(m as f64);
        let marker = if light { [180, 120, 0] } else { [250, 204, 21] };
        line(&mut img, (x, top), (x, top + ph), 1.6, marker);
    }
    // The traces: the series, then the fit over it.
    let w = (height as f64 / 260.0).max(1.4);
    let mut trace = |values: &[f64], rgb: [u8; 3], width: f64| {
        let mut prev: Option<(f64, f64)> = None;
        for (i, v) in values.iter().enumerate() {
            if !v.is_finite() {
                prev = None;
                continue;
            }
            let p = (to_x((g.first + i) as f64), to_y(*v));
            if let Some(q) = prev {
                line(&mut img, q, p, width, rgb);
            }
            prev = Some(p);
        }
    };
    trace(
        g.values,
        if light { [25, 28, 36] } else { [235, 235, 240] },
        w,
    );
    if let Some(fit) = g.fit {
        trace(
            fit,
            if light { [214, 88, 8] } else { [242, 140, 72] },
            w * 1.3,
        );
    }
    img
}

/// `a` with `b` mixed in by `amount`.
fn blend_rgb(a: [u8; 3], b: [u8; 3], amount: f32) -> [u8; 3] {
    let mix = |x: u8, y: u8| (f32::from(x) + (f32::from(y) - f32::from(x)) * amount).round() as u8;
    [mix(a[0], b[0]), mix(a[1], b[1]), mix(a[2], b[2])]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn count(img: &Rgba8Image, pred: impl Fn([u8; 3]) -> bool) -> usize {
        (0..img.height)
            .flat_map(|y| (0..img.width).map(move |x| (x, y)))
            .filter(|&(x, y)| pred(img.get(x, y)))
            .count()
    }

    fn series() -> Vec<f64> {
        (0..60)
            .map(|t| 100.0 + 10.0 * (t as f64 * 0.4).sin())
            .collect()
    }

    #[test]
    fn nice_steps_and_ticks() {
        assert_eq!(nice_step(100.0, 5), 20.0);
        assert_eq!(nice_step(7.0, 5), 2.0);
        assert_eq!(nice_step(0.9, 5), 0.2);
        assert_eq!(nice_step(0.0, 5), 1.0);
        assert_eq!(ticks(3.0, 17.0, 5.0), [5.0, 10.0, 15.0]);
        assert_eq!(tick_label(15.0, 5.0), "15");
        assert_eq!(tick_label(0.4, 0.2), "0.4");
    }

    #[test]
    fn the_picture_has_the_right_size_background_and_traces() {
        let values = series();
        let g = GraphPicture {
            first: 0,
            values: &values,
            fit: None,
            stim: &[],
            marker: None,
        };
        let img = render((600, 300), &g, [0, 0, 0], 14.0);
        assert_eq!((img.width, img.height), (600, 300));
        assert_eq!(img.get(2, 2), [0, 0, 0]);
        // Light trace and labels on the dark background; smooth edges (grays).
        assert!(count(&img, |c| c[0] > 200) > 200);
        assert!(count(&img, |c| c[0] > 40 && c[0] < 200) > 200);
    }

    #[test]
    fn a_light_background_gets_dark_ink() {
        let values = series();
        let g = GraphPicture {
            first: 0,
            values: &values,
            fit: None,
            stim: &[],
            marker: None,
        };
        let img = render((600, 300), &g, [255, 255, 255], 14.0);
        assert_eq!(img.get(2, 2), [255, 255, 255]);
        assert!(count(&img, |c| c[0] < 80) > 200);
        // The grid is a faint gray on the white.
        assert!(count(&img, |c| c[0] > 200 && c[0] < 250) > 200);
    }

    #[test]
    fn stimulus_the_fit_and_the_marker_add_their_colors() {
        let values = series();
        let fit: Vec<f64> = values.iter().map(|v| v + 1.0).collect();
        let plain = render(
            (600, 300),
            &GraphPicture {
                first: 0,
                values: &values,
                fit: None,
                stim: &[],
                marker: None,
            },
            [0, 0, 0],
            14.0,
        );
        let full = render(
            (600, 300),
            &GraphPicture {
                first: 0,
                values: &values,
                fit: Some(&fit),
                stim: &[(10, 20, [250, 204, 21]), (40, 50, [250, 204, 21])],
                marker: Some(30),
            },
            [0, 0, 0],
            14.0,
        );
        let orange = |c: [u8; 3]| c[0] > 180 && c[1] > 90 && c[1] < 170 && c[2] < 110;
        assert_eq!(count(&plain, orange), 0);
        assert!(count(&full, orange) > 100);
        // Shaded blocks: a warm tint where the background was black.
        assert!(count(&full, |c| c[0] > 20 && c[0] < 80 && c[1] > 15 && c[2] < 20) > 1000);
        // The marker is a gold vertical line.
        let gold = count(&full, |c| c[0] > 200 && c[1] > 150 && c[2] < 100);
        assert!(gold > 100, "{gold}");
    }

    #[test]
    fn tiny_or_empty_input_gives_a_blank_picture_not_a_crash() {
        let blank = |size: (usize, usize), values: &[f64]| {
            render(
                size,
                &GraphPicture {
                    first: 0,
                    values,
                    fit: None,
                    stim: &[],
                    marker: None,
                },
                [9, 9, 9],
                14.0,
            )
        };
        assert_eq!(count(&blank((200, 100), &[]), |c| c != [9, 9, 9]), 0);
        assert_eq!(count(&blank((20, 12), &[1.0, 2.0]), |c| c != [9, 9, 9]), 0);
        // A flat series and NaNs are fine.
        let flat = blank((300, 150), &[5.0; 10]);
        assert!(count(&flat, |c| c != [9, 9, 9]) > 0);
        let nan = blank((300, 150), &[1.0, f64::NAN, 3.0, 4.0]);
        assert_eq!((nan.width, nan.height), (300, 150));
    }
}
