mod fixtures;

use du_change_planner::{
    api::{MAX_PLAN_ITEMS, PlanError},
    ports::{PlanSourceKind, RequestChannel},
    readiness::{PlanReadiness, readiness_first_row},
};
use du_core_contracts::data_source::DataSource;
use fixtures::{FakePorts, request, selection};

#[test]
fn fr1_hd1_app_host_dd1_rejects_empty_and_oversized_requests_before_reading_items() {
    let fake = FakePorts::standard();
    assert!(matches!(
        fake.planner().build(request(&fake, Vec::new())),
        Err(PlanError::Empty)
    ));

    let mut oversized = request(
        &fake,
        (0..=MAX_PLAN_ITEMS)
            .map(|index| selection(&fake, index as u32 + 1))
            .collect(),
    );
    assert!(matches!(
        fake.planner().build(oversized.clone()),
        Err(PlanError::TooManyItems)
    ));

    oversized.selections.pop();
    assert_eq!(oversized.selections.len(), MAX_PLAN_ITEMS);
}

#[test]
fn fr1_hd1_app_host_dd1_fixes_the_exact_item_limit_and_sample_readiness() {
    let fake = FakePorts::standard();
    let plan = fake.planner().build(request(
        &fake,
        (0..MAX_PLAN_ITEMS)
            .map(|index| selection(&fake, index as u32 + 1))
            .collect(),
    ));
    assert!(plan.is_ok());
    let Ok(plan) = plan else {
        return;
    };
    assert_eq!(plan.items.len(), MAX_PLAN_ITEMS);
    assert_eq!(plan.readiness, PlanReadiness::Ready);

    let mut sample = FakePorts::standard();
    sample.source = DataSource::Sample;
    let sample_plan = sample
        .planner()
        .build(request(&sample, vec![selection(&sample, 1)]));
    assert!(sample_plan.is_ok());
    if let Ok(plan) = sample_plan {
        assert_eq!(plan.readiness, PlanReadiness::SamplePlan);
    }

    let mut snapshot = FakePorts::standard();
    snapshot.source = DataSource::Snapshot;
    assert_eq!(
        readiness_first_row(Some(DataSource::Sample)),
        Some(PlanReadiness::SamplePlan)
    );
    assert_eq!(
        readiness_first_row(Some(DataSource::Snapshot)),
        Some(PlanReadiness::SourceNotCurrent)
    );
    assert!(matches!(
        snapshot
            .planner()
            .build(request(&snapshot, vec![selection(&snapshot, 1)])),
        Err(PlanError::SourceNotCurrent)
    ));
}

#[test]
fn fr1_hd1_app_host_dd1_restricts_non_manual_sources_to_the_gui() {
    let fake = FakePorts::standard();
    let mut plan_request = request(&fake, vec![selection(&fake, 1)]);
    plan_request.source_kind = PlanSourceKind::Scenario;
    plan_request.request_channel = RequestChannel::Mcp;
    assert!(matches!(
        fake.planner().build(plan_request),
        Err(PlanError::SourceChannelDenied)
    ));
}
