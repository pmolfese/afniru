//! Window/level and grayscale compositing.

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

/// The slice as opaque RGBA bytes, gray through `window`.
pub fn gray_rgba(slice: &Slice, window: Window) -> Vec<u8> {
    slice
        .data
        .iter()
        .flat_map(|&x| {
            let g = window.gray(x);
            [g, g, g, 255]
        })
        .collect()
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

    #[test]
    fn rgba_is_opaque_gray() {
        let s = Slice {
            width: 2,
            height: 1,
            data: vec![0.0, 1.0],
            pixel_mm: [1.0; 2],
            left: 'R',
            right: 'L',
            top: 'A',
            bottom: 'P',
        };
        assert_eq!(
            gray_rgba(&s, Window { lo: 0.0, hi: 1.0 }),
            [0, 0, 0, 255, 255, 255, 255, 255]
        );
    }
}
