//! Reading AFNI's own quality-control text files.
//!
//! * `out.ss_review.<subj>.txt`, written by `@ss_review_basic`: lines of
//!   `label : value`, with the label padded to a column (for example
//!   `censor fraction           : 0.033`). Parsed into keys and values, kept
//!   exactly as written.
//! * Warning files such as `out.pre_ss_warn.txt`: empty when all is well;
//!   every non-blank line is one warning.

/// The parsed `out.ss_review` file.
#[derive(Debug, Clone, PartialEq)]
pub struct Review {
    /// File name, for citing as a source.
    pub file: String,
    /// `(label, value)` in file order.
    pub values: Vec<(String, String)>,
}

impl Review {
    /// Parse the text of an `out.ss_review` file. Lines without a colon are
    /// ignored.
    pub fn parse(file: &str, text: &str) -> Self {
        let values = text
            .lines()
            .filter_map(|l| {
                let (key, value) = l.split_once(':')?;
                let key = key.trim();
                (!key.is_empty()).then(|| (key.to_string(), value.trim().to_string()))
            })
            .collect();
        Self {
            file: file.to_string(),
            values,
        }
    }

    /// The value of `key`, as written.
    pub fn get(&self, key: &str) -> Option<&str> {
        self.values
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    /// The first number in the value of `key`.
    pub fn number(&self, key: &str) -> Option<f64> {
        self.get(key)?.split_whitespace().next()?.parse().ok()
    }
}

/// A warning file: its name and the warnings in it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Warnings {
    /// File name, for citing as a source.
    pub file: String,
    /// Non-blank lines.
    pub lines: Vec<String>,
}

impl Warnings {
    /// Parse the text of a warning file.
    pub fn parse(file: &str, text: &str) -> Self {
        Self {
            file: file.to_string(),
            lines: text
                .lines()
                .map(str::trim)
                .filter(|l| !l.is_empty())
                .map(String::from)
                .collect(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\
subject ID                : sub-01
TR                        : 2
motion limit              : 0.3
censor fraction           : 0.033333
degrees of freedom left   : 270
anat/EPI mask Dice coef   : 0.94 (some note: here)
final anatomy dset        : anat_final.sub-01+tlrc.HEAD

no colon on this line
";

    #[test]
    fn parses_label_value_lines() {
        let r = Review::parse("out.ss_review.sub-01.txt", SAMPLE);
        assert_eq!(r.get("subject ID"), Some("sub-01"));
        assert_eq!(
            r.get("final anatomy dset"),
            Some("anat_final.sub-01+tlrc.HEAD")
        );
        assert_eq!(r.get("missing"), None);
        assert_eq!(r.values.len(), 7);
    }

    #[test]
    fn numbers_take_the_first_token() {
        let r = Review::parse("f", SAMPLE);
        assert_eq!(r.number("censor fraction"), Some(0.033333));
        assert_eq!(r.number("anat/EPI mask Dice coef"), Some(0.94));
        assert_eq!(r.number("subject ID"), None);
    }

    #[test]
    fn warnings_keep_only_non_blank_lines() {
        let w = Warnings::parse(
            "out.pre_ss_warn.txt",
            "\n** TR #0 outliers: possible pre-steady state TRs in run 01\n  \n",
        );
        assert_eq!(
            w.lines,
            ["** TR #0 outliers: possible pre-steady state TRs in run 01"]
        );
        assert!(Warnings::parse("f", "").lines.is_empty());
    }
}
