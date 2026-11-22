use std::sync::{Arc, Mutex};

use du_change_planner::{
    api::PlanBuilder,
    ports::{
        PlanAction, PlanItem, PlanItemState, PlanVersion, PlanVersionPort, ProtectionRulesetPort,
        RestoreRestriction, RulesetSnapshot,
    },
    readiness::PlanReadiness as DomainReadiness,
};
use du_core_contracts::{
    Id,
    plan_ipc::{
        PLAN_SOURCE_NOT_CURRENT, PLAN_TOO_MANY_ITEMS, PlanGetRequest, PlanGetResponse, PlanHash,
        PlanHistoryEntry, PlanHistoryRequest, PlanHistoryResponse, PlanItemDisposition,
        PlanItemReasonResponse, PlanReadiness, PlanSection, PlanVersionFixed, PlanVersionHead,
        PlanVersionItem,
    },
    plan_version_store::{
        PLAN_BODY_MAX_BYTES, PlanOriginChannel, PlanReadiness as StoredReadiness,
        PlanVersionRecord, PlanVersionStore,
    },
    ports::{PortError, PortErrorKind},
    ruleset::{RulesetRead, RulesetReadError},
};
use serde_json::{Value, json};

pub type SharedVersionStore = Arc<Mutex<dyn PlanVersionStore + Send>>;

const BODY_FORMAT: u64 = 1;
const PAGE_SIZE: usize = 100;

/// Adapts the raw app-db ruleset reader to the fail-closed planner port.
pub struct RulesetAdapter(Arc<dyn RulesetRead + Send + Sync>);

impl RulesetAdapter {
    /// Uses the app-db ruleset read port without taking storage ownership.
    pub fn new(reader: Arc<dyn RulesetRead + Send + Sync>) -> Self {
        Self(reader)
    }
}

impl ProtectionRulesetPort for RulesetAdapter {
    fn read_current(&self) -> Result<RulesetSnapshot, PortError> {
        let row = self.0.read_current().map_err(ruleset_read_error)?;
        match row {
            None => Err(port_error(
                PortErrorKind::NotFound,
                "protect.ruleset_unavailable",
            )),
            // The raw row contract has no canonical body validator or hash algorithm.
            // Do not turn an unverified policy into an allow decision.
            Some(_) => Err(port_error(
                PortErrorKind::SchemaMismatch,
                "protect.ruleset_unreadable",
            )),
        }
    }
}

/// Reads and verifies stored plan versions before exposing their contents.
pub struct VersionReader(pub(super) SharedVersionStore);

impl PlanVersionPort for VersionReader {
    fn read(&self, version_id: &Id) -> Result<PlanVersion, PortError> {
        let record = self
            .0
            .lock()
            .map_err(|_| port_error(PortErrorKind::Io, "db.storage_unavailable"))?
            .read(version_id)?;
        decode_record(record)
    }
}

pub(super) fn append(
    store: &SharedVersionStore,
    version: &PlanVersion,
) -> Result<(), &'static str> {
    let record = encode_record(version).map_err(|error| {
        if error.reason_key == PLAN_SOURCE_NOT_CURRENT {
            PLAN_SOURCE_NOT_CURRENT
        } else {
            "plan.store_failed"
        }
    })?;
    append_record(store, &record)
}

fn append_record(
    store: &SharedVersionStore,
    record: &PlanVersionRecord,
) -> Result<(), &'static str> {
    if record.body.len() > PLAN_BODY_MAX_BYTES {
        return Err(PLAN_TOO_MANY_ITEMS);
    }
    store
        .lock()
        .map_err(|_| "plan.store_failed")?
        .append_fixed(record)
        .map_err(|_| "plan.store_failed")
}

pub(super) fn read_for_get(
    store: &SharedVersionStore,
    request: PlanGetRequest,
) -> Result<PlanGetResponse, &'static str> {
    let record = store
        .lock()
        .map_err(|_| "db.storage_unavailable")?
        .read(&request.plan_version_id)
        .map_err(|_| "plan.not_found")?;
    let version = decode_record(record).map_err(|error| {
        if error.kind == PortErrorKind::Corrupt {
            "db.corrupt"
        } else {
            "plan.not_found"
        }
    })?;
    let mut selected = version
        .items
        .iter()
        .filter(|item| section_matches(item, request.section))
        .filter(|item| protection_matches(item, request.protection_filter.as_deref()))
        .collect::<Vec<_>>();
    let offset = request
        .cursor
        .as_deref()
        .map(|cursor| cursor.parse::<usize>().map_err(|_| "command.body_invalid"))
        .transpose()?
        .unwrap_or(0);
    if offset > selected.len() {
        return Err("command.body_invalid");
    }
    selected.drain(..offset);
    let has_more = selected.len() > PAGE_SIZE;
    selected.truncate(PAGE_SIZE);
    let end = offset + selected.len();
    let items = selected
        .into_iter()
        .map(|item| wire_item(&version, item))
        .collect();
    Ok(PlanGetResponse {
        head: wire_head(&version).map_err(|_| "db.corrupt")?,
        items,
        next_cursor: has_more.then(|| end.to_string()),
    })
}

pub(super) fn read_reason(
    store: &SharedVersionStore,
    plan_version_id: &Id,
    item_no: u32,
) -> Result<PlanItemReasonResponse, &'static str> {
    let record = store
        .lock()
        .map_err(|_| "db.storage_unavailable")?
        .read(plan_version_id)
        .map_err(|_| "plan.not_found")?;
    let version = decode_record(record).map_err(|error| {
        if error.kind == PortErrorKind::Corrupt {
            "db.corrupt"
        } else {
            "plan.not_found"
        }
    })?;
    let item = version
        .items
        .iter()
        .find(|item| item.item_no == item_no)
        .ok_or("plan.not_found")?;
    let mut restrictions = item
        .protection
        .as_ref()
        .map(|summary| summary.evidence.clone())
        .unwrap_or_default();
    if let Some(reason) = &item.reason_key {
        restrictions.push(reason.clone());
    }
    restrictions.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    restrictions.dedup();
    Ok(PlanItemReasonResponse {
        item_ref: item.item.clone(),
        selection_basis: item.selection_evidence.clone(),
        restrictions,
        disposition: disposition(item.state),
        reason_key: item.reason_key.clone(),
    })
}

pub(super) fn read_history(
    store: &SharedVersionStore,
    request: PlanHistoryRequest,
) -> Result<PlanHistoryResponse, &'static str> {
    let records = store
        .lock()
        .map_err(|_| "db.storage_unavailable")?
        .read_lineage(&request.lineage_id)
        .map_err(|_| "plan.not_found")?;
    let mut versions = Vec::with_capacity(records.len());
    for record in records {
        let version = decode_record(record).map_err(|error| {
            if error.kind == PortErrorKind::Corrupt {
                "db.corrupt"
            } else {
                "plan.not_found"
            }
        })?;
        versions.push(PlanHistoryEntry {
            plan_version_id: version.version_id,
            fixed_at: version.created_at,
            change_summary: if version.parent_version_id.is_some() {
                "plan.revised".into()
            } else {
                "plan.created".into()
            },
        });
    }
    Ok(PlanHistoryResponse { versions })
}

pub(super) fn fixed_event_body(
    version: &PlanVersion,
    job_ref: Id,
) -> Result<PlanVersionFixed, &'static str> {
    let count = |state| {
        version
            .items
            .iter()
            .filter(|item| item.state == state)
            .count()
            .min(u32::MAX as usize) as u32
    };
    Ok(PlanVersionFixed {
        plan_version_id: version.version_id.clone(),
        lineage_id: version.lineage_id.clone(),
        replaced_plan_version_id: version.parent_version_id.clone(),
        plan_hash: plan_hash(version).map_err(|_| "support.internal_error")?,
        readiness: wire_readiness(version.readiness),
        included_count: count(PlanItemState::Included),
        held_count: count(PlanItemState::Held),
        needs_confirmation_count: version
            .items
            .iter()
            .filter(|item| item.reason_key.as_deref() == Some("plan.item.needs_confirmation"))
            .count()
            .min(u32::MAX as usize) as u32,
        job_ref,
        fixed_at: version.created_at.clone(),
    })
}

fn encode_record(version: &PlanVersion) -> Result<PlanVersionRecord, PortError> {
    let body = serde_json::to_string(&json!({
        "formatVersion": BODY_FORMAT,
        "plan": version,
    }))
    .map_err(|_| port_error(PortErrorKind::Corrupt, "db.corrupt"))?;
    let readiness = match version.readiness {
        DomainReadiness::Ready => StoredReadiness::Ready,
        DomainReadiness::NeedsConfirmation => StoredReadiness::NeedsConfirmation,
        DomainReadiness::NoTarget => StoredReadiness::NoTarget,
        DomainReadiness::SamplePlan => StoredReadiness::NoTarget,
        DomainReadiness::SourceNotCurrent => {
            return Err(port_error(
                PortErrorKind::Unsupported,
                du_core_contracts::plan_ipc::PLAN_SOURCE_NOT_CURRENT,
            ));
        }
    };
    let origin_channel = match version.request_channel {
        du_change_planner::ports::RequestChannel::Gui => PlanOriginChannel::Gui,
        du_change_planner::ports::RequestChannel::Cli => PlanOriginChannel::Cli,
        du_change_planner::ports::RequestChannel::Mcp => PlanOriginChannel::Mcp,
    };
    Ok(PlanVersionRecord {
        plan_version_id: version.version_id.clone(),
        lineage_id: version.lineage_id.clone(),
        parent_plan_version_id: version.parent_version_id.clone(),
        version_hash: version.hash,
        hash_format: version.hash_format.clone(),
        ruleset_version: version.ruleset.as_ref().map_or(0, |rules| rules.version),
        readiness,
        origin_channel,
        body,
        created_at_ms: version.created_at.unix_millis(),
        data_source: version.data_source,
    })
}

fn decode_record(record: PlanVersionRecord) -> Result<PlanVersion, PortError> {
    let body: Value = serde_json::from_str(&record.body)
        .map_err(|_| port_error(PortErrorKind::Corrupt, "db.corrupt"))?;
    if body.get("formatVersion").and_then(Value::as_u64) != Some(BODY_FORMAT) {
        return Err(port_error(PortErrorKind::SchemaMismatch, "db.corrupt"));
    }
    let version: PlanVersion = serde_json::from_value(
        body.get("plan")
            .cloned()
            .ok_or_else(|| port_error(PortErrorKind::Corrupt, "db.corrupt"))?,
    )
    .map_err(|_| port_error(PortErrorKind::Corrupt, "db.corrupt"))?;
    let channel = match version.request_channel {
        du_change_planner::ports::RequestChannel::Gui => PlanOriginChannel::Gui,
        du_change_planner::ports::RequestChannel::Cli => PlanOriginChannel::Cli,
        du_change_planner::ports::RequestChannel::Mcp => PlanOriginChannel::Mcp,
    };
    let stored_readiness = match version.readiness {
        DomainReadiness::Ready => StoredReadiness::Ready,
        DomainReadiness::NeedsConfirmation => StoredReadiness::NeedsConfirmation,
        DomainReadiness::NoTarget | DomainReadiness::SamplePlan => StoredReadiness::NoTarget,
        DomainReadiness::SourceNotCurrent => {
            return Err(port_error(PortErrorKind::Corrupt, "db.corrupt"));
        }
    };
    if version.version_id != record.plan_version_id
        || version.lineage_id != record.lineage_id
        || version.parent_version_id != record.parent_plan_version_id
        || version.hash != record.version_hash
        || version.hash_format != record.hash_format
        || version.created_at.unix_millis() != record.created_at_ms
        || version.data_source != record.data_source
        || version.ruleset.as_ref().map_or(0, |rules| rules.version) != record.ruleset_version
        || channel != record.origin_channel
        || stored_readiness != record.readiness
        || PlanBuilder::hash_version(&version) != version.hash
    {
        return Err(port_error(PortErrorKind::Corrupt, "db.corrupt"));
    }
    Ok(version)
}

fn wire_head(version: &PlanVersion) -> Result<PlanVersionHead, ()> {
    let ruleset = version.ruleset.as_ref();
    Ok(PlanVersionHead {
        plan_version_id: version.version_id.clone(),
        lineage_id: version.lineage_id.clone(),
        parent_plan_version_id: version.parent_version_id.clone(),
        plan_hash: plan_hash(version)?,
        hash_format: version.hash_format.clone(),
        readiness: wire_readiness(version.readiness),
        protection_ruleset_version: ruleset
            .map_or_else(|| "unknown".into(), |r| r.version.to_string()),
        protection_class_table_version: ruleset.map_or_else(
            || "unknown".into(),
            |r| r.classification_version.to_string(),
        ),
        effect_summary: serde_json::to_value(&version.expected_effect).unwrap_or(Value::Null),
        restoration_limit_summary: restoration_summary(&version.items),
        origin_channel: match version.request_channel {
            du_change_planner::ports::RequestChannel::Gui => {
                du_core_contracts::plan_ipc::PlanOriginChannel::Gui
            }
            du_change_planner::ports::RequestChannel::Cli => {
                du_core_contracts::plan_ipc::PlanOriginChannel::Cli
            }
            du_change_planner::ports::RequestChannel::Mcp => {
                du_core_contracts::plan_ipc::PlanOriginChannel::Mcp
            }
        },
        fixed_at: version.created_at.clone(),
        change_summary: if version.parent_version_id.is_some() {
            "plan.revised"
        } else {
            "plan.created"
        }
        .into(),
    })
}

fn wire_item(version: &PlanVersion, item: &PlanItem) -> PlanVersionItem {
    let action = match item.action {
        PlanAction::Trash => du_core_contracts::job::ChangeAction::Trash,
        PlanAction::Move => du_core_contracts::job::ChangeAction::Move,
        PlanAction::Delete => du_core_contracts::job::ChangeAction::Delete,
    };
    PlanVersionItem {
        item_no: item.item_no,
        item_ref: item.item.clone(),
        action,
        destination: item.destination.clone(),
        retention_condition_ref: retention_condition(&item.condition),
        disposition: disposition(item.state),
        overwrite: item.overwrite,
        expected_identity: serde_json::to_value(&item.expected_identity).unwrap_or(Value::Null),
        protection_summary: serde_json::to_value(&item.protection).unwrap_or(Value::Null),
        reason_key: item.reason_key.clone(),
        selection_basis: item.selection_evidence.clone(),
        absorbed_by: item.absorbed_by.and_then(|number| {
            version
                .items
                .iter()
                .find(|candidate| candidate.item_no == number)
                .map(|candidate| candidate.item.clone())
        }),
    }
}

fn section_matches(item: &PlanItem, section: PlanSection) -> bool {
    match section {
        PlanSection::Executable => item.state == PlanItemState::Included,
        PlanSection::Held => item.state == PlanItemState::Held,
        PlanSection::NeedsConfirmation => {
            item.reason_key.as_deref() == Some("plan.item.needs_confirmation")
        }
        PlanSection::Absorbed => item.absorbed_by.is_some(),
    }
}

fn protection_matches(item: &PlanItem, filter: Option<&str>) -> bool {
    let Some(filter) = filter else { return true };
    item.protection.as_ref().is_some_and(|protection| {
        protection.classification_id.as_deref() == Some(filter)
            || format!("{:?}", protection.verdict).eq_ignore_ascii_case(filter)
    })
}

fn restoration_summary(items: &[PlanItem]) -> Value {
    let supported = items
        .iter()
        .filter(|item| item.restore == RestoreRestriction::Supported)
        .count();
    let unsupported = items
        .iter()
        .filter(|item| item.restore == RestoreRestriction::Unsupported)
        .count();
    let unconfirmed = items
        .iter()
        .filter(|item| item.restore == RestoreRestriction::Unconfirmed)
        .count();
    json!({"supported": supported, "unsupported": unsupported, "unconfirmed": unconfirmed})
}

fn disposition(state: PlanItemState) -> PlanItemDisposition {
    match state {
        PlanItemState::Included => PlanItemDisposition::Included,
        PlanItemState::Held => PlanItemDisposition::Held,
        PlanItemState::Rejected => PlanItemDisposition::Rejected,
    }
}

fn wire_readiness(readiness: DomainReadiness) -> PlanReadiness {
    match readiness {
        DomainReadiness::SamplePlan => PlanReadiness::ExperiencePlan,
        DomainReadiness::SourceNotCurrent => PlanReadiness::SourceNotCurrent,
        DomainReadiness::NeedsConfirmation => PlanReadiness::NeedsConfirmation,
        DomainReadiness::NoTarget => PlanReadiness::NoExecutableItems,
        DomainReadiness::Ready => PlanReadiness::ApprovalPossible,
    }
}

fn plan_hash(version: &PlanVersion) -> Result<PlanHash, ()> {
    let mut hex = String::with_capacity(64);
    for byte in version.hash {
        use std::fmt::Write as _;
        let _ = write!(hex, "{byte:02x}");
    }
    PlanHash::parse(hex).map_err(|_| ())
}

fn retention_condition(condition: &[u8]) -> Option<Id> {
    let value = condition.strip_prefix(b"f10-retention-v1:")?;
    Id::parse(std::str::from_utf8(value).ok()?).ok()
}

pub(super) fn retention_condition_bytes(id: Option<&Id>) -> Vec<u8> {
    id.map_or_else(Vec::new, |id| {
        [b"f10-retention-v1:".as_slice(), id.as_str().as_bytes()].concat()
    })
}

fn ruleset_read_error(error: RulesetReadError) -> PortError {
    let (kind, reason) = match error {
        RulesetReadError::Corrupt => (PortErrorKind::Corrupt, "protect.ruleset_unreadable"),
        RulesetReadError::StorageUnavailable => (PortErrorKind::Io, "protect.ruleset_unavailable"),
    };
    port_error(kind, reason)
}

fn port_error(kind: PortErrorKind, reason: &str) -> PortError {
    PortError {
        kind,
        reason_key: reason.into(),
        os_code: None,
    }
}

#[cfg(test)]
mod tests;
