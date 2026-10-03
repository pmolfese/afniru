//! The loaded run plus what the user is doing with it: which step is
//! selected, and whether the files changed on disk.
//!
//! Selection is by [`StepId`], so refreshing the run (new outputs appear)
//! keeps the selected step, and never touches the displayed dataset.

use std::path::PathBuf;

use super::discover::{self, DiscoverError, Located};
use super::{ArtifactRole, ProcessingRun, StepId};

/// A dataset the user can choose to view for a step.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewOption {
    /// Dataset name.
    pub label: String,
    /// Why it is offered: `output of Smoothing`, `before this step`.
    pub note: String,
    /// The dataset's `.HEAD` or NIfTI path.
    pub path: PathBuf,
}

/// The most datasets offered for one step.
const MAX_OPTIONS: usize = 8;

/// A run with a selection.
#[derive(Debug, Clone)]
pub struct ProcessingModel {
    /// The run.
    pub run: ProcessingRun,
    /// The selected step.
    pub selected: Option<StepId>,
    located: Located,
    fingerprint: u64,
}

impl ProcessingModel {
    /// Find and load the run in `dir`; nothing is selected.
    pub fn open(dir: &std::path::Path) -> Result<Self, DiscoverError> {
        let located = discover::locate(dir)?;
        let run = discover::load_located(&located)?;
        let fingerprint = discover::fingerprint(&run);
        Ok(Self {
            run,
            selected: None,
            located,
            fingerprint,
        })
    }

    /// Select a step (or clear the selection with `None`).
    pub fn select(&mut self, id: Option<StepId>) {
        self.selected = id.filter(|i| self.run.index_of(i).is_some());
    }

    /// Have files the run depends on changed since it was loaded?
    pub fn changed_on_disk(&self) -> bool {
        discover::fingerprint(&self.run) != self.fingerprint
    }

    /// Reload from disk. The selection stays on the same step if it still
    /// exists. Returns whether anything changed.
    pub fn refresh(&mut self) -> bool {
        let Ok(run) = discover::load_located(&self.located) else {
            return false;
        };
        let fingerprint = discover::fingerprint(&run);
        let changed = run != self.run;
        self.run = run;
        self.fingerprint = fingerprint;
        if let Some(sel) = self.selected.take() {
            self.selected = self.run.index_of(&sel).map(|_| sel);
        }
        changed
    }

    /// The datasets worth viewing for a step: its own outputs, then (when
    /// there is one) the most recent earlier output of the same kind, so
    /// before and after can be compared.
    pub fn view_options(&self, id: &StepId) -> Vec<ViewOption> {
        let Some(at) = self.run.index_of(id) else {
            return Vec::new();
        };
        let step = &self.run.steps[at];
        let outputs = step.viewable();
        let mut options: Vec<ViewOption> = outputs
            .iter()
            .take(MAX_OPTIONS)
            .map(|a| ViewOption {
                label: a.name.clone(),
                note: format!("output of {}", step.label),
                path: a.path.clone(),
            })
            .collect();
        let anchor = outputs
            .iter()
            .find(|a| matches!(a.role, ArtifactRole::Epi | ArtifactRole::Anat));
        if let Some(anchor) = anchor {
            let before = self.run.steps[..at].iter().rev().find_map(|earlier| {
                earlier
                    .viewable()
                    .into_iter()
                    .find(|a| a.role == anchor.role && a.path != anchor.path)
                    .map(|a| (earlier, a))
            });
            if let Some((earlier, a)) = before {
                options.push(ViewOption {
                    label: a.name.clone(),
                    note: format!("before this step (output of {})", earlier.label),
                    path: a.path.clone(),
                });
            }
        }
        options
    }
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;
    use crate::processing::Health;
    use crate::processing::discover::testkit::{build, write_dataset};

    fn model(tmp: &crate::testutil::TempDir) -> ProcessingModel {
        ProcessingModel::open(tmp.path()).unwrap()
    }

    fn id(s: &str) -> StepId {
        StepId(s.into())
    }

    #[test]
    fn selecting_an_unknown_step_clears_the_selection() {
        let (tmp, _) = build("complete", &[], None);
        let mut m = model(&tmp);
        assert!(m.selected.is_none());
        m.select(Some(id("tcat")));
        assert_eq!(m.selected, Some(id("tcat")));
        m.select(Some(id("nonexistent")));
        assert!(m.selected.is_none());
        m.select(None);
        assert!(m.selected.is_none());
    }

    #[test]
    fn refresh_keeps_the_selected_step_and_picks_up_new_outputs() {
        let (tmp, results) = build("complete", &[], Some(3));
        let mut m = model(&tmp);
        m.select(Some(id("tshift")));
        assert!(!m.changed_on_disk());
        assert_eq!(
            m.run
                .steps
                .iter()
                .find(|s| s.block == "volreg")
                .unwrap()
                .assessment
                .health,
            Health::Unknown
        );

        write_dataset(&results.join("pb02.sub-01.r01.volreg+tlrc"), [4, 5, 6]);
        assert!(m.changed_on_disk());
        assert!(m.refresh());
        assert_eq!(m.selected, Some(id("tshift")));
        assert!(!m.changed_on_disk());
        let volreg = m.run.steps.iter().find(|s| s.block == "volreg").unwrap();
        assert!(
            volreg
                .artifacts
                .iter()
                .any(|a| a.name == "pb02.sub-01.r01.volreg" && a.exists)
        );
        assert!(!m.refresh()); // nothing new
    }

    #[test]
    fn a_selected_step_that_vanishes_is_deselected() {
        let (tmp, _) = build("complete", &[], None);
        let mut m = model(&tmp);
        m.select(Some(id("regress")));
        // The script is rewritten without the regression block.
        let text = crate::processing::discover::testkit::script_text("complete");
        let cut = text
            .find("# ================================ regress")
            .unwrap();
        fs::write(tmp.path().join("proc.sub-01"), &text[..cut]).unwrap();
        assert!(m.refresh());
        assert!(m.selected.is_none());
    }

    #[test]
    fn view_options_offer_outputs_and_the_earlier_dataset_for_comparison() {
        let (tmp, _) = build("complete", &[], None);
        let m = model(&tmp);
        let blur = m.view_options(&id("blur"));
        assert_eq!(blur[0].label, "pb03.sub-01.r01.blur");
        assert!(blur[0].note.contains("output of Smoothing"));
        let before = blur.last().unwrap();
        assert!(before.note.starts_with("before this step"), "{before:?}");
        assert_eq!(before.label, "pb02.sub-01.r01.volreg"); // the EPI before smoothing
        assert!(before.path.to_string_lossy().ends_with(".HEAD"));
    }

    #[test]
    fn steps_without_datasets_offer_nothing() {
        let (tmp, _) = build(
            "complete",
            &["pb01.sub-01.r01.tshift", "vr_base_min_outlier"],
            None,
        );
        let m = model(&tmp);
        assert!(m.view_options(&id("tshift")).is_empty());
        assert!(m.view_options(&id("nope")).is_empty());
    }

    #[test]
    fn multi_run_steps_are_capped() {
        let (tmp, _) = build("two_runs", &[], None);
        let m = model(&tmp);
        assert!(m.view_options(&id("tshift")).len() <= MAX_OPTIONS + 1);
    }
}
