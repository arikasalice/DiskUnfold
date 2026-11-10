use du_analysis_index::visualization::ratio::calculate;
use du_core_contracts::visualization::quantity::VizQuantity;

#[test]
fn rounds_the_magnitude_and_preserves_sign_and_exact_zero() -> Result<(), &'static str> {
    for (numerator, denominator, expected) in [
        (1, 3, "0.33333333333333333333"),
        (1, 6, "0.16666666666666666667"),
        (-1, 6, "-0.16666666666666666667"),
        (-5, 2, "-2.5"),
        (0, 3, "0"),
        (60, 3, "20"),
        (1, 2_097_152, "0.00000047683715820313"),
    ] {
        let numerator = VizQuantity::exact("bytes", numerator).ok_or("invalid numerator")?;
        let denominator = VizQuantity::exact("bytes", denominator).ok_or("invalid denominator")?;
        assert_eq!(
            calculate(&numerator, &denominator).as_deref(),
            Some(expected)
        );
    }
    Ok(())
}

#[test]
fn retains_a_nonzero_ratio_at_the_limits_of_the_wire_range() -> Result<(), &'static str> {
    let max = VizQuantity::exact("bytes", u64::MAX.into()).ok_or("invalid maximum")?;
    let one = VizQuantity::exact("bytes", 1).ok_or("invalid one")?;
    let negative = VizQuantity::exact("bytes", -1).ok_or("invalid negative one")?;
    assert_eq!(
        calculate(&one, &max).as_deref(),
        Some("0.00000000000000000005")
    );
    assert_eq!(
        calculate(&negative, &max).as_deref(),
        Some("-0.00000000000000000005")
    );
    assert_eq!(
        calculate(&max, &one).as_deref(),
        Some("18446744073709551615")
    );
    Ok(())
}

#[test]
fn declines_inexact_incompatible_and_nonpositive_operands() -> Result<(), &'static str> {
    let exact = VizQuantity::exact("bytes", 10).ok_or("invalid exact")?;
    let unknown =
        VizQuantity::unknown("bytes", "viz.unavailable".into()).ok_or("invalid unknown")?;
    let lower = VizQuantity::lower_bound("bytes", 1).ok_or("invalid lower bound")?;
    let items = VizQuantity::exact("items", 10).ok_or("invalid item count")?;
    let zero = VizQuantity::exact("bytes", 0).ok_or("invalid zero")?;
    let negative = VizQuantity::exact("bytes", -1).ok_or("invalid negative")?;
    for inexact in [&unknown, &lower, &items] {
        assert_eq!(calculate(inexact, &exact), None);
        assert_eq!(calculate(&exact, inexact), None);
    }
    for invalid_denominator in [&zero, &negative] {
        assert_eq!(calculate(&exact, invalid_denominator), None);
    }
    Ok(())
}
