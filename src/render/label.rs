//! The slice number drawn on an image: whether, in which corner, and how big.
//! (Saved images draw it with `render::text`.)

/// A corner of an image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Corner {
    /// Top left.
    #[default]
    TopLeft,
    /// Top right.
    TopRight,
    /// Bottom left.
    BottomLeft,
    /// Bottom right.
    BottomRight,
}

impl Corner {
    /// Every corner, in menu order.
    pub const ALL: [Corner; 4] = [
        Corner::TopLeft,
        Corner::TopRight,
        Corner::BottomLeft,
        Corner::BottomRight,
    ];

    /// Menu text.
    pub fn label(self) -> &'static str {
        match self {
            Corner::TopLeft => "top left",
            Corner::TopRight => "top right",
            Corner::BottomLeft => "bottom left",
            Corner::BottomRight => "bottom right",
        }
    }

    /// The spelling in `~/.afniru` (`TL`, `TR`, `BL`, `BR`, or the words).
    pub fn parse(text: &str) -> Option<Corner> {
        match text
            .to_ascii_lowercase()
            .replace([' ', '-', '_'], "")
            .as_str()
        {
            "tl" | "topleft" => Some(Corner::TopLeft),
            "tr" | "topright" => Some(Corner::TopRight),
            "bl" | "bottomleft" => Some(Corner::BottomLeft),
            "br" | "bottomright" => Some(Corner::BottomRight),
            _ => None,
        }
    }
}

/// How big the number is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LabelSize {
    /// Small.
    Small,
    /// Medium.
    #[default]
    Medium,
    /// Large.
    Large,
    /// Extra large.
    ExtraLarge,
}

impl LabelSize {
    /// Every size, in menu order.
    pub const ALL: [LabelSize; 4] = [
        LabelSize::Small,
        LabelSize::Medium,
        LabelSize::Large,
        LabelSize::ExtraLarge,
    ];

    /// Menu text.
    pub fn label(self) -> &'static str {
        match self {
            LabelSize::Small => "small",
            LabelSize::Medium => "medium",
            LabelSize::Large => "large",
            LabelSize::ExtraLarge => "extra large",
        }
    }

    /// Text height on screen, in points.
    pub fn points(self) -> f32 {
        match self {
            LabelSize::Small => 11.0,
            LabelSize::Medium => 16.0,
            LabelSize::Large => 24.0,
            LabelSize::ExtraLarge => 34.0,
        }
    }

    /// Text height in a saved image, as a fraction of the image's height.
    pub fn fraction(self) -> f32 {
        match self {
            LabelSize::Small => 0.04,
            LabelSize::Medium => 0.065,
            LabelSize::Large => 0.10,
            LabelSize::ExtraLarge => 0.15,
        }
    }

    /// The spelling in `~/.afniru`.
    pub fn parse(text: &str) -> Option<LabelSize> {
        match text
            .to_ascii_lowercase()
            .replace([' ', '-', '_'], "")
            .as_str()
        {
            "small" | "s" => Some(LabelSize::Small),
            "medium" | "m" => Some(LabelSize::Medium),
            "large" | "l" => Some(LabelSize::Large),
            "extralarge" | "xl" => Some(LabelSize::ExtraLarge),
            _ => None,
        }
    }
}

/// Whether, where and how big the slice number is drawn on each image.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct SliceLabel {
    /// Drawn at all?
    pub show: bool,
    /// In which corner of the image.
    pub corner: Corner,
    /// How big.
    pub size: LabelSize,
    /// Write the slice index instead of its position in mm (AFNI's
    /// `AFNI_IMAGE_LABEL_IJK`).
    pub by_index: bool,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn corners_and_sizes_parse_in_both_spellings() {
        assert_eq!(Corner::parse("BR"), Some(Corner::BottomRight));
        assert_eq!(Corner::parse("top left"), Some(Corner::TopLeft));
        assert_eq!(Corner::parse("Bottom-Left"), Some(Corner::BottomLeft));
        assert_eq!(Corner::parse("middle"), None);
        assert_eq!(LabelSize::parse("XL"), Some(LabelSize::ExtraLarge));
        assert_eq!(LabelSize::parse("Medium"), Some(LabelSize::Medium));
        assert_eq!(LabelSize::parse("huge"), None);
    }

    #[test]
    fn sizes_grow() {
        let pts: Vec<f32> = LabelSize::ALL.iter().map(|s| s.points()).collect();
        assert!(pts.windows(2).all(|w| w[0] < w[1]));
        let frac: Vec<f32> = LabelSize::ALL.iter().map(|s| s.fraction()).collect();
        assert!(frac.windows(2).all(|w| w[0] < w[1]));
    }
}
