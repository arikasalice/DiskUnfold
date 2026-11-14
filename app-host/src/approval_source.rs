//! Data source check for approval requests (FR4.HD2.app-host.DD1).

use du_core_contracts::{data_source::DataSource, sample_ipc::APPROVAL_SAMPLE_SOURCE_DENIED};

/// Checks the data source of the plan a caller wants reviewed or approved.
///
/// `found` is the source recorded on the requested plan, or `None` when the plan
/// could not be found. A missing or sample plan is denied with
/// `approval.sample_source_denied`, so no approval state is created for it. The
/// host runs this as the first row of approval review and grant, before any other check.
pub fn check_request_plan_source(found: Option<DataSource>) -> Result<(), &'static str> {
    match found {
        Some(DataSource::Real) => Ok(()),
        Some(DataSource::Sample) | None => Err(APPROVAL_SAMPLE_SOURCE_DENIED),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_a_found_real_plan_passes() {
        let rows = [
            (Some(DataSource::Real), Ok(())),
            (
                Some(DataSource::Sample),
                Err("approval.sample_source_denied"),
            ),
            (None, Err("approval.sample_source_denied")),
        ];
        for (found, expected) in rows {
            assert_eq!(check_request_plan_source(found), expected, "{found:?}");
        }
    }
}
