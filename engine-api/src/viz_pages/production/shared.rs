//! Sizes and denominators shared by the page row producers.
use super::Error;
use du_analysis_index::ResultVersion;
use du_core_contracts::{
    ItemRef, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultItemKind, ResultVersionRead},
    visualization::{
        quantity::VizQuantity,
        rows::{VizDenominator, VizDenominatorBasis},
    },
};

pub(super) fn denominator(size: &SizeValue, basis: &SizeBasis) -> Result<VizDenominator, Error> {
    if &size.basis != basis {
        return Err(Error::SourceMismatch);
    }
    Ok(VizDenominator {
        basis: VizDenominatorBasis::Scope,
        value: quantity(size)?,
        size: Some(size.clone()),
    })
}
pub(super) fn original_size(
    version: &ResultVersion,
    item_no: u32,
    basis: &SizeBasis,
) -> Option<SizeValue> {
    let mut size = version.item_space_size(item_no, basis.clone())?;
    if shared_counted_item(version, item_no, basis)
        .ok()?
        .is_some_and(|counted| counted.item_no != item_no)
    {
        size = zero_non_counted_size(size);
    }
    Some(size)
}

pub(super) fn shared_counted_item(
    version: &ResultVersion,
    item_no: u32,
    basis: &SizeBasis,
) -> Result<Option<ItemRef>, Error> {
    if *basis != SizeBasis::Allocated
        || !version
            .items()
            .get(item_no as usize)
            .is_some_and(|item| item.kind == ResultItemKind::File)
    {
        return Ok(None);
    }
    version
        .shared_counted_item(&ItemRef {
            result_version: version.version_id().clone(),
            item_no,
        })
        .map_err(|_| Error::SourceMismatch)
}

pub(super) fn zero_non_counted_size(mut size: SizeValue) -> SizeValue {
    size.amount = match size.amount {
        SizeAmount::Exact { .. } => SizeAmount::Exact { bytes: 0.into() },
        SizeAmount::LowerBound { .. } => SizeAmount::LowerBound { bytes: 0.into() },
        SizeAmount::Unknown => SizeAmount::Unknown,
    };
    size
}

pub(super) fn quantity(size: &SizeValue) -> Result<VizQuantity, Error> {
    let bytes = |bytes: &du_core_contracts::ByteSize| Ok::<_, Error>(bytes.get());
    let value = match &size.amount {
        SizeAmount::Exact { bytes: value } if size.evidence != SizeEvidence::Unknown => {
            VizQuantity::exact("bytes", i128::from(bytes(value)?))
        }
        SizeAmount::Exact { .. } => return Err(Error::SourceMismatch),
        SizeAmount::LowerBound { bytes: value } => VizQuantity::lower_bound("bytes", bytes(value)?),
        _ => VizQuantity::unknown("bytes", "viz.unavailable.no_attribution".into()),
    };
    value.ok_or(Error::InvalidQuantity)
}
