use super::*;

#[test]
fn descriptor_is_cancellable_compute_only_and_unrecorded() {
    let descriptor = descriptor();
    assert_eq!(descriptor.kind_id, "scan.sample");
    assert_eq!(descriptor.version, 1);
    assert_eq!(descriptor.display_name_key, "job.kind.sample_load");
    assert_eq!(descriptor.controls, [JobControl::Cancel]);
    assert_eq!(descriptor.safe_points, ["sample.observation_sealed"]);
    assert_eq!(descriptor.progress_unit, ProgressUnit::Items);
    assert_eq!(descriptor.resource_classes, [ResourceClass::Compute]);
    assert_eq!(descriptor.history, HistoryMode::None);
}
