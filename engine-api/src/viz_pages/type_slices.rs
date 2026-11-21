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
        .map(Some)
}
