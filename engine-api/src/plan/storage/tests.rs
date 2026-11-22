use super::*;
use std::sync::atomic::{AtomicUsize, Ordering};

struct CountingStore(Arc<AtomicUsize>);

impl PlanVersionStore for CountingStore {
    fn append_fixed(&mut self, _: &PlanVersionRecord) -> Result<(), PortError> {
        self.0.fetch_add(1, Ordering::Relaxed);
        Ok(())
    }

    fn read(&self, _: &Id) -> Result<PlanVersionRecord, PortError> {
        Err(port_error(
            PortErrorKind::NotFound,
            "plan.version_not_found",
        ))
    }

    fn read_lineage(&self, _: &Id) -> Result<Vec<PlanVersionRecord>, PortError> {
        Ok(Vec::new())
    }
}

#[test]
fn non_current_readiness_has_a_wire_state() {
    assert_eq!(
        wire_readiness(DomainReadiness::SourceNotCurrent),
        PlanReadiness::SourceNotCurrent
    );
}

#[test]
fn oversized_serialized_plan_returns_a_plan_reason_without_writing() -> Result<(), &'static str> {
    let writes = Arc::new(AtomicUsize::new(0));
    let store: SharedVersionStore = Arc::new(Mutex::new(CountingStore(writes.clone())));
    let version = PlanVersion {
        version_id: Id::from_v7_bytes([1; 16]),
        lineage_id: Id::from_v7_bytes([2; 16]),
        parent_version_id: None,
        source_version_id: Id::from_v7_bytes([3; 16]),
        source_kind: du_change_planner::ports::PlanSourceKind::UserSelection,
        data_source: du_core_contracts::data_source::DataSource::Real,
        request_channel: du_change_planner::ports::RequestChannel::Gui,
        ruleset: None,
        items: Vec::new(),
        expected_effect: Vec::new(),
        confirmations: Vec::new(),
        readiness: DomainReadiness::Ready,
        hash_format: "\0".repeat(PLAN_BODY_MAX_BYTES / 6 + 1),
        hash: [4; 32],
        created_at: du_core_contracts::Timestamp::from_unix_millis(0)
            .ok_or("test timestamp must be valid")?,
    };

    assert_eq!(append(&store, &version), Err(PLAN_TOO_MANY_ITEMS));
    assert_eq!(writes.load(Ordering::Relaxed), 0);
    Ok(())
}
