//! Private model-cache identity, separate from the caller's condition fingerprint.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Error {
    Source,
    Cancelled,
}
use du_analysis_index::visualization::{
    geometry::{NoArea, Weight},
    histogram::{Basis, BinLayout, Histogram},
    live_models::{GroupRows, SizeRange},
};
use du_core_contracts::{result::ResultCompleteness, visualization::rows::VizRow};
use std::collections::HashMap;

/// A versioned, length-delimited encoding binds cache reuse to the actual candidate.
/// The supplied seed remains the caller's canonical condition fingerprint; this
/// derived key is private to ModelPages and must not replace the wire fingerprint.
pub(super) fn candidate_fingerprint(
    seed: &str,
    model: &Histogram,
    rows: &[VizRow],
    groups: &HashMap<String, GroupRows>,
    no_attribution: bool,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<String, Error> {
    if model.bins.len() != rows.len() || groups.len() != rows.len() {
        return Err(Error::Source);
    }
    let mut h = blake3::Hasher::new();
    bytes(&mut h, b"diskunfold.histogram-page-candidate.v2")?;
    bytes(&mut h, seed.as_bytes())?;
    byte(&mut h, u8::from(no_attribution));
    byte(
        &mut h,
        match model.basis {
            Basis::ItemCount => 0,
            Basis::Bytes => 1,
        },
    );
    byte(
        &mut h,
        match model.source_completeness {
            ResultCompleteness::Complete => 0,
            ResultCompleteness::Partial => 1,
            ResultCompleteness::Missing => 2,
            ResultCompleteness::Cancelled => 3,
        },
    );
    number(&mut h, model.observed_items);
    weight(&mut h, model.total_bytes);
    optional_number(&mut h, model.axis_max);
    byte(&mut h, u8::from(model.total_bytes_overflowed));
    numbers(&mut h, &model.uncertain_items, cancelled)?;
    numbers(&mut h, &model.unclassified_items, cancelled)?;
    number(&mut h, rows.len() as u64);
    for (bin, row) in model.bins.iter().zip(rows) {
        check(cancelled)?;
        bin_fields(&mut h, bin, cancelled)?;
        json(&mut h, row)?;
        let group = groups
            .get(row.group_ref.as_deref().ok_or(Error::Source)?)
            .ok_or(Error::Source)?;
        json(&mut h, &group.total)?;
        match &group.size_range {
            SizeRange::Unknown => byte(&mut h, 0),
            SizeRange::Known { min, max } => {
                byte(&mut h, 1);
                json(&mut h, min)?;
                json(&mut h, max)?;
            }
        }
        number(&mut h, group.rows.len() as u64);
        for member in &group.rows {
            check(cancelled)?;
            json(&mut h, member)?;
        }
    }
    check(cancelled)?;
    Ok(h.finalize().to_hex().to_string())
}
fn bin_fields(
    h: &mut blake3::Hasher,
    bin: &BinLayout,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Error> {
    number(h, bin.bin.lower);
    optional_number(h, bin.bin.upper);
    weight(h, bin.item_count);
    weight(h, bin.bytes);
    match bin.rect {
        Some(rect) => {
            byte(h, 1);
            for value in [rect.left, rect.top, rect.right, rect.bottom] {
                h.update(&value.to_le_bytes());
            }
        }
        None => byte(h, 0),
    }
    byte(
        h,
        match bin.no_area {
            None => 0,
            Some(NoArea::Unknown) => 1,
            Some(NoArea::ExactZero) => 2,
            Some(NoArea::LowerBoundZero) => 3,
            Some(NoArea::CoordinatePrecision) => 4,
            Some(NoArea::UncertainOffset) => 5,
            Some(NoArea::UncertainScale) => 6,
        },
    );
    byte(h, u8::from(bin.count_overflowed));
    byte(h, u8::from(bin.bytes_overflowed));
    numbers(h, &bin.members, cancelled)
}
fn numbers(
    h: &mut blake3::Hasher,
    values: &[u32],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Error> {
    number(h, values.len() as u64);
    for value in values {
        check(cancelled)?;
        h.update(&value.to_le_bytes());
    }
    Ok(())
}
fn weight(h: &mut blake3::Hasher, value: Weight) {
    match value {
        Weight::Exact(n) => {
            byte(h, 0);
            number(h, n);
        }
        Weight::LowerBound(n) => {
            byte(h, 1);
            number(h, n);
        }
        Weight::Unknown => byte(h, 2),
    }
}
fn optional_number(h: &mut blake3::Hasher, value: Option<u64>) {
    match value {
        Some(n) => {
            byte(h, 1);
            number(h, n);
        }
        None => byte(h, 0),
    }
}
fn byte(h: &mut blake3::Hasher, value: u8) {
    h.update(&[value]);
}
fn number(h: &mut blake3::Hasher, value: u64) {
    h.update(&value.to_le_bytes());
}
fn bytes(h: &mut blake3::Hasher, value: &[u8]) -> Result<(), Error> {
    number(h, u64::try_from(value.len()).map_err(|_| Error::Source)?);
    h.update(value);
    Ok(())
}
fn json<T: serde::Serialize>(h: &mut blake3::Hasher, value: &T) -> Result<(), Error> {
    bytes(h, &serde_json::to_vec(value).map_err(|_| Error::Source)?)
}
fn check(cancelled: &mut dyn FnMut() -> bool) -> Result<(), Error> {
    if cancelled() {
        Err(Error::Cancelled)
    } else {
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use du_analysis_index::visualization::histogram::Bin;
    use du_core_contracts::{
        SizeAmount, SizeBasis, SizeEvidence, SizeValue,
        visualization::{
            quantity::VizQuantity,
            rows::{VizDenominator, VizDenominatorBasis},
        },
    };
    #[test]
    fn member_hashing_is_cancellable_and_bin_changes_change_the_key() -> Result<(), Error> {
        let size = SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Unknown,
            amount: SizeAmount::Unknown,
        };
        let row = VizRow {
            item_ref: None,
            group_ref: Some("g".into()),
            display_name: "bin".into(),
            size: size.clone(),
            quantity: VizQuantity::exact("items", 4096),
            ratio: None,
            denominator: VizDenominator {
                basis: VizDenominatorBasis::Scope,
                value: VizQuantity::exact("items", 4096).ok_or(Error::Source)?,
                size: None,
            },
            legend_class_key: "c".into(),
            display_flags: vec![],
        };
        let groups = HashMap::from([(
            "g".into(),
            GroupRows {
                rows: vec![],
                total: size,
                size_range: SizeRange::Unknown,
            },
        )]);
        let bin = BinLayout {
            bin: Bin {
                lower: 0,
                upper: None,
            },
            members: (0..4096).collect(),
            item_count: Weight::Exact(4096),
            bytes: Weight::Unknown,
            rect: None,
            no_area: Some(NoArea::Unknown),
            count_overflowed: false,
            bytes_overflowed: false,
        };
        let mut model = Histogram {
            basis: Basis::ItemCount,
            bins: vec![bin],
            uncertain_items: vec![],
            unclassified_items: vec![],
            observed_items: 4096,
            total_bytes: Weight::Unknown,
            axis_max: None,
            source_completeness: ResultCompleteness::Complete,
            total_bytes_overflowed: false,
        };
        let rows = [row];
        let first =
            candidate_fingerprint("condition", &model, &rows, &groups, false, &mut || false)?;
        let fallback =
            candidate_fingerprint("condition", &model, &rows, &groups, true, &mut || false)?;
        assert_ne!(first, fallback);
        model.bins[0].bin.upper = Some(100);
        let second =
            candidate_fingerprint("condition", &model, &rows, &groups, false, &mut || false)?;
        assert_ne!(first, second);
        let mut checks = 0;
        assert_eq!(
            candidate_fingerprint("condition", &model, &rows, &groups, false, &mut || {
                checks += 1;
                checks == 1000
            }),
            Err(Error::Cancelled)
        );
        assert_eq!(checks, 1000);
        Ok(())
    }
}
