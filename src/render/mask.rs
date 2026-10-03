//! Mask rules: turning a `3dcalc` expression and the values of its variables
//! into on/off.
//!
//! Pure: the caller supplies one column of numbers per variable letter (one
//! entry per voxel); a voxel is **on** where the expression is not zero, as in
//! `3dcalc` masks (`step(a-3)*step(b-2)`).

use afni_core::calc::Expr;

/// Evaluate `expr` at `n` voxels. `columns` gives each used variable's values
/// (letter and one number per voxel); a used variable without a column reads
/// as 0. A result that is zero or NaN is off.
pub fn evaluate_rule(expr: &Expr, columns: &[(char, Vec<f64>)], n: usize) -> Vec<bool> {
    let mut vars = [0.0_f64; 26];
    let slots: Vec<(usize, &[f64])> = columns
        .iter()
        .filter(|(c, v)| c.is_ascii_lowercase() && v.len() >= n)
        .map(|(c, v)| ((*c as u8 - b'a') as usize, v.as_slice()))
        .collect();
    let mut stack = Vec::new();
    (0..n)
        .map(|i| {
            for (slot, column) in &slots {
                vars[*slot] = column[i];
            }
            let r = expr.eval_vars(&vars, &mut stack);
            r != 0.0 && !r.is_nan()
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cols(a: &[f64], b: &[f64]) -> Vec<(char, Vec<f64>)> {
        vec![('a', a.to_vec()), ('b', b.to_vec())]
    }

    #[test]
    fn a_conjunction_is_on_only_where_both_are() {
        let e = Expr::parse("step(a-3)*step(b-2)").unwrap();
        let on = evaluate_rule(&e, &cols(&[4.0, 4.0, 1.0, 3.0], &[3.0, 1.0, 3.0, 3.0]), 4);
        assert_eq!(on, [true, false, false, false]); // step(0) = 0: strictly greater
    }

    #[test]
    fn c_style_rules_give_the_same_masks_as_the_function_forms() {
        let c = cols(&[4.0, 4.0, 1.0, 3.0], &[3.0, 1.0, 3.0, 3.0]);
        let on = |t: &str| evaluate_rule(&Expr::parse(t).unwrap(), &c, 4);
        assert_eq!(on("a>3 && b>2"), [true, false, false, false]);
        assert_eq!(on("a>3 || b>2"), [true, true, true, true]);
        assert_eq!(on("a>3 || b>3"), [true, true, false, false]);
        assert_eq!(on("a>3 && !(b>2)"), [false, true, false, false]);
        assert_eq!(on("a>=3 && b<=2"), [false, true, false, false]);
        assert_eq!(on("a==b"), [false, false, false, true]);
        assert_eq!(on("a>b ? 1 : 0"), [true, true, false, false]);
    }

    #[test]
    fn a_union_and_a_difference() {
        let c = cols(&[4.0, 4.0, 1.0, 1.0], &[3.0, 1.0, 3.0, 1.0]);
        let either = Expr::parse("or(step(a-3),step(b-2))").unwrap();
        assert_eq!(evaluate_rule(&either, &c, 4), [true, true, true, false]);
        let a_not_b = Expr::parse("step(a-3)*(1-step(b-2))").unwrap();
        assert_eq!(evaluate_rule(&a_not_b, &c, 4), [false, true, false, false]);
    }

    #[test]
    fn nonzero_results_are_on_and_nan_is_off() {
        let e = Expr::parse("a").unwrap();
        let on = evaluate_rule(&e, &cols(&[0.0, -2.0, 0.5, f64::NAN], &[]), 4);
        assert_eq!(on, [false, true, true, false]);
    }

    #[test]
    fn a_missing_column_reads_as_zero() {
        let e = Expr::parse("a+1").unwrap();
        assert_eq!(evaluate_rule(&e, &[], 2), [true, true]);
        let e = Expr::parse("a").unwrap();
        assert_eq!(evaluate_rule(&e, &[], 2), [false, false]);
    }

    #[test]
    fn a_short_column_is_ignored_rather_than_read_past_its_end() {
        let e = Expr::parse("a").unwrap();
        assert_eq!(
            evaluate_rule(&e, &[('a', vec![1.0])], 3),
            [false, false, false]
        );
    }

    #[test]
    fn no_voxels_gives_nothing() {
        let e = Expr::parse("1").unwrap();
        assert!(evaluate_rule(&e, &[], 0).is_empty());
    }
}
