use super::*;
use crate::visualization::quantity::VizQuantity;

type TestResult = Result<(), &'static str>;

fn total(quantity: Option<VizQuantity>) -> Result<VizTotal, &'static str> {
    Ok(VizTotal {
        quantity: quantity.ok_or("invalid quantity")?,
        source_size: None,
    })
}

fn bytes(value: i128) -> Result<VizTotal, &'static str> {
    total(VizQuantity::exact("bytes", value))
}

fn items(value: i128) -> Result<VizTotal, &'static str> {
    total(VizQuantity::exact("items", value))
}

fn at_least(value: u64) -> Result<VizTotal, &'static str> {
    total(VizQuantity::lower_bound("bytes", value))
}

fn unknown() -> Result<VizTotal, &'static str> {
    total(VizQuantity::unknown(
        "bytes",
        "space.data_insufficient".into(),
    ))
}

/// Exact byte totals; `remainder` is absent when it is `None`.
fn totals(
    scope: i128,
    parts: [i128; 3],
    remainder: Option<i128>,
) -> Result<VizTotals, &'static str> {
    Ok(VizTotals {
        scope: bytes(scope)?,
        drawn: bytes(parts[0])?,
        grouped: bytes(parts[1])?,
        filtered_out: bytes(parts[2])?,
        remainder: remainder.map(bytes).transpose()?,
        remainder_reason_keys: None,
        scope_measurement_limits: None,
    })
}

fn with_keys(totals: VizTotals, keys: &[&str]) -> VizTotals {
    VizTotals {
        remainder_reason_keys: Some(keys.iter().map(|key| (*key).to_owned()).collect()),
        ..totals
    }
}

const ADD_UP: Result<(), &str> = Err("totals do not add up to the scope");
const EXCEED: Result<(), &str> = Err("totals exceed the scope");
const BAD_KEYS: Result<(), &str> = Err("invalid remainder reason keys");

#[test]
fn the_registered_keys_are_pinned() {
    assert_eq!(REMAINDER_TOO_SMALL, "viz.remainder.too_small");
    assert_eq!(REMAINDER_UNATTRIBUTED, "viz.remainder.unattributed");
}

#[test]
fn exact_parts_and_remainder_add_up_to_the_scope() -> TestResult {
    assert_eq!(
        totals(100, [60, 30, 0], Some(10))?.validate_breakdown(),
        Ok(())
    );
    assert_eq!(
        totals(100, [100, 0, 0], Some(0))?.validate_breakdown(),
        Ok(())
    );
    assert_eq!(
        totals(100, [60, 30, 5], Some(5))?.validate_breakdown(),
        Ok(())
    );
    assert_eq!(
        totals(100, [60, 30, 0], Some(5))?.validate_breakdown(),
        ADD_UP
    );
    assert_eq!(
        totals(100, [60, 30, 0], Some(11))?.validate_breakdown(),
        ADD_UP
    );
    Ok(())
}

#[test]
fn without_a_remainder_the_parts_may_leave_a_gap_but_not_exceed_the_scope() -> TestResult {
    assert_eq!(totals(100, [60, 30, 0], None)?.validate_breakdown(), Ok(()));
    assert_eq!(totals(100, [100, 0, 0], None)?.validate_breakdown(), Ok(()));
    assert_eq!(totals(100, [60, 50, 0], None)?.validate_breakdown(), EXCEED);
    assert_eq!(
        totals(100, [60, 30, 11], None)?.validate_breakdown(),
        EXCEED
    );
    Ok(())
}

#[test]
fn a_lower_bound_or_an_unknown_value_switches_the_sum_check_off() -> TestResult {
    let exact = totals(100, [60, 30, 0], Some(5))?;
    // The scope is a lower bound: the parts may exceed it.
    let lower_scope = VizTotals {
        scope: at_least(100)?,
        drawn: bytes(200)?,
        ..totals(0, [0, 0, 0], None)?
    };
    assert_eq!(lower_scope.validate_breakdown(), Ok(()));
    for part in 0..4 {
        for value in [at_least(1)?, unknown()?] {
            let mut mixed = exact.clone();
            match part {
                0 => mixed.drawn = value,
                1 => mixed.grouped = value,
                2 => mixed.filtered_out = value,
                _ => mixed.remainder = Some(value),
            }
            assert_eq!(mixed.validate_breakdown(), Ok(()), "part {part}");
        }
    }
    let mut unknown_scope = exact.clone();
    unknown_scope.scope = unknown()?;
    assert_eq!(unknown_scope.validate_breakdown(), Ok(()));
    Ok(())
}

#[test]
fn quantities_of_different_units_are_not_added() -> TestResult {
    let mixed = VizTotals {
        drawn: items(500)?,
        grouped: items(3)?,
        filtered_out: items(0)?,
        ..totals(100, [0, 0, 0], Some(7))?
    };
    assert_eq!(mixed.validate_breakdown(), Ok(()));
    // A model that counts items checks only its own unit.
    let counts =
        |scope, parts: [i128; 3], remainder: Option<i128>| -> Result<VizTotals, &'static str> {
            Ok(VizTotals {
                scope: items(scope)?,
                drawn: items(parts[0])?,
                grouped: items(parts[1])?,
                filtered_out: items(parts[2])?,
                remainder: remainder.map(items).transpose()?,
                remainder_reason_keys: None,
                scope_measurement_limits: None,
            })
        };
    assert_eq!(counts(10, [0, 8, 0], Some(2))?.validate_breakdown(), Ok(()));
    assert_eq!(counts(10, [0, 8, 0], Some(3))?.validate_breakdown(), ADD_UP);
    assert_eq!(counts(10, [0, 8, 0], None)?.validate_breakdown(), Ok(()));
    assert_eq!(counts(10, [0, 11, 0], None)?.validate_breakdown(), EXCEED);
    Ok(())
}

#[test]
fn an_exact_negative_remainder_is_rejected() -> TestResult {
    let negative = "negative remainder";
    // The five values add up, but the parts exceed the scope.
    assert_eq!(
        totals(100, [60, 50, 0], Some(-10))?.validate_breakdown(),
        Err(negative)
    );
    let lower_scope = VizTotals {
        scope: at_least(100)?,
        ..totals(0, [0, 0, 0], Some(-1))?
    };
    assert_eq!(lower_scope.validate_breakdown(), Err(negative));
    Ok(())
}

#[test]
fn the_reason_keys_need_a_remainder_and_must_be_sorted_and_unique() -> TestResult {
    let with_remainder = || totals(100, [60, 30, 0], Some(10));
    let both = [REMAINDER_TOO_SMALL, REMAINDER_UNATTRIBUTED];
    assert_eq!(
        with_keys(with_remainder()?, &both).validate_breakdown(),
        Ok(())
    );
    assert_eq!(
        with_keys(with_remainder()?, &[REMAINDER_UNATTRIBUTED]).validate_breakdown(),
        Ok(())
    );
    // An empty list says nothing and is valid; a producer omits it.
    assert_eq!(
        with_keys(with_remainder()?, &[]).validate_breakdown(),
        Ok(())
    );
    // A key that is not registered is kept.
    assert_eq!(
        with_keys(
            with_remainder()?,
            &["viz.remainder.future", REMAINDER_TOO_SMALL]
        )
        .validate_breakdown(),
        Ok(())
    );
    let reversed = [REMAINDER_UNATTRIBUTED, REMAINDER_TOO_SMALL];
    assert_eq!(
        with_keys(with_remainder()?, &reversed).validate_breakdown(),
        BAD_KEYS
    );
    let duplicate = [REMAINDER_TOO_SMALL, REMAINDER_TOO_SMALL];
    assert_eq!(
        with_keys(with_remainder()?, &duplicate).validate_breakdown(),
        BAD_KEYS
    );
    assert_eq!(
        with_keys(with_remainder()?, &[""]).validate_breakdown(),
        BAD_KEYS
    );
    assert_eq!(
        with_keys(totals(100, [60, 30, 0], None)?, &both).validate_breakdown(),
        BAD_KEYS
    );
    // An unknown remainder may name the causes that are known.
    let unknown_remainder = VizTotals {
        remainder: Some(unknown()?),
        ..totals(100, [60, 30, 0], None)?
    };
    assert_eq!(
        with_keys(unknown_remainder, &[REMAINDER_TOO_SMALL]).validate_breakdown(),
        Ok(())
    );
    Ok(())
}

#[test]
fn scope_measurement_limits_are_present_only_with_distinct_keys() -> TestResult {
    let limits = |keys: &[&str]| -> Result<VizTotals, &'static str> {
        Ok(VizTotals {
            scope_measurement_limits: Some(keys.iter().map(|key| (*key).to_owned()).collect()),
            ..totals(10, [10, 0, 0], None)?
        })
    };
    // F03's order and an unknown key are kept.
    limits(&["space.later_key", "space.allocated_may_include_shared"])?.validate_breakdown()?;
    let bad = Err("invalid scope measurement limits");
    assert_eq!(limits(&[])?.validate_breakdown(), bad);
    assert_eq!(limits(&["space.a", "space.a"])?.validate_breakdown(), bad);
    assert_eq!(limits(&[""])?.validate_breakdown(), bad);
    // Omitted rather than null.
    let mut wire = serde_json::to_value(totals(10, [10, 0, 0], None)?).map_err(|_| "json")?;
    assert!(wire.get("scopeMeasurementLimits").is_none());
    wire["scopeMeasurementLimits"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<VizTotals>(wire).is_err());
    Ok(())
}
