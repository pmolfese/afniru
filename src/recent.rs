//! Recently chosen datasets, remembered between runs and offered at the top
//! of the dataset dropdowns: one list for the underlay, one for overlays, one
//! for the Graph (its series and its fit).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::session::action::LoadRole;

/// Entries kept in each list.
pub const KEEP: usize = 10;

/// Which dropdown a dataset was chosen in.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecentKind {
    /// The underlay.
    Underlay,
    /// An overlay layer.
    Overlay,
    /// The Graph's series or fit.
    Graph,
}

impl RecentKind {
    /// The list a load with this role belongs to.
    pub fn of(role: LoadRole) -> Self {
        match role {
            LoadRole::Underlay => RecentKind::Underlay,
            LoadRole::Overlay | LoadRole::Layer(_) => RecentKind::Overlay,
            LoadRole::GraphSource | LoadRole::GraphFit => RecentKind::Graph,
        }
    }
}

/// The three lists, most recent first. Saved with the app's settings.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Recents {
    underlay: Vec<PathBuf>,
    overlay: Vec<PathBuf>,
    graph: Vec<PathBuf>,
}

/// The path as the dropdowns use it: an AFNI dataset without `.HEAD`, so the
/// same dataset chosen two ways is one entry.
pub fn normalize(path: &Path) -> PathBuf {
    let text = path.to_string_lossy();
    PathBuf::from(text.strip_suffix(".HEAD").unwrap_or(&text).to_string())
}

impl Recents {
    /// The list for `kind`, most recent first.
    pub fn list(&self, kind: RecentKind) -> &[PathBuf] {
        match kind {
            RecentKind::Underlay => &self.underlay,
            RecentKind::Overlay => &self.overlay,
            RecentKind::Graph => &self.graph,
        }
    }

    /// Forget the list for `kind`.
    pub fn clear(&mut self, kind: RecentKind) {
        match kind {
            RecentKind::Underlay => self.underlay.clear(),
            RecentKind::Overlay => self.overlay.clear(),
            RecentKind::Graph => self.graph.clear(),
        }
    }

    /// Remember that `path` was chosen for `kind`: it moves to the top, and
    /// the list keeps its last [`KEEP`].
    pub fn note(&mut self, kind: RecentKind, path: &Path) {
        let path = normalize(path);
        let list = match kind {
            RecentKind::Underlay => &mut self.underlay,
            RecentKind::Overlay => &mut self.overlay,
            RecentKind::Graph => &mut self.graph,
        };
        list.retain(|p| *p != path);
        list.insert(0, path);
        list.truncate(KEEP);
    }
}

/// The name shown for a recent entry: its file name.
pub fn display_name(path: &Path) -> String {
    path.file_name().map_or_else(
        || path.display().to_string(),
        |n| n.to_string_lossy().into(),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_latest_choice_comes_first_and_repeats_move_up() {
        let mut r = Recents::default();
        r.note(RecentKind::Underlay, Path::new("/d/a+orig"));
        r.note(RecentKind::Underlay, Path::new("/d/b+orig"));
        r.note(RecentKind::Underlay, Path::new("/d/a+orig.HEAD")); // same dataset
        assert_eq!(
            r.list(RecentKind::Underlay),
            [Path::new("/d/a+orig"), Path::new("/d/b+orig")]
        );
        // The lists are separate.
        assert!(r.list(RecentKind::Overlay).is_empty());
        assert!(r.list(RecentKind::Graph).is_empty());
    }

    #[test]
    fn only_the_last_ten_are_kept() {
        let mut r = Recents::default();
        for n in 0..15 {
            r.note(RecentKind::Overlay, Path::new(&format!("/d/f{n}.nii.gz")));
        }
        let list = r.list(RecentKind::Overlay);
        assert_eq!(list.len(), KEEP);
        assert_eq!(list[0], Path::new("/d/f14.nii.gz"));
        assert_eq!(list[KEEP - 1], Path::new("/d/f5.nii.gz"));
    }

    #[test]
    fn roles_choose_their_list() {
        use crate::session::LayerId;
        assert_eq!(RecentKind::of(LoadRole::Underlay), RecentKind::Underlay);
        assert_eq!(RecentKind::of(LoadRole::Overlay), RecentKind::Overlay);
        assert_eq!(
            RecentKind::of(LoadRole::Layer(LayerId(2))),
            RecentKind::Overlay
        );
        assert_eq!(RecentKind::of(LoadRole::GraphSource), RecentKind::Graph);
        assert_eq!(RecentKind::of(LoadRole::GraphFit), RecentKind::Graph);
    }

    #[test]
    fn the_lists_survive_the_settings_file() {
        let mut r = Recents::default();
        r.note(RecentKind::Graph, Path::new("/d/errts+tlrc"));
        let text = ron_roundtrip(&r);
        assert_eq!(text, r);
    }

    fn ron_roundtrip(r: &Recents) -> Recents {
        // eframe stores values as RON through its storage; check the same encoding.
        struct Mem(std::collections::HashMap<String, String>);
        impl eframe::Storage for Mem {
            fn get_string(&self, k: &str) -> Option<String> {
                self.0.get(k).cloned()
            }
            fn set_string(&mut self, k: &str, v: String) {
                self.0.insert(k.into(), v);
            }
            fn remove_string(&mut self, k: &str) {
                self.0.remove(k);
            }
            fn flush(&mut self) {}
        }
        let mut m = Mem(Default::default());
        eframe::set_value(&mut m, "k", r);
        eframe::get_value(&m, "k").unwrap()
    }

    #[test]
    fn a_name_is_the_file_name() {
        assert_eq!(display_name(Path::new("/d/x/anat+tlrc")), "anat+tlrc");
    }
}
