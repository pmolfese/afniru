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
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Prefs {
    /// `AFNIRU_THEME = System | Dark | Light`.
    pub theme: ThemeChoice,
    /// `AFNIRU_CANVAS_BACKGROUND = Black | White`.
    pub canvas: CanvasBackground,
    /// `AFNI_LEFT_IS_LEFT = YES | NO`. `NO` (AFNI's default) is radiological:
    /// the subject's right is on the screen's left.
    pub left_is_left: bool,
    /// `AFNI_SESSTRAIL = n`: directory levels kept in dataset names.
    pub sess_trail: usize,
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
// This file was created with the defaults on first run. Edit it freely;
// afniru does not overwrite it.

***ENVIRONMENT

// ---- Appearance --------------------------------------------------------

   AFNIRU_THEME             = System  // System (follow macOS) | Dark | Light

   AFNIRU_CANVAS_BACKGROUND = Black   // Black | White; behind slice images.
                                      // White is for publication figures.

// ---- Display -----------------------------------------------------------

   AFNI_LEFT_IS_LEFT        = NO      // NO  = radiological (subject's right
                                      //       on screen left), AFNI's default
                                      // YES = neurological

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
            Ok(text) => Ok(Self::parse(&text)),
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

    fn set(&mut self, key: &str, value: &str) {
        let value = value.to_ascii_lowercase();
        match key {
            "AFNIRU_THEME" => match value.as_str() {
                "system" => self.theme = ThemeChoice::System,
                "dark" => self.theme = ThemeChoice::Dark,
                "light" => self.theme = ThemeChoice::Light,
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
            "AFNI_SESSTRAIL" => {
                if let Ok(n) = value.parse() {
                    self.sess_trail = n;
                }
            }
            _ => {}
        }
    }
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
}
