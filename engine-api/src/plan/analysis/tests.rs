use super::map_folder_collection;
use du_analysis_index::folder_collection::{FolderBoundary, Unenumerated};
use du_change_planner::ports::{DescendantResult, Fact};
use du_core_contracts::protection_facts::fact_reason;

#[test]
fn unread_and_already_counted_folders_are_not_treated_as_complete() {
    assert_eq!(
        map_folder_collection(
            Unenumerated::Yes {
                reason_key: "scan.lower_bound_unexplained",
            },
            FolderBoundary::Unknown {
                reason_key: "space.data_insufficient",
            },
        ),
        Fact::Unknown(fact_reason::PARTIAL)
    );
    assert_eq!(
        map_folder_collection(
            Unenumerated::Yes {
                reason_key: "scan.cancelled",
            },
            FolderBoundary::Unknown {
                reason_key: "space.data_insufficient",
            },
        ),
        Fact::Unknown(fact_reason::CANCELLED)
    );
    assert_eq!(
        map_folder_collection(Unenumerated::No, FolderBoundary::AlreadyCounted),
        Fact::Known(DescendantResult::Incomplete)
    );
    assert_eq!(
        map_folder_collection(Unenumerated::No, FolderBoundary::NoBoundary),
        Fact::Known(DescendantResult::Clear)
    );
}
