//! The remainder of a model's totals: what the scope holds beyond the drawn, grouped and
//! filtered-out totals, and the causes it names.
//!
//! The producer never publishes a remainder it cannot explain. An exact remainder is the
//! scope minus the three totals; a difference that is negative or that is not zero without
//! a known cause stops the model. A value that is not exact makes the remainder unknown,
//! because a difference of lower bounds is not a value.
use super::internal;
use crate::error::EngineError;
use du_analysis_index::visualization::projection::RemainderCauses;
use du_core_contracts::{
    SizeAmount, SizeEvidence, SizeValue,
    visualization::{
        model::{REMAINDER_TOO_SMALL, REMAINDER_UNATTRIBUTED, VizTotal},
        quantity::{VizAmount, VizQuantity},
    },
};

/// The remainder and the keys of what it holds.
pub(super) struct Remainder {
    pub total: VizTotal,
    /// Omitted for a zero remainder; at least one key for an exact remainder that is not zero.
    pub reason_keys: Option<Vec<String>>,
}

/// Computes the remainder of the totals of one model.
///
/// `unknown_reason` is the reason carried by the quantity when the remainder is unknown.
/// An unknown remainder lists the causes the producer knows, or none.
pub(super) fn compute(
    scope: &VizTotal,
    parts: [&VizTotal; 3],
    causes: RemainderCauses,
    unknown_reason: &str,
) -> Result<Remainder, EngineError> {
    let known = keys(causes);
    match exact_difference(scope, parts) {
        Some(rest) if rest < 0 => Err(internal()),
        Some(0) => Ok(Remainder {
            total: exact(scope, 0)?,
            reason_keys: None,
        }),
        // A remainder that nothing explains is a mistake of the producer.
        Some(_) if known.is_empty() => Err(internal()),
        Some(rest) => Ok(Remainder {
            total: exact(scope, rest)?,
            reason_keys: Some(known),
        }),
        None => Ok(Remainder {
            total: unknown(scope, unknown_reason)?,
            reason_keys: (!known.is_empty()).then_some(known),
        }),
    }
}

/// The scope minus the three totals, when all four are exact and in one unit.
fn exact_difference(scope: &VizTotal, parts: [&VizTotal; 3]) -> Option<i128> {
    let unit = scope.quantity.unit();
    let value = |total: &VizTotal| match total.quantity.amount() {
        VizAmount::Exact { value } if total.quantity.unit() == unit => Some(value.get()),
        _ => None,
    };
    let mut rest = value(scope)?;
    for part in parts {
        rest = rest.checked_sub(value(part)?)?;
    }
    Some(rest)
}

/// The cause keys in the sorted order the contract requires.
fn keys(causes: RemainderCauses) -> Vec<String> {
    let mut keys = Vec::new();
    if causes.too_small {
        keys.push(REMAINDER_TOO_SMALL.to_owned());
    }
    if causes.unattributed {
        keys.push(REMAINDER_UNATTRIBUTED.to_owned());
    }
    keys.sort_unstable();
    keys
}

fn exact(scope: &VizTotal, value: i128) -> Result<VizTotal, EngineError> {
    let source_size = match &scope.source_size {
        Some(size) => Some(SizeValue {
            basis: size.basis.clone(),
            evidence: size.evidence.clone(),
            amount: SizeAmount::Exact {
                bytes: u64::try_from(value).map_err(|_| internal())?.into(),
            },
        }),
        None => None,
    };
    Ok(VizTotal {
        quantity: VizQuantity::exact(scope.quantity.unit(), value).ok_or_else(internal)?,
        source_size,
    })
}

fn unknown(scope: &VizTotal, reason: &str) -> Result<VizTotal, EngineError> {
    Ok(VizTotal {
        quantity: VizQuantity::unknown(scope.quantity.unit(), reason.to_owned())
            .ok_or_else(internal)?,
        source_size: scope.source_size.as_ref().map(|size| SizeValue {
            basis: size.basis.clone(),
            evidence: SizeEvidence::Unknown,
            amount: SizeAmount::Unknown,
        }),
    })
}

#[cfg(test)]
mod tests;
