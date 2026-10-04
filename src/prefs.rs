//! User preferences from `~/.afniru`.
//!
//! The format is AFNI's `~/.afnirc`: `***SECTION` headers, `KEY = value`
//! lines, and `//` comments (`#` also works, as in sumaru's `~/.sumaru`).
//! Settings live in `***ENVIRONMENT`. Names AFNI already defines keep their
//! AFNI spelling and meaning (`AFNI_LEFT_IS_LEFT`, `AFNI_SESSTRAIL`);
//! afniru-only settings start with `AFNIRU_`. Unknown keys and sections are
//! ignored so a newer file never breaks an older build.
//!
//! On first run the complete, commented default file is created (never
//! overwriting an existing one). A malformed value keeps its default.

use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};

use afni_core::afni_colors::AfniColorScale;

use crate::geom::CoordOrient;
use crate::render::label::{Corner, LabelSize, SliceLabel};

/// Which color theme to use for the application chrome.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ThemeChoice {
    /// Follow the operating system (macOS appearance).
    #[default]
    System,
    /// Always dark.
    Dark,
    /// Always light.
    Light,
    /// AFNI's own look: black and orange.
    Classic,
}

/// Background behind slice images.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum CanvasBackground {
    /// Black in both themes (default).
    #[default]
    Black,
    /// White, for publication figures.
    White,
}

/// Parsed preferences.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Prefs {
    /// `AFNIRU_THEME = System | Dark | Light | Classic`.
    pub theme: ThemeChoice,
    /// `AFNIRU_CANVAS_BACKGROUND = Black | White`.
    pub canvas: CanvasBackground,
    /// `AFNI_LEFT_IS_LEFT = YES | NO`. `NO` (AFNI's default) is radiological:
    /// the subject's right is on the screen's left.
    pub left_is_left: bool,
    /// `AFNI_ORIENT = RAI | LPI`: how coordinates are written.
    pub coord_orient: CoordOrient,
    /// `AFNI_COLORSCALE_DEFAULT = name`: the color scale a new overlay starts with.
    pub colorscale: AfniColorScale,
    /// `AFNI_SESSTRAIL = n`: directory levels kept in dataset names.
    pub sess_trail: usize,
    /// `AFNIRU_FOLDER_BROWSER = YES | NO`: list the datasets of a folder
    /// (given on the command line, dropped, or opened) in the Datasets card.
    /// Only the names are read; a dataset is loaded when picked.
    pub folder_browser: bool,
    /// `AFNIRU_SLICE_NUMBER`, `_CORNER` and `_SIZE`: the slice number drawn on
    /// each image (also the starting point of the right-click menu).
    pub slice_label: SliceLabel,
    /// `AFNIRU_RECENT_OVERLAYS = YES | NO`: remember recently chosen overlay
    /// datasets between runs and offer them in the dropdowns (default NO).
    pub recent_overlays: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Self {
            theme: ThemeChoice::default(),
            canvas: CanvasBackground::default(),
            left_is_left: false,
            coord_orient: CoordOrient::default(),
            colorscale: AfniColorScale::afni_default(),
            sess_trail: 0,
            folder_browser: true,
            slice_label: SliceLabel::default(),
            recent_overlays: false,
        }
    }
}

/// The documented default file written on first run.
pub const DEFAULT_FILE: &str = "\
// ~/.afniru: afniru settings.
//
// Same format as AFNI's ~/.afnirc: '***SECTION' headers, 'KEY = value' lines,
// and '//' comments ('#' also works). Names AFNI already defines keep their
// AFNI meaning; afniru-only settings start with AFNIRU_. Unknown settings are
// ignored, so this file keeps working across versions. Delete a line (or
// comment it out with //) to get its default.
//
// This file was created with the defaults on first run. Edit it freely.
// When a newer afniru adds settings, it appends them here (with their
// defaults) and never changes what you wrote. A setting you comment out with
// // stays off; one you delete is added back with its default.

***ENVIRONMENT

// ---- Appearance --------------------------------------------------------

   AFNIRU_THEME             = System  // System (follow macOS) | Dark | Light
                                      // | Classic (AFNI's black and orange)

   AFNIRU_CANVAS_BACKGROUND = Black   // Black | White; behind slice images.
                                      // White is for publication figures.

// ---- Display -----------------------------------------------------------

   AFNI_LEFT_IS_LEFT        = NO      // NO  = radiological (subject's right
                                      //       on screen left), AFNI's default
                                      // YES = neurological

   AFNI_ORIENT              = RAI     // how coordinates are written (afniru
                                      // supports RAI and LPI):
                                      // RAI = x grows to the Left, y to the
                                      //       Posterior (AFNI's default)
                                      // LPI = x grows to the Right, y to the
                                      //       Anterior (= RAS+)

   AFNI_COLORSCALE_DEFAULT  = Reds_and_Blues_Inv  // the color scale a new
                                      // overlay starts with (AFNI's default).
                                      // One of: Reds_and_Blues_Inv,
                                      // Spectrum:red_to_blue, Spectrum:red_to_blue+gap,
                                      // Spectrum:yellow_to_cyan, Spectrum:yellow_to_cyan+gap,
                                      // Spectrum:yellow_to_red, Color_circle_AJJ,
                                      // Color_circle_ZSS, Reds_and_Blues,
                                      // Reds_and_Blues_w_Green

   AFNIRU_FOLDER_BROWSER    = YES     // YES = a folder given on the command line
                                      //       (or dropped / opened) lists its
                                      //       datasets in the Datasets card to
                                      //       pick from; only names are read, a
                                      //       dataset loads when you pick it
                                      // NO  = never list folders

   AFNIRU_SLICE_NUMBER      = NO      // YES = draw the slice number on every
                                      //       image (also set by the right-click
                                      //       menu of an image)
   AFNI_IMAGE_LABEL_IJK     = NO      // NO  = the label is the slice's position
                                      //       in mm with its side (33S, 12R),
                                      //       as in AFNI; YES = its index
   AFNIRU_RECENT_OVERLAYS   = NO      // YES = remember the overlay datasets you
                                      //       chose last time and list them in
                                      //       the dropdowns (underlay and Graph
                                      //       always are)
   AFNIRU_SLICE_NUMBER_CORNER = TL    // TL | TR | BL | BR
   AFNIRU_SLICE_NUMBER_SIZE = Medium  // Small | Medium | Large | XL

   AFNI_SESSTRAIL           = 0       // directory levels shown before a
                                      // dataset's name (0 = name only)
";

impl Prefs {
    /// Path of the preferences file, `~/.afniru`.
    pub fn path() -> Option<PathBuf> {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".afniru"))
    }

    /// Load `~/.afniru`, creating the documented default on first run. Never
    /// fails: any problem is reported on stderr and defaults are used.
    pub fn load_or_create() -> Self {
        let Some(path) = Self::path() else {
            return Self::default();
        };
        match Self::load_or_create_at(&path) {
            Ok(prefs) => prefs,
            Err(e) => {
                eprintln!("afniru: preferences {}: {e}", path.display());
                Self::default()
            }
        }
    }

    /// Like [`load_or_create`](Self::load_or_create) for an explicit path.
    pub fn load_or_create_at(path: &Path) -> std::io::Result<Self> {
        match fs::read_to_string(path) {
            Ok(text) => {
                // Settings this version added are written into the user's
                // file (with their defaults) so they can be found and edited.
                let added = missing_settings(&text);
                if !added.is_empty() {
                    let mut upgraded = text.clone();
                    if !upgraded.ends_with('\n') {
                        upgraded.push('\n');
                    }
                    upgraded.push_str(&added);
                    if let Err(e) = write_atomically(path, &upgraded) {
                        eprintln!("afniru: could not update {}: {e}", path.display());
                    }
                }
                Ok(Self::parse(&text))
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                // `create_new` never overwrites, even if another afniru
                // created the file in the meantime.
                match OpenOptions::new().write(true).create_new(true).open(path) {
                    Ok(mut f) => f.write_all(DEFAULT_FILE.as_bytes())?,
                    Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {
                        return Ok(Self::parse(&fs::read_to_string(path)?));
                    }
                    Err(e) => return Err(e),
                }
                Ok(Self::default())
            }
            Err(e) => Err(e),
        }
    }

    /// Parse the text of a preferences file.
    pub fn parse(text: &str) -> Self {
        let mut prefs = Self::default();
        let mut in_environment = true; // keys before any header count too
        for raw in text.lines() {
            let line = strip_comment(raw).trim();
            if let Some(section) = line.strip_prefix("***") {
                in_environment = section.trim().eq_ignore_ascii_case("ENVIRONMENT");
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                continue;
            };
            if !in_environment {
                continue;
            }
            prefs.set(key.trim(), value.trim());
        }
        prefs
    }

    fn set(&mut self, key: &str, original: &str) {
        let value = original.to_ascii_lowercase();
        match key {
            "AFNIRU_THEME" => match value.as_str() {
                "system" => self.theme = ThemeChoice::System,
                "dark" => self.theme = ThemeChoice::Dark,
                "light" => self.theme = ThemeChoice::Light,
                "classic" => self.theme = ThemeChoice::Classic,
                _ => {}
            },
            "AFNIRU_CANVAS_BACKGROUND" => match value.as_str() {
                "black" => self.canvas = CanvasBackground::Black,
                "white" => self.canvas = CanvasBackground::White,
                _ => {}
            },
            "AFNI_LEFT_IS_LEFT" => match value.as_str() {
                "yes" | "true" | "1" => self.left_is_left = true,
                "no" | "false" | "0" => self.left_is_left = false,
                _ => {}
            },
            "AFNIRU_SLICE_NUMBER" => match value.as_str() {
                "yes" | "true" | "1" => self.slice_label.show = true,
                "no" | "false" | "0" => self.slice_label.show = false,
                _ => {}
            },
            "AFNI_IMAGE_LABEL_IJK" => match value.as_str() {
                "yes" | "true" | "1" => self.slice_label.by_index = true,
                "no" | "false" | "0" => self.slice_label.by_index = false,
                _ => {}
            },
            "AFNIRU_RECENT_OVERLAYS" => match value.as_str() {
                "yes" | "true" | "1" => self.recent_overlays = true,
                "no" | "false" | "0" => self.recent_overlays = false,
                _ => {}
            },
            "AFNIRU_SLICE_NUMBER_CORNER" => {
                if let Some(c) = Corner::parse(&value) {
                    self.slice_label.corner = c;
                }
            }
            "AFNIRU_SLICE_NUMBER_SIZE" => {
                if let Some(s) = LabelSize::parse(&value) {
                    self.slice_label.size = s;
                }
            }
            "AFNIRU_FOLDER_BROWSER" => match value.as_str() {
                "yes" | "true" | "1" => self.folder_browser = true,
                "no" | "false" | "0" => self.folder_browser = false,
                _ => {}
            },
            "AFNI_ORIENT" => match value.as_str() {
                "rai" => self.coord_orient = CoordOrient::Rai,
                "lpi" => self.coord_orient = CoordOrient::Lpi,
                _ => {}
            },
            "AFNI_COLORSCALE_DEFAULT" => {
                if let Some(scale) = AfniColorScale::from_name(original) {
                    self.colorscale = scale;
                }
            }
            "AFNI_SESSTRAIL" => {
                if let Ok(n) = value.parse() {
                    self.sess_trail = n;
                }
            }
            _ => {}
        }
    }
}

/// The `(key, text)` blocks of [`DEFAULT_FILE`]: a setting's line and the
/// indented `//` lines that explain it.
fn default_settings() -> Vec<(&'static str, String)> {
    let mut out: Vec<(&str, String)> = Vec::new();
    for line in DEFAULT_FILE.lines() {
        let key = line
            .starts_with(' ')
            .then(|| line.split_once('=').map(|(k, _)| k.trim()))
            .flatten()
            .filter(|k| !k.is_empty() && k.chars().all(|c| c.is_ascii_alphanumeric() || c == '_'));
        if let Some(key) = key {
            out.push((key, format!("{line}\n")));
        } else if line.starts_with(' ')
            && line.trim_start().starts_with("//")
            && let Some((_, text)) = out.last_mut()
        {
            text.push_str(line);
            text.push('\n');
        }
    }
    out
}

/// Whether `text` mentions setting `key`, as a setting or commented out.
fn mentions(text: &str, key: &str) -> bool {
    text.lines().any(|l| {
        let l = l.trim_start_matches(|c: char| c == '/' || c == '#' || c.is_whitespace());
        l.strip_prefix(key)
            .is_some_and(|rest| rest.trim_start().starts_with('='))
    })
}

/// The documented defaults of every setting `existing` does not mention, as
/// text to append (empty when nothing is missing). A setting the user
/// commented out counts as mentioned, so it stays off.
pub fn missing_settings(existing: &str) -> String {
    let missing: Vec<String> = default_settings()
        .into_iter()
        .filter(|(key, _)| !mentions(existing, key))
        .map(|(_, text)| text)
        .collect();
    if missing.is_empty() {
        return String::new();
    }
    format!(
        "\n// ---- Added by afniru {} (new settings, with their defaults) ----\n\n***ENVIRONMENT\n\n{}",
        env!("CARGO_PKG_VERSION"),
        missing.join("\n")
    )
}

/// Write `text` to `path` through a temporary file, so a crash cannot leave
/// a half-written preferences file.
fn write_atomically(path: &Path, text: &str) -> std::io::Result<()> {
    let tmp = path.with_extension(format!("tmp{}", std::process::id()));
    fs::write(&tmp, text)?;
    fs::rename(&tmp, path).inspect_err(|_| {
        let _ = fs::remove_file(&tmp);
    })
}

/// Drop a trailing `//` or `#` comment.
fn strip_comment(line: &str) -> &str {
    let cut = [line.find("//"), line.find('#')]
        .into_iter()
        .flatten()
        .min()
        .unwrap_or(line.len());
    &line[..cut]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_when_empty() {
        let p = Prefs::parse("");
        assert_eq!(p.theme, ThemeChoice::System);
        assert_eq!(p.canvas, CanvasBackground::Black);
        assert!(!p.left_is_left);
        assert_eq!(p.sess_trail, 0);
        assert_eq!(p.coord_orient, CoordOrient::Rai);
        assert!(p.folder_browser);
        assert!(!Prefs::parse("AFNIRU_FOLDER_BROWSER = no").folder_browser);
        assert_eq!(p.colorscale, AfniColorScale::afni_default());
        assert_eq!(p.colorscale, AfniColorScale::RedsAndBluesInv);
    }

    #[test]
    fn colorscale_default_takes_afni_names_and_ignores_unknown_ones() {
        let p = Prefs::parse("AFNI_COLORSCALE_DEFAULT = Reds_and_Blues");
        assert_eq!(p.colorscale, AfniColorScale::RedsAndBlues);
        let p = Prefs::parse("AFNI_COLORSCALE_DEFAULT = Spectrum:red_to_blue");
        assert_eq!(p.colorscale, AfniColorScale::SpectrumRedToBlue);
        let p = Prefs::parse("AFNI_COLORSCALE_DEFAULT = Nope");
        assert_eq!(p.colorscale, AfniColorScale::afni_default());
    }

    #[test]
    fn afni_orient_accepts_rai_and_lpi_only() {
        assert_eq!(
            Prefs::parse("AFNI_ORIENT = lpi").coord_orient,
            CoordOrient::Lpi
        );
        assert_eq!(
            Prefs::parse("AFNI_ORIENT = LPS").coord_orient,
            CoordOrient::Rai
        );
    }

    #[test]
    fn shipped_default_file_parses_to_defaults() {
        assert_eq!(Prefs::parse(DEFAULT_FILE), Prefs::default());
    }

    #[test]
    fn parses_afnirc_style() {
        let p = Prefs::parse(
            "// hi\n***ENVIRONMENT\n AFNIRU_THEME = Dark // always\n AFNI_LEFT_IS_LEFT = YES\n\
             AFNIRU_CANVAS_BACKGROUND=white\n AFNI_SESSTRAIL = 2\n FUTURE = 3\n",
        );
        assert_eq!(p.theme, ThemeChoice::Dark);
        assert_eq!(p.canvas, CanvasBackground::White);
        assert!(p.left_is_left);
        assert_eq!(p.sess_trail, 2);
    }

    #[test]
    fn other_sections_are_ignored() {
        let p =
            Prefs::parse("***COLORS\nAFNIRU_THEME = Dark\n***ENVIRONMENT\nAFNI_SESSTRAIL = 1\n");
        assert_eq!(p.theme, ThemeChoice::System);
        assert_eq!(p.sess_trail, 1);
    }

    #[test]
    fn bad_value_keeps_default() {
        assert_eq!(
            Prefs::parse("AFNIRU_THEME = purple").theme,
            ThemeChoice::System
        );
        assert_eq!(Prefs::parse("AFNI_SESSTRAIL = x").sess_trail, 0);
    }

    #[test]
    fn creates_file_once_and_never_overwrites() {
        let dir = std::env::temp_dir().join(format!("afniru-prefs-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".afniru");
        let _ = fs::remove_file(&path);

        assert_eq!(Prefs::load_or_create_at(&path).unwrap(), Prefs::default());
        assert_eq!(fs::read_to_string(&path).unwrap(), DEFAULT_FILE);

        fs::write(&path, "***ENVIRONMENT\nAFNIRU_THEME = Light\n").unwrap();
        let p = Prefs::load_or_create_at(&path).unwrap();
        assert_eq!(p.theme, ThemeChoice::Light);
        assert!(fs::read_to_string(&path).unwrap().contains("Light"));
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn a_complete_file_has_nothing_missing() {
        assert_eq!(missing_settings(DEFAULT_FILE), "");
    }

    #[test]
    fn missing_settings_are_appended_and_user_values_kept() {
        let dir = std::env::temp_dir().join(format!("afniru-upgrade-{}", std::process::id()));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join(".afniru");
        // An old file: one value changed, one setting commented out, the rest absent.
        let old = "***ENVIRONMENT\n AFNIRU_THEME = Light\n // AFNI_LEFT_IS_LEFT = YES\n";
        fs::write(&path, old).unwrap();
        let p = Prefs::load_or_create_at(&path).unwrap();
        assert_eq!(p.theme, ThemeChoice::Light);
        let text = fs::read_to_string(&path).unwrap();
        assert!(text.starts_with(old), "the user's text is untouched");
        assert!(text.contains("AFNIRU_FOLDER_BROWSER"));
        assert!(text.contains("AFNIRU_SLICE_NUMBER_SIZE"));
        assert_eq!(text.matches("AFNIRU_THEME").count(), 1);
        assert_eq!(text.matches("AFNI_LEFT_IS_LEFT").count(), 1);
        // The upgraded file is complete, parses the same, and is not changed again.
        assert_eq!(Prefs::parse(&text).theme, ThemeChoice::Light);
        Prefs::load_or_create_at(&path).unwrap();
        assert_eq!(fs::read_to_string(&path).unwrap(), text);
        fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_slice_number_can_be_set_in_the_file() {
        let p = Prefs::parse("");
        assert_eq!(p.slice_label, SliceLabel::default());
        assert!(!p.slice_label.show);
        let p = Prefs::parse(
            "AFNIRU_SLICE_NUMBER = YES\nAFNIRU_SLICE_NUMBER_CORNER = br\nAFNIRU_SLICE_NUMBER_SIZE = XL",
        );
        assert!(p.slice_label.show);
        assert_eq!(p.slice_label.corner, Corner::BottomRight);
        assert_eq!(p.slice_label.size, LabelSize::ExtraLarge);
        // Nonsense leaves the defaults.
        let p =
            Prefs::parse("AFNIRU_SLICE_NUMBER_CORNER = middle\nAFNIRU_SLICE_NUMBER_SIZE = huge");
        assert_eq!(p.slice_label, SliceLabel::default());
    }
}
