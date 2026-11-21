//! Fixed in-memory sizes for the F03 result build and sealed result.

#[test]
#[ignore = "measurement: run in release mode with --ignored --nocapture"]
fn memory_layout_measure() {
    use std::mem::size_of;

    println!(
        "F03_MEMORY_LAYOUT pointer={} node={} stored_observation={} result_item={} \
         optional_size={} optional_physical_arc={} space_observation={} timestamp={} \
         optional_timestamp={} root_index={} range_indices={} space_aggregate={} \
         accumulator={} result_version={} folder_marks={}",
        size_of::<usize>(),
        crate::observation_store::node_size_bytes(),
        size_of::<crate::StoredObservation>(),
        size_of::<du_core_contracts::result::ResultItem>(),
        size_of::<Option<du_core_contracts::SizeValue>>(),
        size_of::<Option<std::sync::Arc<crate::SpaceObservation>>>(),
        size_of::<crate::SpaceObservation>(),
        size_of::<du_core_contracts::Timestamp>(),
        size_of::<Option<du_core_contracts::Timestamp>>(),
        size_of::<u32>(),
        size_of::<crate::observation_range::RangeIndices>(),
        size_of::<du_core_contracts::result::SpaceAggregate>(),
        crate::space::accumulator_size_bytes(),
        size_of::<crate::ResultVersion>(),
        size_of::<crate::folder_marks::FolderMarks>(),
    );
}
