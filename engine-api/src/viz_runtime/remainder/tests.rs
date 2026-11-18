use super::*;
use du_core_contracts::SizeBasis;

type TestResult = Result<(), String>;

const UNKNOWN: &str = "space.data_insufficient";

fn size(amount: SizeAmount) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::Measured,
        amount,
    }
}

fn exact_bytes(bytes: u64) -> Result<VizTotal, String> {
    Ok(VizTotal {
        quantity: VizQuantity::exact("bytes", i128::from(bytes)).ok_or("quantity")?,
        source_size: Some(size(SizeAmount::Exact {
            bytes: bytes.into(),
        })),
    })
}

fn lower_bytes(bytes: u64) -> Result<VizTotal, String> {
    Ok(VizTotal {
        quantity: VizQuantity::lower_bound("bytes", bytes).ok_or("quantity")?,
        source_size: Some(size(SizeAmount::LowerBound {
            bytes: bytes.into(),
        })),
    })
}

fn items(count: i128) -> Result<VizTotal, String> {
    Ok(VizTotal {
        quantity: VizQuantity::exact("items", count).ok_or("quantity")?,
        source_size: None,
    })
}

const NONE: RemainderCauses = RemainderCauses {
    too_small: false,
    unattributed: false,
};
const SMALL: RemainderCauses = RemainderCauses {
    too_small: true,
    unattributed: false,
};
const BOTH: RemainderCauses = RemainderCauses {
    too_small: true,
    unattributed: true,
};

fn run(
    scope: &VizTotal,
    parts: [&VizTotal; 3],
    causes: RemainderCauses,
) -> Result<Remainder, EngineError> {
    compute(scope, parts, causes, UNKNOWN)
}

fn keys_of(remainder: &Remainder) -> Vec<&str> {
    remainder
        .reason_keys
        .iter()
        .flatten()
        .map(String::as_str)
        .collect()
}

#[test]
fn an_exact_remainder_is_the_difference_with_the_evidence_of_the_scope() -> TestResult {
    let (scope, drawn, grouped, filtered) = (
        exact_bytes(100)?,
        exact_bytes(60)?,
        exact_bytes(30)?,
        exact_bytes(0)?,
    );
    let remainder =
        run(&scope, [&drawn, &grouped, &filtered], SMALL).map_err(|e| format!("{e:?}"))?;
    assert_eq!(remainder.total, exact_bytes(10)?);
    assert_eq!(keys_of(&remainder), [REMAINDER_TOO_SMALL]);
    Ok(())
}

#[test]
fn a_zero_remainder_names_no_cause() -> TestResult {
    let (scope, drawn, zero) = (exact_bytes(100)?, exact_bytes(100)?, exact_bytes(0)?);
    let remainder = run(&scope, [&drawn, &zero, &zero], BOTH).map_err(|e| format!("{e:?}"))?;
    assert_eq!(remainder.total, exact_bytes(0)?);
    assert_eq!(remainder.reason_keys, None);
    Ok(())
}

#[test]
fn a_remainder_that_nothing_explains_stops_the_model() -> TestResult {
    let (scope, drawn, zero) = (exact_bytes(100)?, exact_bytes(60)?, exact_bytes(0)?);
    assert!(run(&scope, [&drawn, &zero, &zero], NONE).is_err());
    Ok(())
}

#[test]
fn a_negative_difference_stops_the_model_whatever_the_causes() -> TestResult {
    let (scope, drawn, grouped, zero) = (
        exact_bytes(100)?,
        exact_bytes(60)?,
        exact_bytes(50)?,
        exact_bytes(0)?,
    );
    assert!(run(&scope, [&drawn, &grouped, &zero], BOTH).is_err());
    Ok(())
}

#[test]
fn both_causes_are_listed_in_the_sorted_order_of_the_contract() -> TestResult {
    let (scope, drawn, zero) = (exact_bytes(100)?, exact_bytes(60)?, exact_bytes(0)?);
    let remainder = run(&scope, [&drawn, &zero, &zero], BOTH).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        keys_of(&remainder),
        [REMAINDER_TOO_SMALL, REMAINDER_UNATTRIBUTED]
    );
    Ok(())
}

#[test]
fn a_lower_bound_makes_the_remainder_unknown_and_never_a_difference_of_bounds() -> TestResult {
    let (exact, lower, zero) = (exact_bytes(100)?, lower_bytes(60)?, exact_bytes(0)?);
    let unknown = |remainder: &Remainder| -> TestResult {
        assert_eq!(remainder.total.quantity.unit(), "bytes");
        assert_eq!(
            remainder.total.quantity.amount(),
            &VizAmount::Unknown {
                reason_key: UNKNOWN.to_owned()
            }
        );
        let source = remainder
            .total
            .source_size
            .as_ref()
            .ok_or("no source size")?;
        assert_eq!(source.amount, SizeAmount::Unknown);
        assert_eq!(source.basis, SizeBasis::Logical);
        Ok(())
    };
    // A lower-bound part, a lower-bound scope and an unknown part.
    unknown(&run(&exact, [&lower, &zero, &zero], NONE).map_err(|e| format!("{e:?}"))?)?;
    unknown(&run(&lower, [&zero, &zero, &zero], NONE).map_err(|e| format!("{e:?}"))?)?;
    let missing = VizTotal {
        quantity: VizQuantity::unknown("bytes", UNKNOWN.into()).ok_or("quantity")?,
        source_size: Some(size(SizeAmount::Unknown)),
    };
    unknown(&run(&exact, [&zero, &missing, &zero], NONE).map_err(|e| format!("{e:?}"))?)?;
    Ok(())
}

#[test]
fn an_unknown_remainder_names_the_causes_it_knows_or_none() -> TestResult {
    let (exact, lower, zero) = (exact_bytes(100)?, lower_bytes(60)?, exact_bytes(0)?);
    let known = run(&exact, [&lower, &zero, &zero], SMALL).map_err(|e| format!("{e:?}"))?;
    assert_eq!(keys_of(&known), [REMAINDER_TOO_SMALL]);
    let none = run(&exact, [&lower, &zero, &zero], NONE).map_err(|e| format!("{e:?}"))?;
    assert_eq!(none.reason_keys, None);
    Ok(())
}

#[test]
fn totals_of_a_model_that_counts_items_are_subtracted_in_items() -> TestResult {
    let (scope, grouped, zero) = (items(10)?, items(8)?, items(0)?);
    let remainder = run(&scope, [&zero, &grouped, &zero], BOTH).map_err(|e| format!("{e:?}"))?;
    assert_eq!(remainder.total, items(2)?);
    // Bytes and items are never subtracted from each other.
    let bytes = exact_bytes(5)?;
    let mixed = run(&scope, [&bytes, &zero, &zero], NONE).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        mixed.total.quantity.amount(),
        &VizAmount::Unknown {
            reason_key: UNKNOWN.to_owned()
        }
    );
    Ok(())
}
