//! Voxel-value text for readouts.

/// A voxel value for the readout: integers plain, otherwise four decimals
/// (scientific notation for tiny values), NaN as `NaN`.
pub fn format_value(v: f32) -> String {
    if !v.is_finite() {
        return v.to_string();
    }
    if v.fract() == 0.0 && v.abs() < 1e9 {
        return format!("{v:.0}");
    }
    if v != 0.0 && v.abs() < 1e-3 {
        return format!("{v:.3e}");
    }
    let s = format!("{v:.4}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// A p- or q-value: `0.25`, `0.05`, `0.01`, `0.001`, `0.0024`, and below 0.001
/// scientific, `4.0e-04`, `1.0e-12`; `--` for NaN. The values everyone uses as
/// cutoffs (.05, .01, .001) are never written in scientific notation.
pub fn format_p(v: f64) -> String {
    if v.is_nan() {
        return "--".to_string();
    }
    if v >= 0.1 {
        return format!("{v:.2}");
    }
    // Two significant digits in plain decimals, down to 0.001 (a value that
    // rounds to 0.0010 counts, so a threshold set from p = 0.001 reads 0.001).
    if v >= 0.00095 {
        let decimals = if v >= 0.01 { 3 } else { 4 };
        let text = format!("{v:.decimals$}");
        return text.trim_end_matches('0').to_string();
    }
    let text = format!("{v:.1e}"); // like "2.4e-3"
    match text
        .split_once('e')
        .and_then(|(m, e)| Some((m, e.parse::<i32>().ok()?)))
    {
        Some((m, e)) => format!("{m}e{}{:02}", if e < 0 { '-' } else { '+' }, e.abs()),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn value_formatting() {
        assert_eq!(format_value(612.0), "612");
        assert_eq!(format_value(0.8353935), "0.8354");
        assert_eq!(format_value(1.5), "1.5");
        assert_eq!(format_value(0.0001234), "1.234e-4");
        assert_eq!(format_value(f32::NAN), "NaN");
        assert_eq!(format_value(-3.0), "-3");
    }

    #[test]
    fn the_usual_cutoffs_are_never_scientific() {
        for (p, text) in [
            (0.05, "0.05"),
            (0.01, "0.01"),
            (0.001, "0.001"),
            (0.1, "0.10"),
            (0.5, "0.50"),
            // What a threshold computed from p = .001 gives back.
            (0.0010000000000000002, "0.001"),
            (0.000999999, "0.001"),
            (0.0099999, "0.01"),
            (0.0512, "0.051"),
            (0.0123, "0.012"),
            (0.0056, "0.0056"),
        ] {
            assert_eq!(format_p(p), text, "{p}");
        }
        // Below 0.001 it is scientific.
        assert_eq!(format_p(0.0009), "9.0e-04");
        assert_eq!(format_p(0.0001), "1.0e-04");
    }

    #[test]
    fn p_values_use_two_digit_exponents() {
        assert_eq!(format_p(0.00242029), "0.0024");
        assert_eq!(format_p(1.0e-12), "1.0e-12");
        assert_eq!(format_p(0.0477969), "0.048");
        assert_eq!(format_p(0.25), "0.25");
        assert_eq!(format_p(1.0), "1.00");
        assert_eq!(format_p(f64::NAN), "--");
        assert_eq!(format_p(2.8e-100), "2.8e-100");
    }
}
