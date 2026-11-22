//! Checks of a model's rows against the result version they were built from.
use super::{Error, ModelInput};
use du_core_contracts::{result::ResultVersionRead, visualization::rows::VizRow};
pub(super) fn validate_input_cancellable(
    input: &ModelInput,
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Error> {
    if input.condition_fingerprint.is_empty() || input.groups.keys().any(String::is_empty) {
        return Err(Error::InvalidModel);
    }
    validate_rows_cancellable(input, &input.rows, cancelled)?;
    for group in input.groups.values() {
        validate_rows_cancellable(input, &group.rows, cancelled)?;
    }
    Ok(())
}
pub(super) fn validate_rows_cancellable(
    input: &ModelInput,
    rows: &[VizRow],
    cancelled: &mut dyn FnMut() -> bool,
) -> Result<(), Error> {
    if rows.len() as u64 >= (1_u64 << 53) {
        return Err(Error::InvalidModel);
    }
    for row in rows {
        if cancelled() {
            return Err(Error::Cancelled);
        }
        row.validate_revision(input.version.version_id())
            .map_err(|_| Error::InvalidModel)?;
        if row.size.basis != input.size_basis {
            return Err(Error::InvalidModel);
        }
        if let Some(item) = &row.item_ref
            && input
                .version
                .items()
                .get(item.item_no as usize)
                .is_none_or(|found| found.item_no != item.item_no)
        {
            return Err(Error::InvalidModel);
        }
        if row
            .group_ref
            .as_ref()
            .is_some_and(|group| !input.groups.contains_key(group))
        {
            return Err(Error::InvalidModel);
        }
    }
    Ok(())
}
