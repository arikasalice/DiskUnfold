//! Own approved file change execution.
//! Execution-time approval checks shared by the app-host change command.
//!
//! This crate deliberately performs no file I/O. The app-host calls this stage
//! before it admits any future change job (FR9.HD1.app-host.DD1).

use du_change_planner::{
    api::PlanBuilder,
    ports::{ConfirmationToken, PlanAction, PlanItem, PlanItemState, PlanVersion},
};
use du_core_contracts::{
    ByteSize, Id, SizeAmount,
    approval_check::{
        ApprovalCheckFacts, ApprovalCheckFinishRecord, ApprovalCheckRecorder,
        ApprovalCheckStartRecord,
    },
    approval_ipc::ApprovalConfirmationProofs,
};

pub mod trash;

/// Appends the stage-one result before the approved job is admitted.
///
/// The supplied facts carry the exact closed result returned by the comparison.
/// The host supplies the recorder backed by `app-db`; this crate owns no storage.
pub fn record_check_start(
    recorder: &mut dyn ApprovalCheckRecorder,
    facts: &ApprovalCheckFacts,
) -> Result<(), du_core_contracts::ports::PortError> {
    recorder.append_start(&ApprovalCheckStartRecord {
        facts: facts.clone(),
    })
}

/// Appends the terminal audit row after the admitted job ends.
pub fn record_check_finish(
    recorder: &mut dyn ApprovalCheckRecorder,
    facts: &ApprovalCheckFacts,
    end_state: impl Into<String>,
) -> Result<(), du_core_contracts::ports::PortError> {
    recorder.append_finish(&ApprovalCheckFinishRecord {
        facts: facts.clone(),
        end_state: end_state.into(),
    })
}

/// Approval state kept only in the current app session.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalRecordState {
    /// A grant can be consumed once by the matching execution request.
    Issued,
    /// The grant was consumed before job admission.
    Consumed,
    /// A plan or protection change invalidated the grant.
    Invalidated,
}

/// Opaque in-memory grant record. It is never serialized or persisted.
pub struct ApprovalRecord {
    /// Internal identifier; never included in state events.
    pub approval_id: Id,
    /// Exact immutable request version approved.
    pub request_version_id: Id,
    /// Hash shown to the user and confirmed by the grant request.
    pub plan_hash: [u8; 32],
    /// Digest format bound to the grant.
    pub hash_format: String,
    /// Application session that issued the grant.
    pub app_session_id: Id,
    /// Secret compared in constant time and erased after consumption.
    pub secret: [u8; 32],
    /// Stored plan retained to compare operation and destination facts.
    pub approved_plan: PlanVersion,
    /// Proof values confirmed by the user.
    pub confirmations: Option<ApprovalConfirmationProofs>,
    /// Ruleset version used during approval review.
    pub ruleset_version: Option<u64>,
    /// Current state of this grant.
    pub state: ApprovalRecordState,
}

impl Drop for ApprovalRecord {
    fn drop(&mut self) {
        self.secret.fill(0);
    }
}

/// Execution-side copy of the secret and its request binding.
pub struct ApprovalExecutionToken {
    request_version_id: Id,
    app_session_id: Id,
    secret: [u8; 32],
    consumed: bool,
}

impl ApprovalExecutionToken {
    /// Builds an execution token for the host-owned, in-memory approval record.
    pub fn issue(request_version_id: Id, app_session_id: Id, secret: [u8; 32]) -> Self {
        Self {
            request_version_id,
            app_session_id,
            secret,
            consumed: false,
        }
    }

    /// Invalidates the token and erases its secret from this session record.
    pub fn invalidate(&mut self) {
        self.secret.fill(0);
        self.consumed = true;
    }
}

impl Drop for ApprovalExecutionToken {
    fn drop(&mut self) {
        self.secret.fill(0);
    }
}

/// Successful stage-one result. It contains no filesystem operation handle.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ApprovalCheckReceipt {
    /// Internal approval id for job history linkage.
    pub approval_id: Id,
    /// Approved request version.
    pub request_version_id: Id,
    /// Recomputed canonical hash.
    pub plan_hash: [u8; 32],
    /// Ruleset version fixed during review.
    pub ruleset_version: Option<u64>,
}

/// First-mismatch result for the common execution frame.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ApprovalCheckFailure {
    /// No issued approval record exists.
    NotIssued,
    /// The token has already been consumed or was invalidated.
    AlreadyUsed,
    /// A binding field does not match the issued approval.
    Mismatch,
    /// Required explicit user proofs are absent or incorrect.
    ConfirmationMissing,
}

impl ApprovalCheckFailure {
    /// Converts the refusal to the closed value recorded by `app-db`.
    pub const fn audit_result(self) -> du_core_contracts::approval_check::ApprovalCheckResult {
        use du_core_contracts::approval_check::ApprovalCheckResult as ResultValue;
        match self {
            Self::NotIssued => ResultValue::NotIssued,
            Self::AlreadyUsed => ResultValue::AlreadyUsed,
            Self::Mismatch => ResultValue::Mismatch,
            Self::ConfirmationMissing => ResultValue::ConfirmationMissing,
        }
    }

    /// Returns the stable audit value required by app-db.
    pub const fn as_str(self) -> &'static str {
        self.audit_result().as_db_value()
    }
}

/// Checks a grant in DLD order and consumes it before returning success.
///
/// The caller must hold the approval registry lock for the entire operation so
/// consumption cannot race a second execution request. No filesystem work may
/// happen until this function returns `Ok`.
pub fn check_approval(
    record: Option<&mut ApprovalRecord>,
    token: &mut ApprovalExecutionToken,
    current_plan: &PlanVersion,
    current_session_id: &Id,
    confirmations: Option<&ApprovalConfirmationProofs>,
) -> Result<ApprovalCheckReceipt, ApprovalCheckFailure> {
    let Some(record) = record else {
        return Err(ApprovalCheckFailure::NotIssued);
    };
    if record.state != ApprovalRecordState::Issued || token.consumed {
        return Err(ApprovalCheckFailure::AlreadyUsed);
    }
    if !constant_time_equal(&record.secret, &token.secret) {
        return Err(ApprovalCheckFailure::Mismatch);
    }
    if &record.app_session_id != current_session_id
        || &token.app_session_id != current_session_id
        || token.request_version_id != record.request_version_id
    {
        return Err(ApprovalCheckFailure::Mismatch);
    }
    let recomputed_hash = PlanBuilder::hash_version(&record.approved_plan);
    let current_hash = PlanBuilder::hash_version(current_plan);
    if current_plan.version_id != record.request_version_id
        || current_plan.version_id != record.approved_plan.version_id
        || recomputed_hash != record.plan_hash
        || current_hash != current_plan.hash
        || recomputed_hash != current_hash
        || current_plan.hash_format != record.hash_format
    {
        return Err(ApprovalCheckFailure::Mismatch);
    }
    if operation_facts(&record.approved_plan) != operation_facts(current_plan) {
        return Err(ApprovalCheckFailure::Mismatch);
    }
    if confirmations != record.confirmations.as_ref()
        || !proofs_satisfy_plan(current_plan, confirmations)
    {
        return Err(ApprovalCheckFailure::ConfirmationMissing);
    }

    record.state = ApprovalRecordState::Consumed;
    record.secret.fill(0);
    token.secret.fill(0);
    token.consumed = true;
    Ok(ApprovalCheckReceipt {
        approval_id: record.approval_id.clone(),
        request_version_id: record.request_version_id.clone(),
        plan_hash: recomputed_hash,
        ruleset_version: record.ruleset_version,
    })
}

fn constant_time_equal(left: &[u8; 32], right: &[u8; 32]) -> bool {
    left.iter()
        .zip(right)
        .fold(0_u8, |difference, (a, b)| difference | (a ^ b))
        == 0
}

#[derive(PartialEq)]
struct OperationFact<'a> {
    item: &'a du_core_contracts::ItemRef,
    action: PlanAction,
    destination: &'a Option<du_core_contracts::OsPath>,
    condition: &'a [u8],
    overwrite: bool,
}

fn operation_facts(plan: &PlanVersion) -> Vec<OperationFact<'_>> {
    plan.items
        .iter()
        .filter(|item| item.state == PlanItemState::Included)
        .map(|item| OperationFact {
            item: &item.item,
            action: item.action,
            destination: &item.destination,
            condition: &item.condition,
            overwrite: item.overwrite,
        })
        .collect()
}

fn proofs_satisfy_plan(plan: &PlanVersion, proofs: Option<&ApprovalConfirmationProofs>) -> bool {
    let included = plan
        .items
        .iter()
        .filter(|item| item.state == PlanItemState::Included)
        .collect::<Vec<_>>();
    let delete = included
        .iter()
        .any(|item| item.action == PlanAction::Delete);
    let release = plan
        .confirmations
        .iter()
        .any(|value| matches!(value, ConfirmationToken::ReleaseDefaultProtection { .. }));
    let overwrites = included
        .iter()
        .filter(|item| item.overwrite)
        .copied()
        .collect::<Vec<_>>();
    let Some(proofs) = proofs else {
        return !delete && !release && overwrites.is_empty();
    };
    if delete {
        let Some(target_bytes) = known_size(included.iter().copied()) else {
            return false;
        };
        if proofs.permanent_delete_selected != Some(true)
            || proofs.irreversibility_confirmed != Some(true)
            || proofs.confirmed_target_count != Some(included.len().min(u32::MAX as usize) as u32)
            || proofs.confirmed_target_size.as_ref().map(ByteSize::get) != Some(target_bytes)
        {
            return false;
        }
    }
    if release && proofs.basic_protection_unlock_confirmed != Some(true) {
        return false;
    }
    if !overwrites.is_empty() {
        let Some(bytes) = known_size(overwrites.iter().copied()) else {
            return false;
        };
        if proofs.overwrite_confirmed != Some(true)
            || proofs.confirmed_overwrite_item_count
                != Some(overwrites.len().min(u32::MAX as usize) as u32)
            || proofs.confirmed_displaced_bytes.as_ref().map(ByteSize::get) != Some(bytes)
        {
            return false;
        }
    }
    true
}

fn known_size<'a>(items: impl IntoIterator<Item = &'a PlanItem>) -> Option<u64> {
    items.into_iter().try_fold(0_u64, |total, item| {
        let value = item.expected_size.as_ref()?;
        let bytes = match &value.amount {
            SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => bytes.get(),
            SizeAmount::Unknown => return None,
        };
        total.checked_add(bytes)
    })
}

#[cfg(test)]
mod tests;
