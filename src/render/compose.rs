//! Window/level and grayscale compositing.

use afni_core::color::Rgba;
use afni_core::composite::{Layer, composite_layers};

use super::slice::Slice;

/// A display window: values at or below `lo` are black, at or above `hi`
/// white.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Window {
    /// Black point.
    pub lo: f32,
    /// White point.
    pub hi: f32,
}

impl Window {
    /// Automatic window: the 2nd to 98th percentile of the finite values
    /// (AFNI's default for anatomicals). Falls back to min–max when those
    /// coincide, and to a unit-wide window for constant data.
    pub fn auto(frame: &[f32]) -> Self {
        let mut v: Vec<f32> = frame.iter().copied().filter(|x| x.is_finite()).collect();
        if v.is_empty() {
            return Self { lo: 0.0, hi: 1.0 };
        }
        let (lo, hi) = (percentile(&mut v, 0.02), percentile(&mut v, 0.98));
        if hi > lo {
            return Self { lo, hi };
        }
        let min = v.iter().copied().fold(f32::INFINITY, f32::min);
        let max = v.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        if max > min {
            Self { lo: min, hi: max }
        } else {
            Self {
                lo: min,
                hi: min + 1.0,
            }
        }
    }

    /// Map a value to a gray level (NaN → black).
    pub fn gray(self, x: f32) -> u8 {
        if !x.is_finite() || self.hi <= self.lo {
            return 0;
        }
        (((x - self.lo) / (self.hi - self.lo)).clamp(0.0, 1.0) * 255.0).round() as u8
    }
}

/// The `p` quantile (0..=1) by nearest rank. Reorders `v`.
fn percentile(v: &mut [f32], p: f32) -> f32 {
    let idx = ((v.len() - 1) as f32 * p).round() as usize;
    *v.select_nth_unstable_by(idx, f32::total_cmp).1
}

/// The slice as opaque colors, gray through `window`.
pub fn underlay_colors(slice: &Slice, window: Window) -> Vec<Rgba> {
    slice
        .data
        .iter()
        .map(|&x| {
            let g = f32::from(window.gray(x)) / 255.0;
            Rgba {
                r: g,
                g,
                b: g,
                a: 1.0,
            }
        })
        .collect()
}

/// The slice as RGBA bytes: gray through `window`, with each of `layers`
/// (one color per pixel, straight alpha) composited over it, first layer
/// first. A layer whose length does not match the slice is skipped.
pub fn compose_rgba(slice: &Slice, window: Window, layers: &[Vec<Rgba>]) -> Vec<u8> {
    let under = underlay_colors(slice, window);
    let planes: Vec<Layer> = layers
        .iter()
        .filter(|l| l.len() == under.len())
        .map(|l| Layer::new(l))
        .collect();
    let colors = composite_layers(&under, &planes).unwrap_or(under);
    colors.iter().flat_map(|c| c.to_u8()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn auto_window_uses_2_to_98_percentiles() {
        let frame: Vec<f32> = (0..=100).map(|x| x as f32).collect();
        assert_eq!(Window::auto(&frame), Window { lo: 2.0, hi: 98.0 });
    }

    #[test]
    fn auto_window_ignores_nan_and_inf() {
        let mut frame: Vec<f32> = (0..=100).map(|x| x as f32).collect();
        frame.extend([f32::NAN, f32::INFINITY]);
        assert_eq!(Window::auto(&frame), Window { lo: 2.0, hi: 98.0 });
    }

    #[test]
    fn auto_window_falls_back_when_percentiles_coincide() {
        // Mostly zeros with a few bright voxels: 2–98% are both 0.
        let mut frame = vec![0.0; 1000];
        frame[0] = 50.0;
        assert_eq!(Window::auto(&frame), Window { lo: 0.0, hi: 50.0 });
        assert_eq!(Window::auto(&[3.0; 10]), Window { lo: 3.0, hi: 4.0 });
        assert_eq!(Window::auto(&[]), Window { lo: 0.0, hi: 1.0 });
    }

    #[test]
    fn gray_maps_and_clamps() {
        let w = Window { lo: 10.0, hi: 20.0 };
        assert_eq!(w.gray(5.0), 0);
        assert_eq!(w.gray(10.0), 0);
        assert_eq!(w.gray(15.0), 128);
        assert_eq!(w.gray(25.0), 255);
        assert_eq!(w.gray(f32::NAN), 0);
    }

    fn two_pixels() -> Slice {
        Slice {
            width: 2,
            height: 1,
            data: vec![0.0, 1.0],
            pixel_mm: [1.0; 2],
            left: 'R',
            right: 'L',
            top: 'A',
            bottom: 'P',
        }
    }

    #[test]
    fn rgba_is_opaque_gray_without_layers() {
        assert_eq!(
            compose_rgba(&two_pixels(), Window { lo: 0.0, hi: 1.0 }, &[]),
            [0, 0, 0, 255, 255, 255, 255, 255]
        );
    }

    #[test]
    fn an_opaque_layer_replaces_the_pixel_and_a_transparent_one_leaves_it() {
        let red = Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let layer = vec![Rgba::TRANSPARENT, red];
        assert_eq!(
            compose_rgba(&two_pixels(), Window { lo: 0.0, hi: 1.0 }, &[layer]),
            [0, 0, 0, 255, 255, 0, 0, 255]
        );
    }

    #[test]
    fn a_half_transparent_layer_blends_with_the_gray() {
        let red = Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 0.5,
        };
        let out = compose_rgba(
            &two_pixels(),
            Window { lo: 0.0, hi: 1.0 },
            &[vec![red, red]],
        );
        // Over black: (127/128, 0, 0); over white: (255, 128, 128).
        assert_eq!(out[3], 255);
        assert!((127..=128).contains(&out[0]) && out[1] == 0, "{out:?}");
        assert!(out[4] == 255 && (127..=128).contains(&out[5]), "{out:?}");
    }

    #[test]
    fn a_layer_of_the_wrong_size_is_ignored() {
        let out = compose_rgba(
            &two_pixels(),
            Window { lo: 0.0, hi: 1.0 },
            &[vec![Rgba::WHITE]],
        );
        assert_eq!(out, [0, 0, 0, 255, 255, 255, 255, 255]);
    }

    #[test]
    fn layers_composite_bottom_to_top_so_order_and_opacity_matter() {
        let one = Slice {
            width: 1,
            height: 1,
            data: vec![0.0],
            pixel_mm: [1.0; 2],
            left: 'R',
            right: 'L',
            top: 'A',
            bottom: 'P',
        };
        let w = Window { lo: 0.0, hi: 1.0 };
        let red = Rgba {
            r: 1.0,
            g: 0.0,
            b: 0.0,
            a: 1.0,
        };
        let blue_quarter = Rgba {
            r: 0.0,
            g: 0.0,
            b: 1.0,
            a: 0.25,
        };
        let blue = Rgba {
            b: 1.0,
            r: 0.0,
            g: 0.0,
            a: 1.0,
        };
        let red_quarter = Rgba { a: 0.25, ..red };
        // Blue at 25% over red: mostly red.
        assert_eq!(
            compose_rgba(&one, w, &[vec![red], vec![blue_quarter]]),
            [191, 0, 64, 255]
        );
        // The other way round: mostly blue.
        assert_eq!(
            compose_rgba(&one, w, &[vec![blue], vec![red_quarter]]),
            [64, 0, 191, 255]
        );
    }
}
