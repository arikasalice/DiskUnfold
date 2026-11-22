//! A folder's type slices, for its row and for its shape (F06 README 54, #298).
//!
//! One function makes both, so a folder's bar, its rectangle and its table row show the
//! same slices.
use du_analysis_index::{
    ResultVersion,
    visualization::{folder_types, type_legend},
};
use du_core_contracts::{
    ItemRef, SizeBasis,
    visualization::{quantity::VizQuantity, type_slices::VizTypeSlice},
};

/// Why a folder's slices could not be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SlicesError {
    /// The caller asked to stop.
    Cancelled,
    /// The reference does not name an item of the version, or a total does not fit.
    Invalid,
}

/// The slices of an item drawn with `legend_class_key`, or `None` where they do not
/// apply.
///
/// They apply to a folder under the type color criterion, which gives a folder the
/// folder class, in a model without a filter: a filtered value holds only the kept
/// bytes, which the slices of the whole folder would exceed. Narrowed to one type, a
/// folder takes that type's class and has nothing to divide. See
/// [`folder_types::folder_types`] for the folders that have none.
pub(crate) fn folder_slices(
    version: &ResultVersion,
    item: &ItemRef,
    legend_class_key: &str,
    basis: &SizeBasis,
    filtered: bool,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<Vec<VizTypeSlice>>, SlicesError> {
    if filtered || legend_class_key != type_legend::FOLDER {
        return Ok(None);
    }
    let types = match folder_types::folder_types(version, item, basis, cancelled) {
        Ok(Some(types)) => types,
        Ok(None) => return Ok(None),
        Err(type_legend::TotalsError::Cancelled) => return Err(SlicesError::Cancelled),
        Err(type_legend::TotalsError::InvalidScope) => return Err(SlicesError::Invalid),
    };
    to_slices(types).map(Some)
}

/// The slices of the Other group of `members` (README 54.2), when `enabled`: the
/// presentation owner turns them on under the type criterion without a filter. Its
/// row and its shape take them from here.
pub(crate) fn group_slices(
    version: &ResultVersion,
    members: &[ItemRef],
    basis: &SizeBasis,
    enabled: bool,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<Option<Vec<VizTypeSlice>>, SlicesError> {
    if !enabled {
        return Ok(None);
    }
    match folder_types::group_types(version, members, basis, cancelled) {
        Ok(Some(types)) => to_slices(types).map(Some),
        Ok(None) => Ok(None),
        Err(type_legend::TotalsError::Cancelled) => Err(SlicesError::Cancelled),
        Err(type_legend::TotalsError::InvalidScope) => Err(SlicesError::Invalid),
    }
}

pub(crate) fn to_slices(
    types: folder_types::FolderTypes,
) -> Result<Vec<VizTypeSlice>, SlicesError> {
    let lower = types.below_unread;
    types
        .classes
        .into_iter()
        .map(|(key, total)| {
            let item_count = if lower {
                VizQuantity::lower_bound("items", total.count)
            } else {
                VizQuantity::exact("items", i128::from(total.count))
            };
            let bytes = if lower || total.lower_bound {
                VizQuantity::lower_bound("bytes", total.bytes)
            } else {
                VizQuantity::exact("bytes", i128::from(total.bytes))
            };
            Ok(VizTypeSlice {
                class_key: key.to_owned(),
                item_count: item_count.ok_or(SlicesError::Invalid)?,
                total: bytes.ok_or(SlicesError::Invalid)?,
            })
        })
        .collect::<Result<Vec<_>, _>>()
}
