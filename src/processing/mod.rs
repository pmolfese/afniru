//! Processing provenance: how the data on screen was made.
//!
//! An application service (not an `afni-core` algorithm). It turns an
//! `afni_proc.py` results directory into a [`ProcessingRun`]: the steps in the
//! order they were executed, the artifacts each step wrote, and a
//! [`HealthAssessment`] per step built from inspectable [`HealthEvidence`].
//!
//! The types here are file-neutral; nothing outside [`script`] and
//! [`discover`] knows about shell syntax or file names, so another pipeline
//! (fMRIPrep, a custom script) could be added as one more adapter.
//!
//! * [`script`]: parse a `proc.<subj>` script into ordered blocks and outputs.
//! * [`discover`]: find the script and results, resolve artifacts on disk.
//! * [`review`]: read AFNI's `out.ss_review.*.txt` and warning files.
//! * [`health`]: the rules, their thresholds and the aggregation.
//! * [`model`]: the loaded run plus selection, refresh and "View" options.
//!
//! See `docs/PROCESSING_RAIL.md` for the supported inputs and every rule.

pub mod discover;
pub mod health;
pub mod model;
pub mod review;
pub mod script;

use std::path::PathBuf;

/// How good a step looks. Declared worst-first: the order is the order used
/// to combine checks (a failed check is never hidden by green ones).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum Health {
    /// At least one check failed.
    Failed,
    /// No check failed, at least one raises a caution.
    Caution,
    /// Something could not be assessed (missing evidence); never guessed good.
    Unknown,
    /// Every check passed.
    Good,
}

impl Health {
    /// Word for tooltips and screen readers.
    pub fn label(self) -> &'static str {
        match self {
            Health::Good => "Good",
            Health::Caution => "Caution",
            Health::Failed => "Failed",
            Health::Unknown => "Unknown",
        }
    }
}

/// Where a fact came from, so the interface can explain its conclusions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SourceKind {
    /// An option of the generating `afni_proc.py` command.
    #[expect(
        dead_code,
        reason = "evidence derived from afni_proc.py options, e.g. the censor limits"
    )]
    ScriptOption,
    /// A command in the generated `proc` script.
    GeneratedCommand,
    /// A dataset header.
    DatasetHeader,
    /// A value in AFNI's `out.ss_review.*.txt`.
    ReviewValue,
    /// Another QC artifact (a warning file, an output file).
    QcArtifact,
    /// Whether a file exists.
    FileSystem,
}

/// The origin of one inferred fact.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EvidenceSource {
    /// What kind of source.
    pub kind: SourceKind,
    /// Where exactly, e.g. `out.ss_review.sub-01.txt: censor fraction` or
    /// `proc.sub-01:312`.
    pub detail: String,
}

/// One check and what it found.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthEvidence {
    /// Stable id of the rule, e.g. `regress.censor_fraction`.
    pub check: &'static str,
    /// Short name of the check.
    pub title: String,
    /// The result.
    pub health: Health,
    /// One line: what was measured or why it could not be.
    pub finding: String,
    /// The rule: thresholds and who defined them.
    pub rule: String,
    /// Where the measurement came from.
    pub source: EvidenceSource,
}

/// All checks of a step and their combination.
#[derive(Debug, Clone, PartialEq)]
pub struct HealthAssessment {
    /// The combined state (see [`health::combine`]).
    pub health: Health,
    /// Every individual check, kept in full.
    pub evidence: Vec<HealthEvidence>,
}

impl HealthAssessment {
    /// A concise reason: the finding of the check that decided the state.
    pub fn reason(&self) -> String {
        self.evidence
            .iter()
            .find(|e| e.health == self.health)
            .map_or_else(|| "no checks".to_string(), |e| e.finding.clone())
    }
}

/// What an artifact is for.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtifactRole {
    /// Anatomical dataset.
    Anat,
    /// Functional (EPI) time series.
    Epi,
    /// A mask.
    Mask,
    /// Statistics (`stats.*`).
    Stats,
    /// Residuals (`errts.*`) or fitted time series.
    Residual,
    /// A transform or warp.
    Transform,
    /// Motion parameters or their summaries.
    Motion,
    /// Censoring or outlier time series.
    Censor,
    /// The regression matrix or regressors.
    Regressors,
    /// A text report or QC file.
    Report,
    /// Anything else.
    Other,
}

impl ArtifactRole {
    /// Word for the interface.
    pub fn label(self) -> &'static str {
        match self {
            ArtifactRole::Anat => "anatomy",
            ArtifactRole::Epi => "EPI",
            ArtifactRole::Mask => "mask",
            ArtifactRole::Stats => "stats",
            ArtifactRole::Residual => "residuals",
            ArtifactRole::Transform => "transform",
            ArtifactRole::Motion => "motion",
            ArtifactRole::Censor => "censor",
            ArtifactRole::Regressors => "regressors",
            ArtifactRole::Report => "report",
            ArtifactRole::Other => "other",
        }
    }
}

/// The coordinate space an artifact is in, from its AFNI view.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Space {
    /// `+orig`: the subject's own space.
    Orig,
    /// `+acpc`.
    Acpc,
    /// `+tlrc`: standard space.
    Tlrc,
    /// Not known (NIfTI without view, text files, missing files).
    Unknown,
}

impl Space {
    /// Word for the interface.
    pub fn label(self) -> &'static str {
        match self {
            Space::Orig => "orig",
            Space::Acpc => "acpc",
            Space::Tlrc => "tlrc",
            Space::Unknown => "?",
        }
    }
}

/// One file a step wrote (or should have written).
#[derive(Debug, Clone, PartialEq)]
pub struct StepArtifact {
    /// Short name as the script gives it, e.g. `pb01.sub-01.r01.tshift`.
    pub name: String,
    /// The file on disk when it exists (a `.HEAD` or NIfTI for datasets),
    /// otherwise where it was expected.
    pub path: PathBuf,
    /// What it is for.
    pub role: ArtifactRole,
    /// Its space.
    pub space: Space,
    /// Is it a volume dataset (AFNI or NIfTI)?
    pub is_dataset: bool,
    /// Does the file exist?
    pub exists: bool,
    /// Can `afni-io` open it? `None` when not checked (NIfTI files, which
    /// are not read in full here, and non-datasets).
    pub openable: Option<bool>,
    /// Why it cannot be opened, when `openable` is `Some(false)`.
    pub open_error: Option<String>,
    /// Grid size from the header, when known.
    pub dims: Option<[usize; 3]>,
    /// Where the script says it is written.
    pub source: EvidenceSource,
}

/// The kind of a step, for labels and rules.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    /// `tcat`: the input data.
    Inputs,
    /// `outcount`: outlier fractions.
    Outliers,
    /// `tshift`: slice timing correction.
    SliceTiming,
    /// `volreg`: volume registration (motion correction).
    MotionCorrection,
    /// `align`: anatomical alignment.
    Alignment,
    /// `tlrc`: warp to a template.
    TemplateWarp,
    /// `blur`: spatial smoothing.
    Smoothing,
    /// `mask`: brain masks.
    Masking,
    /// `scale`: percent signal change scaling.
    Scaling,
    /// `regress`: the regression model.
    Regression,
    /// A block afniru has no special knowledge of (its own name is kept).
    Custom(String),
}

impl StepKind {
    /// The kind of an `afni_proc.py` block name.
    pub fn from_block(block: &str) -> Self {
        match block {
            "tcat" => StepKind::Inputs,
            "outcount" => StepKind::Outliers,
            "tshift" => StepKind::SliceTiming,
            "volreg" => StepKind::MotionCorrection,
            "align" => StepKind::Alignment,
            "tlrc" => StepKind::TemplateWarp,
            "blur" => StepKind::Smoothing,
            "mask" => StepKind::Masking,
            "scale" => StepKind::Scaling,
            "regress" => StepKind::Regression,
            other => StepKind::Custom(other.to_string()),
        }
    }

    /// Label in the rail.
    pub fn label(&self) -> String {
        match self {
            StepKind::Inputs => "Inputs".into(),
            StepKind::Outliers => "Outlier check".into(),
            StepKind::SliceTiming => "Slice timing".into(),
            StepKind::MotionCorrection => "Motion correction".into(),
            StepKind::Alignment => "Alignment".into(),
            StepKind::TemplateWarp => "Template warp".into(),
            StepKind::Smoothing => "Smoothing".into(),
            StepKind::Masking => "Masks".into(),
            StepKind::Scaling => "Scaling".into(),
            StepKind::Regression => "Regression".into(),
            StepKind::Custom(name) => {
                let mut c = name.chars();
                c.next()
                    .map(|f| f.to_uppercase().chain(c).collect())
                    .unwrap_or_default()
            }
        }
    }
}

/// A step's identity within a run: the block name, with `#2`, `#3`, ... for
/// blocks that appear more than once.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct StepId(pub String);

/// One executed block of the pipeline.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessingStep {
    /// Identity within the run.
    pub id: StepId,
    /// What kind of step.
    pub kind: StepKind,
    /// Label for the rail.
    pub label: String,
    /// The block name in the script.
    pub block: String,
    /// Line of the block's banner in the script (1-based).
    pub script_line: usize,
    /// What the step wrote.
    pub artifacts: Vec<StepArtifact>,
    /// Health, with all evidence.
    pub assessment: HealthAssessment,
}

impl ProcessingStep {
    /// Dataset artifacts that exist and that `afni-io` can (or may be able
    /// to) open: what "View" can show.
    pub fn viewable(&self) -> Vec<&StepArtifact> {
        self.artifacts
            .iter()
            .filter(|a| a.is_dataset && a.exists && a.openable != Some(false))
            .collect()
    }
}

/// A whole `afni_proc.py` run.
#[derive(Debug, Clone, PartialEq)]
pub struct ProcessingRun {
    /// The generating tool.
    pub tool: String,
    /// Subject id (the run's name).
    pub name: String,
    /// The `proc.<subj>` script.
    pub script: PathBuf,
    /// The results directory.
    pub results_dir: PathBuf,
    /// The `afni_proc.py` command that generated the script, if recorded.
    pub command: Option<String>,
    /// Steps in executed order.
    pub steps: Vec<ProcessingStep>,
    /// Things that were noticed but did not stop the parse.
    pub notes: Vec<String>,
}

impl ProcessingRun {
    /// The step with this id.
    pub fn step(&self, id: &StepId) -> Option<&ProcessingStep> {
        self.steps.iter().find(|s| &s.id == id)
    }

    /// Position of the step with this id.
    pub fn index_of(&self, id: &StepId) -> Option<usize> {
        self.steps.iter().position(|s| &s.id == id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn health_orders_worst_first() {
        let mut all = [
            Health::Good,
            Health::Unknown,
            Health::Failed,
            Health::Caution,
        ];
        all.sort();
        assert_eq!(
            all,
            [
                Health::Failed,
                Health::Caution,
                Health::Unknown,
                Health::Good
            ]
        );
    }

    #[test]
    fn block_names_map_to_kinds_and_labels() {
        assert_eq!(StepKind::from_block("volreg").label(), "Motion correction");
        assert_eq!(StepKind::from_block("tlrc"), StepKind::TemplateWarp);
        assert_eq!(StepKind::from_block("ricor").label(), "Ricor");
        assert_eq!(
            StepKind::from_block("custom_thing"),
            StepKind::Custom("custom_thing".into())
        );
    }
}
