//! Health rules: which checks exist, their thresholds, and how results combine.
//!
//! Principles:
//!
//! * **Evidence, not guesses.** Every check names its source (a file or a
//!   header) and states its rule. A check that cannot be evaluated because
//!   its source is missing is [`Health::Unknown`], never `Good`.
//! * **No hiding.** A step's state is the worst of its checks
//!   ([`combine`]); all checks stay available.
//! * **Whose thresholds.** Where AFNI itself raises a warning (its warning
//!   files), afniru reports exactly that. The numeric limits below are afniru
//!   defaults, chosen from AFNI's own review guidance; they are not AFNI
//!   standards.
//!
//! The rules are listed with their thresholds in `docs/PROCESSING_RAIL.md`.

use super::review::{Review, Warnings};
use super::{
    ArtifactRole, EvidenceSource, Health, HealthAssessment, HealthEvidence, ProcessingStep,
    SourceKind, StepKind,
};

/// Mask Dice below this is a caution (AFNI: "closer to 0 than 1 might flag
/// alignment failure").
pub const DICE_CAUTION: f64 = 0.8;
/// Mask Dice below this is a failure.
pub const DICE_FAILED: f64 = 0.5;
/// A censor fraction above this is a caution.
pub const CENSOR_CAUTION: f64 = 0.10;
/// A censor fraction above this is a failure.
pub const CENSOR_FAILED: f64 = 0.30;
/// Degrees of freedom left below this is a caution (AFNI: "a small value
/// suggests over-modeling").
pub const DOF_CAUTION: f64 = 50.0;

/// Measurements gathered from the results directory.
#[derive(Debug, Clone, Default)]
pub struct Facts {
    /// `out.ss_review.*.txt`.
    pub review: Option<Review>,
    /// `out.pre_ss_warn.txt`.
    pub pre_ss_warn: Option<Warnings>,
    /// `out.4095_warn.txt`.
    pub warn_4095: Option<Warnings>,
    /// `out.cormat_warn.txt`.
    pub cormat_warn: Option<Warnings>,
}

/// The state of a set of checks: the worst one; `Unknown` for none.
pub fn combine(evidence: &[HealthEvidence]) -> Health {
    evidence
        .iter()
        .map(|e| e.health)
        .min()
        .unwrap_or(Health::Unknown)
}

/// Assess every step in place.
pub fn assess(steps: &mut [ProcessingStep], facts: &Facts) {
    let produced: Vec<bool> = steps
        .iter()
        .map(|s| s.artifacts.iter().any(|a| a.is_dataset && a.exists))
        .collect();
    for (i, step) in steps.iter_mut().enumerate() {
        let later_output = produced[i + 1..].iter().any(|p| *p);
        let mut evidence = Vec::new();
        evidence.extend(present(step, later_output));
        evidence.extend(readable(step));
        evidence.extend(grids(step));
        match step.kind {
            StepKind::Inputs => {
                evidence.push(warning_check(
                    "inputs.pre_steady_state",
                    "Pre-steady-state",
                    facts.pre_ss_warn.as_ref(),
                    "out.pre_ss_warn.txt",
                    "Caution if AFNI wrote any pre-steady-state warning",
                ));
                evidence.push(warning_check(
                    "inputs.saturation_4095",
                    "4095 saturation",
                    facts.warn_4095.as_ref(),
                    "out.4095_warn.txt",
                    "Caution if AFNI wrote any 4095-saturation warning",
                ));
            }
            StepKind::Alignment => evidence.push(dice_check(
                "align.mask_dice",
                "EPI/anat mask overlap",
                facts.review.as_ref(),
                "anat/EPI mask Dice coef",
            )),
            StepKind::TemplateWarp => evidence.push(dice_check(
                "warp.mask_dice",
                "Anat/template mask overlap",
                facts.review.as_ref(),
                "anat/templ mask Dice coef",
            )),
            StepKind::Regression => {
                evidence.push(dof_check(facts.review.as_ref()));
                evidence.push(censor_check(facts.review.as_ref()));
                evidence.push(warning_check(
                    "regress.cormat_warnings",
                    "Regressor correlation",
                    facts.cormat_warn.as_ref(),
                    "out.cormat_warn.txt",
                    "Caution if AFNI wrote any correlation warning",
                ));
            }
            _ => {}
        }
        if evidence.is_empty() {
            evidence.push(HealthEvidence {
                check: "none",
                title: "No checks".into(),
                health: Health::Unknown,
                finding: "No outputs or measurements are recorded for this step".into(),
                rule: "A step needs at least one recorded output or measurement to be assessed"
                    .into(),
                source: source(
                    SourceKind::GeneratedCommand,
                    format!("script line {}", step.script_line),
                ),
            });
        }
        step.assessment = HealthAssessment {
            health: combine(&evidence),
            evidence,
        };
    }
}

fn source(kind: SourceKind, detail: impl Into<String>) -> EvidenceSource {
    EvidenceSource {
        kind,
        detail: detail.into(),
    }
}

/// Which of the step's declared outputs exist.
fn present(step: &ProcessingStep, later_output: bool) -> Option<HealthEvidence> {
    let expected: Vec<_> = step
        .artifacts
        .iter()
        .filter(|a| a.role != ArtifactRole::Report)
        .collect();
    if expected.is_empty() {
        return None;
    }
    // Each missing output with the script line that declares it, so the cause
    // can be found in the script (an empty name is shown as "").
    let missing: Vec<String> = expected
        .iter()
        .filter(|a| !a.exists)
        .map(|a| format!("\"{}\" from {}", a.name, a.source.detail))
        .collect();
    let total = expected.len();
    let (health, finding) = if missing.is_empty() {
        (Health::Good, format!("all {total} outputs present"))
    } else if later_output {
        (
            Health::Failed,
            format!(
                "{} of {total} outputs missing ({}), but later steps produced output",
                missing.len(),
                list(&missing)
            ),
        )
    } else {
        (
            Health::Unknown,
            format!(
                "{} of {total} outputs not there yet ({}): not run, or still running",
                missing.len(),
                list(&missing)
            ),
        )
    };
    Some(HealthEvidence {
        check: "outputs.present",
        title: "Outputs present".into(),
        health,
        finding,
        rule: "Failed if an output is missing although a later step wrote output; Unknown if nothing later exists yet".into(),
        source: source(SourceKind::FileSystem, format!("results directory, script line {}", step.script_line)),
    })
}

/// At most three names, then a count.
fn list<S: AsRef<str>>(names: &[S]) -> String {
    let mut s = names
        .iter()
        .take(3)
        .map(AsRef::as_ref)
        .collect::<Vec<_>>()
        .join(", ");
    if names.len() > 3 {
        s.push_str(&format!(", +{} more", names.len() - 3));
    }
    s
}

/// Whether the existing AFNI datasets of the step open.
fn readable(step: &ProcessingStep) -> Option<HealthEvidence> {
    let checked: Vec<_> = step
        .artifacts
        .iter()
        .filter(|a| a.exists && a.openable.is_some())
        .collect();
    if checked.is_empty() {
        return None;
    }
    let bad: Vec<_> = checked
        .iter()
        .filter(|a| a.openable == Some(false))
        .collect();
    let (health, finding, detail) = match bad.first() {
        None => (
            Health::Good,
            format!("all {} datasets open", checked.len()),
            "dataset headers".to_string(),
        ),
        Some(first) => (
            Health::Failed,
            format!(
                "{} of {} datasets cannot be opened: {}{}",
                bad.len(),
                checked.len(),
                first.name,
                first
                    .open_error
                    .as_deref()
                    .map_or(String::new(), |e| format!(" ({e})"))
            ),
            first.path.display().to_string(),
        ),
    };
    Some(HealthEvidence {
        check: "outputs.readable",
        title: "Datasets open".into(),
        health,
        finding,
        rule: "Failed if afni-io cannot read a dataset's header or BRIK".into(),
        source: source(SourceKind::DatasetHeader, detail),
    })
}

/// Whether the step's EPI runs share a grid.
fn grids(step: &ProcessingStep) -> Option<HealthEvidence> {
    let epi: Vec<_> = step
        .artifacts
        .iter()
        .filter(|a| a.role == ArtifactRole::Epi && a.exists)
        .filter_map(|a| a.dims.map(|d| (a, d)))
        .collect();
    if epi.len() < 2 {
        return None;
    }
    let (first, dims) = epi[0];
    let odd = epi.iter().find(|(_, d)| *d != dims);
    let (health, finding) = match odd {
        None => (
            Health::Good,
            format!(
                "{} EPI datasets share a {}×{}×{} grid",
                epi.len(),
                dims[0],
                dims[1],
                dims[2]
            ),
        ),
        Some((a, d)) => (
            Health::Failed,
            format!(
                "{} is {}×{}×{} but {} is {}×{}×{}",
                first.name, dims[0], dims[1], dims[2], a.name, d[0], d[1], d[2]
            ),
        ),
    };
    Some(HealthEvidence {
        check: "grid.runs",
        title: "Runs share a grid".into(),
        health,
        finding,
        rule: "Failed if EPI datasets of one step have different grid sizes".into(),
        source: source(SourceKind::DatasetHeader, "DATASET_DIMENSIONS of each run"),
    })
}

/// A check driven by AFNI's own warning file.
fn warning_check(
    check: &'static str,
    title: &str,
    warnings: Option<&Warnings>,
    file: &str,
    rule: &str,
) -> HealthEvidence {
    let (health, finding, detail) = match warnings {
        None => (
            Health::Unknown,
            format!("not checked: {file} not found"),
            file.to_string(),
        ),
        Some(w) if w.lines.is_empty() => (Health::Good, "no warnings".to_string(), w.file.clone()),
        Some(w) => (
            Health::Caution,
            format!(
                "{} warning{}: {}",
                w.lines.len(),
                if w.lines.len() == 1 { "" } else { "s" },
                w.lines[0]
            ),
            w.file.clone(),
        ),
    };
    HealthEvidence {
        check,
        title: title.into(),
        health,
        finding,
        rule: rule.into(),
        source: source(SourceKind::QcArtifact, detail),
    }
}

fn review_source(review: Option<&Review>, key: &str) -> EvidenceSource {
    let file = review.map_or("out.ss_review.*.txt", |r| r.file.as_str());
    source(SourceKind::ReviewValue, format!("{file}: {key}"))
}

fn dice_check(
    check: &'static str,
    title: &str,
    review: Option<&Review>,
    key: &str,
) -> HealthEvidence {
    let rule = format!(
        "Caution below {DICE_CAUTION}, failed below {DICE_FAILED} (afniru defaults; AFNI: a low value may flag alignment failure)"
    );
    let (health, finding) = match review.and_then(|r| r.number(key)) {
        None => (
            Health::Unknown,
            format!("not measured: no \"{key}\" in the review file"),
        ),
        Some(v) if v < DICE_FAILED => {
            (Health::Failed, format!("Dice {v:.2} (below {DICE_FAILED})"))
        }
        Some(v) if v < DICE_CAUTION => (
            Health::Caution,
            format!("Dice {v:.2} (below {DICE_CAUTION})"),
        ),
        Some(v) => (Health::Good, format!("Dice {v:.2}")),
    };
    HealthEvidence {
        check,
        title: title.into(),
        health,
        finding,
        rule,
        source: review_source(review, key),
    }
}

fn dof_check(review: Option<&Review>) -> HealthEvidence {
    let key = "degrees of freedom left";
    let rule = format!(
        "Failed at 0 or fewer, caution below {DOF_CAUTION} (afniru defaults; AFNI: small values suggest over-modeling)"
    );
    let (health, finding) = match review.and_then(|r| r.number(key)) {
        None => (
            Health::Unknown,
            format!("not measured: no \"{key}\" in the review file"),
        ),
        Some(v) if v <= 0.0 => (Health::Failed, format!("{v} degrees of freedom left")),
        Some(v) if v < DOF_CAUTION => {
            (Health::Caution, format!("only {v} degrees of freedom left"))
        }
        Some(v) => (Health::Good, format!("{v} degrees of freedom left")),
    };
    HealthEvidence {
        check: "regress.dof_left",
        title: "Degrees of freedom".into(),
        health,
        finding,
        rule,
        source: review_source(review, key),
    }
}

fn censor_check(review: Option<&Review>) -> HealthEvidence {
    let key = "censor fraction";
    let rule = format!(
        "Caution above {CENSOR_CAUTION}, failed above {CENSOR_FAILED} (afniru defaults; AFNI: worth considering for subject omission)"
    );
    let (health, finding) = match review.and_then(|r| r.number(key)) {
        None => (
            Health::Unknown,
            format!("not measured: no \"{key}\" in the review file"),
        ),
        Some(v) if v > CENSOR_FAILED => {
            (Health::Failed, format!("{:.1}% of TRs censored", v * 100.0))
        }
        Some(v) if v > CENSOR_CAUTION => (
            Health::Caution,
            format!("{:.1}% of TRs censored", v * 100.0),
        ),
        Some(v) => (Health::Good, format!("{:.1}% of TRs censored", v * 100.0)),
    };
    HealthEvidence {
        check: "regress.censor_fraction",
        title: "Censored TRs".into(),
        health,
        finding,
        rule,
        source: review_source(review, key),
    }
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use super::*;
    use crate::processing::{ProcessingStep, Space, StepArtifact, StepId};

    fn src() -> EvidenceSource {
        source(SourceKind::GeneratedCommand, "proc:1")
    }

    fn artifact(name: &str, exists: bool) -> StepArtifact {
        StepArtifact {
            name: name.into(),
            path: PathBuf::from(name),
            role: ArtifactRole::Epi,
            space: Space::Orig,
            is_dataset: true,
            exists,
            openable: exists.then_some(true),
            open_error: None,
            dims: exists.then_some([4, 5, 6]),
            source: src(),
        }
    }

    fn step(block: &str, artifacts: Vec<StepArtifact>) -> ProcessingStep {
        ProcessingStep {
            id: StepId(block.into()),
            kind: StepKind::from_block(block),
            label: StepKind::from_block(block).label(),
            block: block.into(),
            script_line: 1,
            artifacts,
            assessment: HealthAssessment {
                health: Health::Unknown,
                evidence: vec![],
            },
        }
    }

    fn review(text: &str) -> Review {
        Review::parse("out.ss_review.s.txt", text)
    }

    fn health_of(steps: &[ProcessingStep], block: &str) -> Health {
        steps
            .iter()
            .find(|s| s.block == block)
            .unwrap()
            .assessment
            .health
    }

    #[test]
    fn combine_is_the_worst_and_never_hides_a_failure() {
        let ev = |h| HealthEvidence {
            check: "x",
            title: "x".into(),
            health: h,
            finding: "f".into(),
            rule: "r".into(),
            source: src(),
        };
        assert_eq!(combine(&[]), Health::Unknown);
        assert_eq!(combine(&[ev(Health::Good), ev(Health::Good)]), Health::Good);
        assert_eq!(
            combine(&[ev(Health::Good), ev(Health::Unknown)]),
            Health::Unknown
        );
        assert_eq!(
            combine(&[ev(Health::Unknown), ev(Health::Caution)]),
            Health::Caution
        );
        let many_green = vec![ev(Health::Good); 20];
        let mut with_red = many_green.clone();
        with_red.insert(7, ev(Health::Failed));
        assert_eq!(combine(&with_red), Health::Failed);
    }

    #[test]
    fn present_outputs_are_good() {
        let mut steps = vec![step(
            "tshift",
            vec![artifact("a", true), artifact("b", true)],
        )];
        assess(&mut steps, &Facts::default());
        assert_eq!(health_of(&steps, "tshift"), Health::Good);
        assert!(
            steps[0]
                .assessment
                .evidence
                .iter()
                .any(|e| e.check == "outputs.readable")
        );
    }

    #[test]
    fn a_missing_output_before_later_output_is_failed() {
        let mut steps = vec![
            step("blur", vec![artifact("pb03", false)]),
            step("scale", vec![artifact("pb04", true)]),
        ];
        assess(&mut steps, &Facts::default());
        assert_eq!(health_of(&steps, "blur"), Health::Failed);
        assert!(
            steps[0]
                .assessment
                .reason()
                .contains("later steps produced output")
        );
    }

    #[test]
    fn missing_output_with_nothing_later_is_unknown_not_failed() {
        let mut steps = vec![
            step("tshift", vec![artifact("pb01", true)]),
            step("blur", vec![artifact("pb03", false)]),
            step("scale", vec![artifact("pb04", false)]),
        ];
        assess(&mut steps, &Facts::default());
        assert_eq!(health_of(&steps, "tshift"), Health::Good);
        assert_eq!(health_of(&steps, "blur"), Health::Unknown);
        assert_eq!(health_of(&steps, "scale"), Health::Unknown);
    }

    #[test]
    fn unreadable_dataset_is_failed() {
        let mut bad = artifact("pb01", true);
        bad.openable = Some(false);
        bad.open_error = Some("bad header".into());
        let mut steps = vec![step("tshift", vec![bad])];
        assess(&mut steps, &Facts::default());
        assert_eq!(health_of(&steps, "tshift"), Health::Failed);
        assert!(steps[0].assessment.reason().contains("bad header"));
    }

    #[test]
    fn mismatched_run_grids_are_failed() {
        let mut other = artifact("pb01.r02", true);
        other.dims = Some([4, 5, 7]);
        let mut steps = vec![step("tshift", vec![artifact("pb01.r01", true), other])];
        assess(&mut steps, &Facts::default());
        assert_eq!(health_of(&steps, "tshift"), Health::Failed);
        assert!(steps[0].assessment.reason().contains("4×5×6"));
    }

    #[test]
    fn warning_files_decide_inputs() {
        let w = |lines: &str| Some(Warnings::parse("out.pre_ss_warn.txt", lines));
        let run = |pre: Option<Warnings>| {
            let mut steps = vec![step("tcat", vec![artifact("pb00", true)])];
            let facts = Facts {
                pre_ss_warn: pre,
                warn_4095: Some(Warnings::parse("out.4095_warn.txt", "")),
                ..Facts::default()
            };
            assess(&mut steps, &facts);
            steps[0].assessment.health
        };
        assert_eq!(run(w("")), Health::Good);
        assert_eq!(run(w("** TR #0 outliers")), Health::Caution);
        assert_eq!(run(None), Health::Unknown); // missing evidence is not Good
    }

    #[test]
    fn dice_thresholds() {
        let run = |text: &str| {
            let mut steps = vec![step("align", vec![])];
            assess(
                &mut steps,
                &Facts {
                    review: Some(review(text)),
                    ..Facts::default()
                },
            );
            steps[0].assessment.health
        };
        assert_eq!(run("anat/EPI mask Dice coef : 0.94"), Health::Good);
        assert_eq!(run("anat/EPI mask Dice coef : 0.80"), Health::Good);
        assert_eq!(run("anat/EPI mask Dice coef : 0.79"), Health::Caution);
        assert_eq!(run("anat/EPI mask Dice coef : 0.50"), Health::Caution);
        assert_eq!(run("anat/EPI mask Dice coef : 0.49"), Health::Failed);
        assert_eq!(run("TR : 2"), Health::Unknown);
    }

    #[test]
    fn template_warp_uses_the_template_dice() {
        let mut steps = vec![step("tlrc", vec![])];
        let facts = Facts {
            review: Some(review(
                "anat/templ mask Dice coef : 0.3\nanat/EPI mask Dice coef : 0.99",
            )),
            ..Facts::default()
        };
        assess(&mut steps, &facts);
        assert_eq!(steps[0].assessment.health, Health::Failed);
    }

    #[test]
    fn regression_combines_dof_censoring_and_correlation_warnings() {
        let run = |text: &str, cormat: &str| {
            let mut steps = vec![step("regress", vec![])];
            let facts = Facts {
                review: Some(review(text)),
                cormat_warn: Some(Warnings::parse("out.cormat_warn.txt", cormat)),
                ..Facts::default()
            };
            assess(&mut steps, &facts);
            steps.remove(0).assessment
        };
        let good = "degrees of freedom left : 270\ncensor fraction : 0.03";
        assert_eq!(run(good, "").health, Health::Good);
        assert_eq!(
            run("degrees of freedom left : 20\ncensor fraction : 0.03", "").health,
            Health::Caution
        );
        assert_eq!(
            run("degrees of freedom left : 0\ncensor fraction : 0.03", "").health,
            Health::Failed
        );
        assert_eq!(
            run("degrees of freedom left : 270\ncensor fraction : 0.2", "").health,
            Health::Caution
        );
        assert_eq!(
            run("degrees of freedom left : 270\ncensor fraction : 0.4", "").health,
            Health::Failed
        );
        assert_eq!(
            run(good, "warning: high correlation").health,
            Health::Caution
        );
        // A failure among three checks decides, and the others stay listed.
        let a = run("degrees of freedom left : -3\ncensor fraction : 0.03", "");
        assert_eq!(a.health, Health::Failed);
        assert_eq!(a.evidence.len(), 3);
        assert!(a.reason().contains("-3"));
    }

    #[test]
    fn a_step_with_no_evidence_is_unknown_and_says_so() {
        let mut steps = vec![step("mything", vec![])];
        assess(&mut steps, &Facts::default());
        assert_eq!(steps[0].assessment.health, Health::Unknown);
        assert_eq!(steps[0].assessment.evidence[0].check, "none");
    }

    #[test]
    fn every_evidence_names_its_source_and_rule() {
        let mut steps = vec![step("regress", vec![artifact("stats", true)])];
        assess(&mut steps, &Facts::default());
        for e in &steps[0].assessment.evidence {
            assert!(!e.source.detail.is_empty(), "{}", e.check);
            assert!(!e.rule.is_empty(), "{}", e.check);
        }
    }

    #[test]
    fn list_truncates_long_name_lists() {
        assert_eq!(list(&["a", "b"]), "a, b");
        assert_eq!(list(&["a", "b", "c", "d", "e"]), "a, b, c, +2 more");
    }
}
