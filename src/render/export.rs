//! Saving what the views show: single slices, the three views arranged in a
//! row, a column, or a 2×2 grid, and montages of many slices of one plane.
//!
//! Everything here works on plain RGBA pixels (`Rgba8Image`), so it needs no
//! window: the view area renders each slice to pixels, and this module adds
//! the orientation letters, the slice number and the crosshair, arranges the
//! tiles, and writes a PNG. Saved images are bigger than the screen can show:
//! each voxel becomes a block of `zoom` pixels (nearest neighbor, never
//! smoothed), and anisotropic voxels are made square.

use std::path::Path;

use super::label::{self, SliceLabel};
use super::text;
use crate::geom::Plane;

/// A picture: `width × height` RGBA bytes, top row first.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Rgba8Image {
    /// Pixels across.
    pub width: usize,
    /// Pixels down.
    pub height: usize,
    /// `width * height * 4` bytes.
    pub rgba: Vec<u8>,
}

impl Rgba8Image {
    /// A picture of one opaque color.
    pub fn filled(width: usize, height: usize, [r, g, b]: [u8; 3]) -> Self {
        Self {
            width,
            height,
            rgba: [r, g, b, 255].repeat(width * height),
        }
    }

    /// Wrap `rgba` (which must be `width * height * 4` bytes).
    pub fn from_rgba(width: usize, height: usize, rgba: Vec<u8>) -> Self {
        debug_assert_eq!(rgba.len(), width * height * 4);
        Self {
            width,
            height,
            rgba,
        }
    }

    /// The color at (`x`, `y`).
    #[cfg(test)]
    pub fn get(&self, x: usize, y: usize) -> [u8; 3] {
        let i = (y * self.width + x) * 4;
        [self.rgba[i], self.rgba[i + 1], self.rgba[i + 2]]
    }

    /// Paint (`x`, `y`) with an opaque color; outside the picture is ignored.
    pub fn set(&mut self, x: i64, y: i64, [r, g, b]: [u8; 3]) {
        if x >= 0 && y >= 0 && (x as usize) < self.width && (y as usize) < self.height {
            let i = (y as usize * self.width + x as usize) * 4;
            self.rgba[i..i + 4].copy_from_slice(&[r, g, b, 255]);
        }
    }

    /// Blend `rgb` over (`x`, `y`) with opacity `alpha` (0 to 1); outside the
    /// picture is ignored.
    pub fn blend(&mut self, x: i64, y: i64, [r, g, b]: [u8; 3], alpha: f32) {
        if alpha <= 0.0
            || x < 0
            || y < 0
            || (x as usize) >= self.width
            || (y as usize) >= self.height
        {
            return;
        }
        let a = alpha.min(1.0);
        let i = (y as usize * self.width + x as usize) * 4;
        for (k, v) in [r, g, b].into_iter().enumerate() {
            let old = f32::from(self.rgba[i + k]);
            self.rgba[i + k] = (old + (f32::from(v) - old) * a).round() as u8;
        }
        self.rgba[i + 3] = 255;
    }

    /// Nearest-neighbor resize (blocks stay sharp).
    pub fn resized(&self, width: usize, height: usize) -> Self {
        let mut out = Vec::with_capacity(width * height * 4);
        for y in 0..height {
            let sy = (y * self.height / height).min(self.height - 1);
            for x in 0..width {
                let sx = (x * self.width / width).min(self.width - 1);
                let i = (sy * self.width + sx) * 4;
                out.extend_from_slice(&self.rgba[i..i + 4]);
            }
        }
        Self::from_rgba(width, height, out)
    }

    /// Copy `src` onto this picture with its top left at (`x`, `y`), clipped.
    pub fn blit(&mut self, src: &Rgba8Image, x: usize, y: usize) {
        for sy in 0..src.height {
            let dy = y + sy;
            if dy >= self.height {
                break;
            }
            let w = src.width.min(self.width.saturating_sub(x));
            let from = sy * src.width * 4;
            let to = (dy * self.width + x) * 4;
            self.rgba[to..to + w * 4].copy_from_slice(&src.rgba[from..from + w * 4]);
        }
    }

    /// Write a PNG file.
    pub fn save_png(&self, path: &Path) -> Result<(), String> {
        image::save_buffer(
            path,
            &self.rgba,
            self.width as u32,
            self.height as u32,
            image::ColorType::Rgba8,
        )
        .map_err(|e| format!("{}: {e}", path.display()))
    }
}

/// The text color on a background: white on dark, black on light.
pub fn ink_for(background: [u8; 3]) -> [u8; 3] {
    let luma = 0.299 * f32::from(background[0])
        + 0.587 * f32::from(background[1])
        + 0.114 * f32::from(background[2]);
    if luma > 140.0 {
        [30, 30, 30]
    } else {
        [235, 235, 235]
    }
}

/// Text height in pixels for a size that is `fraction` of the picture's
/// height (never smaller than 9, below which letters blur).
fn text_px(height: usize, fraction: f32) -> f32 {
    (height as f32 * fraction).max(9.0)
}

/// Draw the slice number in its corner (white with a dark outline), at the
/// size the label asks for. The text is smooth, however big.
///
/// `reference` is the height the text size is measured against: pass the same
/// height for every picture of a figure and the numbers come out the same
/// size in all of them, whatever their own heights (`None`: the picture's own).
pub fn draw_slice_number(
    img: &mut Rgba8Image,
    text: &str,
    label: &SliceLabel,
    reference: Option<usize>,
) {
    if !label.show {
        return;
    }
    let px = text_px(reference.unwrap_or(img.height), label.size.fraction());
    let (w, h) = text::measure(text, px);
    let margin = (px * 0.35) as i64;
    let (w, h) = (w as i64, h as i64);
    let (x, y) = match label.corner {
        label::Corner::TopLeft => (margin, margin),
        label::Corner::TopRight => (img.width as i64 - w - margin, margin),
        label::Corner::BottomLeft => (margin, img.height as i64 - h - margin),
        label::Corner::BottomRight => (
            img.width as i64 - w - margin,
            img.height as i64 - h - margin,
        ),
    };
    text::draw(
        img,
        text,
        (x, y),
        px,
        [255, 255, 255],
        Some(([0, 0, 0], (px * 0.07).max(1.0))),
    );
}

/// The picture inside a frame that carries the orientation letters (left,
/// right, top, bottom) on its four sides.
///
/// `reference` is as for [`draw_slice_number`]: the same height for every
/// picture of a figure gives letters of the same size in all of them.
pub fn with_letters(
    img: &Rgba8Image,
    letters: [char; 4],
    background: [u8; 3],
    reference: Option<usize>,
) -> Rgba8Image {
    let px = text_px(reference.unwrap_or(img.height), 0.05);
    let (gw, gh) = text::measure("W", px);
    let pad = gw.max(gh) + (px * 0.6) as usize;
    let mut out = Rgba8Image::filled(img.width + 2 * pad, img.height + 2 * pad, background);
    out.blit(img, pad, pad);
    let ink = ink_for(background);
    let mut put = |c: char, cx: usize, cy: usize| {
        let text = c.to_string();
        let (w, h) = text::measure(&text, px);
        text::draw(
            &mut out,
            &text,
            (cx as i64 - w as i64 / 2, cy as i64 - h as i64 / 2),
            px,
            ink,
            None,
        );
    };
    let (mid_x, mid_y) = (pad + img.width / 2, pad + img.height / 2);
    put(letters[0], pad / 2, mid_y);
    put(letters[1], pad + img.width + pad / 2, mid_y);
    put(letters[2], mid_x, pad / 2);
    put(letters[3], mid_x, pad + img.height + pad / 2);
    out
}

/// Crosshair lines through pixel (`x`, `y`), a gap around it, in two colors
/// (vertical line, horizontal line); `thickness` pixels wide.
pub fn draw_crosshair(
    img: &mut Rgba8Image,
    (x, y): (i64, i64),
    vertical: [u8; 3],
    horizontal: [u8; 3],
    gap: i64,
    thickness: i64,
) {
    let half = thickness / 2;
    for t in -half..=(thickness - 1 - half) {
        for yy in 0..img.height as i64 {
            if (yy - y).abs() > gap {
                img.set(x + t, yy, vertical);
            }
        }
        for xx in 0..img.width as i64 {
            if (xx - x).abs() > gap {
                img.set(xx, y + t, horizontal);
            }
        }
    }
}

/// How the three views are arranged in one image (or not).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ViewsLayout {
    /// Side by side: axial, sagittal, coronal.
    Row,
    /// Stacked, in the same order.
    Column,
    /// 2×2 as on screen: axial, sagittal, coronal and an empty cell where the
    /// Graph is.
    Grid,
    /// One file for each view.
    Individual,
}

impl ViewsLayout {
    /// Every layout, in menu order.
    pub const ALL: [ViewsLayout; 4] = [
        ViewsLayout::Row,
        ViewsLayout::Column,
        ViewsLayout::Grid,
        ViewsLayout::Individual,
    ];

    /// Menu text.
    pub fn label(self) -> &'static str {
        match self {
            ViewsLayout::Row => "one row",
            ViewsLayout::Column => "one column",
            ViewsLayout::Grid => "2×2 grid",
            ViewsLayout::Individual => "separate files",
        }
    }
}

/// A montage: slices `first` to `last` in steps of `step` of one plane, left to
/// right and top to bottom, at most `rows × cols` of them.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MontageSpec {
    /// Which plane.
    pub plane: Plane,
    /// Rows of tiles.
    pub rows: usize,
    /// Columns of tiles.
    pub cols: usize,
    /// First slice.
    pub first: usize,
    /// Last slice (inclusive).
    pub last: usize,
    /// Step between slices (at least 1).
    pub step: usize,
}

impl MontageSpec {
    /// The slice numbers, in order: from `first` up to `last` (clamped to the
    /// `count` slices there are) by `step`, no more than there are tiles.
    pub fn slices(&self, count: usize) -> Vec<usize> {
        if count == 0 {
            return Vec::new();
        }
        let last = self.last.min(count - 1);
        (self.first..=last)
            .step_by(self.step.max(1))
            .take(self.rows.max(1) * self.cols.max(1))
            .collect()
    }

    /// A step that fits slices `first..=last` into `rows × cols` tiles.
    pub fn step_to_fit(first: usize, last: usize, rows: usize, cols: usize) -> usize {
        let n = last.saturating_sub(first) + 1;
        n.div_ceil((rows * cols).max(1)).max(1)
    }
}

/// What to save.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExportWhat {
    /// The slice shown in one plane.
    Slice(Plane),
    /// The three views, arranged.
    Views(ViewsLayout),
    /// A montage of slices of one plane.
    Montage(MontageSpec),
    /// The Graph (the time series at the crosshair) on its own.
    Graph,
}

/// Look of the saved pictures.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ExportOptions {
    /// Pixels per voxel along the smallest voxel edge (1 to 8).
    pub zoom: u32,
    /// Orientation letters on the sides of each tile.
    pub letters: bool,
    /// The crosshair, in the three plane colors.
    pub crosshair: bool,
    /// The slice number (where and how big as on screen; `show` off leaves it out).
    pub label: SliceLabel,
    /// Include the Graph with the three views (as a fourth picture in a row or
    /// column, in its cell of the 2×2 grid, or as its own file).
    pub graph: bool,
}

impl Default for ExportOptions {
    fn default() -> Self {
        Self {
            zoom: 4,
            letters: true,
            crosshair: false,
            label: SliceLabel::default(),
            graph: false,
        }
    }
}

/// Arrange `tiles` in a grid of `cols` columns, left to right and top to
/// bottom, each centered in a cell as big as the biggest tile, `gap` pixels
/// apart, on `background`. Missing tiles leave empty cells.
pub fn arrange(
    tiles: &[Rgba8Image],
    rows: usize,
    cols: usize,
    gap: usize,
    background: [u8; 3],
) -> Rgba8Image {
    let cell_w = tiles.iter().map(|t| t.width).max().unwrap_or(1);
    let cell_h = tiles.iter().map(|t| t.height).max().unwrap_or(1);
    let (rows, cols) = (rows.max(1), cols.max(1));
    let mut out = Rgba8Image::filled(
        cols * cell_w + (cols - 1) * gap,
        rows * cell_h + (rows - 1) * gap,
        background,
    );
    for (n, tile) in tiles.iter().take(rows * cols).enumerate() {
        let (r, c) = (n / cols, n % cols);
        let x = c * (cell_w + gap) + (cell_w - tile.width) / 2;
        let y = r * (cell_h + gap) + (cell_h - tile.height) / 2;
        out.blit(tile, x, y);
    }
    out
}

/// The file name for one of several pictures: `base_axial.png` for
/// `base.png`.
pub fn numbered_name(base: &Path, suffix: &str) -> std::path::PathBuf {
    let stem = base
        .file_stem()
        .map_or_else(|| "image".to_string(), |s| s.to_string_lossy().into_owned());
    base.with_file_name(format!("{stem}_{suffix}.png"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::render::label::{Corner, LabelSize};

    fn solid(w: usize, h: usize, c: [u8; 3]) -> Rgba8Image {
        Rgba8Image::filled(w, h, c)
    }

    #[test]
    fn resizing_keeps_blocks_sharp() {
        let mut img = solid(2, 1, [0, 0, 0]);
        img.set(1, 0, [200, 100, 50]);
        let big = img.resized(6, 3);
        assert_eq!((big.width, big.height), (6, 3));
        assert_eq!(big.get(2, 1), [0, 0, 0]);
        assert_eq!(big.get(3, 2), [200, 100, 50]);
        // No in-between colors.
        let mut colors: Vec<[u8; 3]> = (0..6).map(|x| big.get(x, 0)).collect();
        colors.dedup();
        assert_eq!(colors.len(), 2);
    }

    #[test]
    fn blit_copies_and_clips() {
        let mut canvas = solid(4, 4, [0, 0, 0]);
        let tile = solid(3, 3, [9, 9, 9]);
        canvas.blit(&tile, 2, 2); // hangs off the corner
        assert_eq!(canvas.get(3, 3), [9, 9, 9]);
        assert_eq!(canvas.get(1, 1), [0, 0, 0]);
        canvas.blit(&tile, 0, 0);
        assert_eq!(canvas.get(2, 2), [9, 9, 9]);
    }

    #[test]
    fn the_slice_number_goes_in_the_chosen_corner() {
        let bright = |img: &Rgba8Image, x0: usize, x1: usize, y0: usize, y1: usize| {
            (y0..y1).any(|y| (x0..x1).any(|x| img.get(x, y)[0] >= 200))
        };
        for corner in Corner::ALL {
            let mut img = solid(200, 200, [60, 60, 60]);
            let label = SliceLabel {
                show: true,
                corner,
                size: LabelSize::Large,
                by_index: false,
            };
            draw_slice_number(&mut img, "75", &label, None);
            let (left, top) = (
                matches!(corner, Corner::TopLeft | Corner::BottomLeft),
                matches!(corner, Corner::TopLeft | Corner::TopRight),
            );
            let (x0, x1) = if left { (0, 100) } else { (100, 200) };
            let (y0, y1) = if top { (0, 100) } else { (100, 200) };
            assert!(bright(&img, x0, x1, y0, y1), "{corner:?}");
            // And nowhere else.
            let (ox0, ox1) = if left { (100, 200) } else { (0, 100) };
            let (oy0, oy1) = if top { (100, 200) } else { (0, 100) };
            assert!(!bright(&img, ox0, ox1, 0, 200) && !bright(&img, 0, 200, oy0, oy1));
        }
    }

    #[test]
    fn a_bigger_size_makes_a_bigger_number_and_off_draws_nothing() {
        let count = |size, show| {
            let mut img = solid(300, 300, [0, 0, 0]);
            let label = SliceLabel {
                show,
                corner: Corner::TopLeft,
                size,
                by_index: false,
            };
            draw_slice_number(&mut img, "8", &label, None);
            (0..300)
                .flat_map(|y| (0..300).map(move |x| (x, y)))
                .filter(|&(x, y)| img.get(x, y)[0] >= 200)
                .count()
        };
        assert!(count(LabelSize::Small, true) < count(LabelSize::Medium, true));
        assert!(count(LabelSize::Medium, true) < count(LabelSize::ExtraLarge, true));
        assert_eq!(count(LabelSize::Large, false), 0);
    }

    #[test]
    fn a_shared_reference_height_gives_the_same_text_size_to_pictures_of_different_heights() {
        let label = SliceLabel {
            show: true,
            corner: Corner::TopLeft,
            size: LabelSize::Large,
            by_index: false,
        };
        // Height of the white-ish text in the top left corner.
        let text_height = |img: &Rgba8Image| {
            (0..img.height)
                .filter(|&y| (0..img.width / 2).any(|x| img.get(x, y)[0] >= 200))
                .count()
        };
        let mut short = solid(300, 150, [60, 60, 60]);
        let mut tall = solid(300, 400, [60, 60, 60]);
        draw_slice_number(&mut short, "96", &label, Some(400));
        draw_slice_number(&mut tall, "96", &label, Some(400));
        assert_eq!(text_height(&short), text_height(&tall));
        // Without a shared reference the taller picture gets bigger text.
        let mut short = solid(300, 150, [60, 60, 60]);
        let mut tall = solid(300, 400, [60, 60, 60]);
        draw_slice_number(&mut short, "96", &label, None);
        draw_slice_number(&mut tall, "96", &label, None);
        assert!(text_height(&tall) > text_height(&short));
        // The same holds for the orientation letters' frame.
        let a = with_letters(
            &solid(100, 50, [9, 9, 9]),
            ['R', 'L', 'A', 'P'],
            [0, 0, 0],
            Some(300),
        );
        let b = with_letters(
            &solid(100, 200, [9, 9, 9]),
            ['R', 'L', 'A', 'P'],
            [0, 0, 0],
            Some(300),
        );
        assert_eq!(a.width - 100, b.width - 100);
        assert_eq!(a.height - 50, b.height - 200);
    }

    #[test]
    fn letters_frame_the_picture_without_covering_it() {
        let img = solid(40, 30, [100, 100, 100]);
        let framed = with_letters(&img, ['R', 'L', 'A', 'P'], [0, 0, 0], None);
        assert!(framed.width > 40 && framed.height > 30);
        let pad = (framed.width - 40) / 2;
        assert_eq!(framed.get(pad + 5, pad + 5), [100, 100, 100]); // the picture is intact
        // Some pixel of the left margin is lit (the letter).
        let lit = (0..framed.height).any(|y| (0..pad).any(|x| framed.get(x, y)[0] > 100));
        assert!(lit);
        // Black text on a white background.
        assert_eq!(ink_for([255, 255, 255]), [30, 30, 30]);
    }

    #[test]
    fn a_crosshair_leaves_a_gap_at_the_focus() {
        let mut img = solid(21, 21, [0, 0, 0]);
        draw_crosshair(&mut img, (10, 10), [255, 0, 0], [0, 255, 0], 3, 1);
        assert_eq!(img.get(10, 0), [255, 0, 0]);
        assert_eq!(img.get(0, 10), [0, 255, 0]);
        assert_eq!(img.get(10, 9), [0, 0, 0]); // the gap
        assert_eq!(img.get(12, 10), [0, 0, 0]);
    }

    #[test]
    fn arranging_centers_each_tile_in_its_cell() {
        let a = solid(10, 4, [1, 1, 1]);
        let b = solid(4, 10, [2, 2, 2]);
        let row = arrange(&[a.clone(), b.clone()], 1, 2, 2, [0, 0, 0]);
        assert_eq!((row.width, row.height), (10 + 2 + 10, 10));
        assert_eq!(row.get(0, 3), [1, 1, 1]); // a is centered vertically (rows 3..7)
        assert_eq!(row.get(0, 0), [0, 0, 0]);
        assert_eq!(row.get(12 + 3, 0), [2, 2, 2]); // b is centered horizontally
        let column = arrange(&[a.clone(), b.clone()], 2, 1, 0, [0, 0, 0]);
        assert_eq!((column.width, column.height), (10, 20));
        // 2x2 with a missing fourth tile leaves its cell empty.
        let grid = arrange(&[a.clone(), b.clone(), a], 2, 2, 0, [9, 9, 9]);
        assert_eq!(grid.get(grid.width - 1, grid.height - 1), [9, 9, 9]);
    }

    #[test]
    fn montage_slices_follow_first_last_step_and_the_number_of_tiles() {
        let spec = |first, last, step, rows, cols| MontageSpec {
            plane: Plane::Axial,
            rows,
            cols,
            first,
            last,
            step,
        };
        assert_eq!(spec(0, 9, 3, 2, 2).slices(100), [0, 3, 6, 9]);
        assert_eq!(spec(2, 99, 1, 1, 3).slices(100), [2, 3, 4]); // limited by tiles
        assert_eq!(spec(0, 99, 1, 1, 5).slices(4), [0, 1, 2, 3]); // limited by the volume
        assert_eq!(spec(0, 5, 0, 1, 9).slices(10), [0, 1, 2, 3, 4, 5]); // step 0 is 1
        assert!(spec(0, 5, 1, 1, 1).slices(0).is_empty());
        assert_eq!(MontageSpec::step_to_fit(0, 149, 4, 6), 7); // 150 slices in 24 tiles
        assert_eq!(MontageSpec::step_to_fit(10, 12, 2, 2), 1);
    }

    #[test]
    fn png_files_round_trip() {
        let dir = crate::testutil::TempDir::new("png");
        let mut img = solid(3, 2, [10, 20, 30]);
        img.set(2, 1, [200, 100, 50]);
        let path = dir.path().join("x.png");
        img.save_png(&path).unwrap();
        let back = image::open(&path).unwrap().to_rgba8();
        assert_eq!((back.width(), back.height()), (3, 2));
        assert_eq!(back.get_pixel(2, 1).0, [200, 100, 50, 255]);
        assert_eq!(back.get_pixel(0, 0).0, [10, 20, 30, 255]);
        assert!(img.save_png(&dir.path().join("no/such/dir/x.png")).is_err());
    }

    #[test]
    fn several_files_are_named_after_the_chosen_one() {
        let p = std::path::PathBuf::from("/tmp/figs/brain.png");
        assert_eq!(
            numbered_name(&p, "axial"),
            Path::new("/tmp/figs/brain_axial.png")
        );
        let q = std::path::PathBuf::from("/tmp/figs/brain");
        assert_eq!(
            numbered_name(&q, "coronal"),
            Path::new("/tmp/figs/brain_coronal.png")
        );
    }
}
