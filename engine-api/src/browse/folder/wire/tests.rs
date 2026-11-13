use super::*;
use du_core_contracts::{Id, result::ResultCompleteness};

#[test]
fn out_of_range_counts_fail_without_a_partial_success_body() {
    let id = Id::from_v7_bytes([7; 16]);
    let too_large = u64::from(u32::MAX) + 1;
    for (total, unknown_count) in [
        (Total::Exact(too_large), 0),
        (Total::LowerBound(0), too_large),
    ] {
        let page = FolderPage {
            query_ref: id.clone(),
            result_version: id.clone(),
            rows: vec![],
            total,
            unknown_count,
            observed_at: None,
            completeness: ResultCompleteness::Partial,
            next_position: None,
        };
        assert_eq!(folder_page(page, None), Err(internal()));
    }
    assert_eq!(count(u64::from(u32::MAX)), Ok(u32::MAX));
}
