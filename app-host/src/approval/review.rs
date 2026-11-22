//! FR1.HD1.app-host.DD1: derive the approval review summary from a pinned plan re-judgement.

use super::*;

pub(super) fn review_summary(
    version: &PlanVersion,
    rejudgement: &PlanRejudgement,
) -> (ApprovalReviewSummary, PlanHash, bool, Vec<String>) {
    let hash = PlanHash::from_bytes(version.hash);
    let included = version
        .items
        .iter()
        .filter(|item| item.state == PlanItemState::Included)
        .collect::<Vec<_>>();
    let mut reasons = BTreeSet::new();
    let rejudged: BTreeMap<_, _> = rejudgement.items.iter().copied().collect();
    let protection_results = protection_results(&version.items, &rejudged, &mut reasons);
    let (target_count, target_size) = target_summary(&included, &mut reasons);
    let (actions, destination, retention_conditions) = operation_summary(&included);
    let protection_summary = protection_summary(version, rejudgement, &included);
    let overwrite_summary = overwrite_summary(&included, &mut reasons);
    add_plan_availability_reasons(version, rejudgement, &included, &mut reasons);
    let summary = ApprovalReviewSummary {
        target_count,
        target_size,
        visible_selection_count: target_count,
        hidden_selection_count: version
            .items
            .iter()
            .filter(|item| item.absorbed_by.is_some())
            .count()
            .min(u32::MAX as usize) as u32,
        hidden_descendant_count: version
            .items
            .iter()
            .filter(|item| item.absorbed_by.is_some())
            .count()
            .min(u32::MAX as usize) as u32,
        actions,
        destination,
        retention_conditions,
        effect_summary: serde_json::to_value(&version.expected_effect).unwrap_or(json!([])),
        protection_results,
        restoration_limit_summary: restoration_summary(&version.items),
        protection_summary,
        overwrite_summary,
    };
    let possible = reasons.is_empty();
    (summary, hash, possible, reasons.into_iter().collect())
}

fn protection_results(
    items: &[PlanItem],
    rejudged: &BTreeMap<u32, RejudgementState>,
    reasons: &mut BTreeSet<String>,
) -> Vec<ApprovalProtectionResult> {
    items
        .iter()
        .map(|item| {
            if item.state != PlanItemState::Included {
                return ApprovalProtectionResult {
                    item_ref: item.item.clone(),
                    disposition: disposition(item.state),
                    reason_key: item.reason_key.clone(),
                };
            }
            let status = rejudged
                .get(&item.item_no)
                .copied()
                .unwrap_or(RejudgementState::Undetermined);
            let (disposition, reason_key) = match status {
                RejudgementState::Same => (PlanItemDisposition::Included, None),
                RejudgementState::Denied => {
                    reasons.insert(APPROVAL_PROTECTION_DENIED.to_owned());
                    (
                        PlanItemDisposition::Rejected,
                        Some(APPROVAL_PROTECTION_DENIED.to_owned()),
                    )
                }
                RejudgementState::ClassChanged | RejudgementState::Undetermined => {
                    reasons.insert(APPROVAL_PROTECTION_UNDETERMINED.to_owned());
                    (
                        PlanItemDisposition::Held,
                        Some(APPROVAL_PROTECTION_UNDETERMINED.to_owned()),
                    )
                }
            };
            ApprovalProtectionResult {
                item_ref: item.item.clone(),
                disposition,
                reason_key,
            }
        })
        .collect()
}

fn disposition(state: PlanItemState) -> PlanItemDisposition {
    match state {
        PlanItemState::Included => PlanItemDisposition::Included,
        PlanItemState::Held => PlanItemDisposition::Held,
        PlanItemState::Rejected => PlanItemDisposition::Rejected,
    }
}

fn target_summary(included: &[&PlanItem], reasons: &mut BTreeSet<String>) -> (u32, ByteSize) {
    let count = included.len().min(u32::MAX as usize) as u32;
    let size = sum_sizes(included.iter().copied()).map_or_else(
        || {
            reasons.insert(APPROVAL_REVIEW_UNAVAILABLE.to_owned());
            ByteSize::from(0)
        },
        ByteSize::from,
    );
    (count, size)
}

fn operation_summary(
    included: &[&PlanItem],
) -> (
    Vec<du_core_contracts::job::ChangeAction>,
    Option<du_core_contracts::OsPath>,
    Vec<Id>,
) {
    let mut actions = Vec::new();
    for item in included {
        let action = match item.action {
            PlanAction::Trash => du_core_contracts::job::ChangeAction::Trash,
            PlanAction::Move => du_core_contracts::job::ChangeAction::Move,
            PlanAction::Delete => du_core_contracts::job::ChangeAction::Delete,
        };
        if !actions.contains(&action) {
            actions.push(action);
        }
    }
    let destination = included
        .first()
        .and_then(|item| item.destination.clone())
        .filter(|first| {
            included
                .iter()
                .all(|item| item.destination.as_ref() == Some(first))
        });
    let retention_conditions = included
        .iter()
        .filter_map(|item| {
            std::str::from_utf8(&item.condition)
                .ok()?
                .strip_prefix("f10-retention-v1:")
                .and_then(|id| Id::parse(id).ok())
        })
        .collect();
    (actions, destination, retention_conditions)
}

fn protection_summary(
    version: &PlanVersion,
    rejudgement: &PlanRejudgement,
    included: &[&PlanItem],
) -> Option<ApprovalProtectionSummary> {
    let unlocked = included
        .iter()
        .copied()
        .filter(|item| {
            item.protection
                .as_ref()
                .is_some_and(|protection| protection.released)
        })
        .collect::<Vec<_>>();
    if unlocked.is_empty() {
        return None;
    }
    let unlocked_classes = unlocked
        .iter()
        .filter_map(|item| item.classification_id.clone())
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    Some(ApprovalProtectionSummary {
        ruleset_version: rejudgement
            .ruleset_version
            .map_or_else(|| "unknown".into(), |version| version.to_string()),
        class_table_version: version.ruleset.as_ref().map_or_else(
            || "unknown".into(),
            |ruleset| ruleset.classification_version.to_string(),
        ),
        unlocked_classes,
        unlocked_item_count: unlocked.len().min(u32::MAX as usize) as u32,
        unlocked_bytes: ByteSize::from(sum_sizes(unlocked.iter().copied()).unwrap_or(0)),
        required_basic_protection_ack: true,
    })
}

fn overwrite_summary(
    included: &[&PlanItem],
    reasons: &mut BTreeSet<String>,
) -> Option<ApprovalOverwriteSummary> {
    let overwrites = included
        .iter()
        .filter(|item| item.overwrite)
        .copied()
        .collect::<Vec<_>>();
    if overwrites.is_empty() {
        return None;
    }
    let displaced_bytes = sum_displaced_sizes(overwrites.iter().copied());
    if displaced_bytes.is_none() {
        reasons.insert(APPROVAL_REVIEW_UNAVAILABLE.to_owned());
    }
    Some(ApprovalOverwriteSummary {
        overwrite_item_count: overwrites.len().min(u32::MAX as usize) as u32,
        displaced_bytes: ByteSize::from(displaced_bytes.unwrap_or(0)),
        required_overwrite_ack: true,
    })
}

fn add_plan_availability_reasons(
    version: &PlanVersion,
    rejudgement: &PlanRejudgement,
    included: &[&PlanItem],
    reasons: &mut BTreeSet<String>,
) {
    if included.is_empty()
        || version.data_source != DataSource::Real
        || rejudgement.version_id != version.version_id
        || rejudgement.recomputed_hash != version.hash
    {
        reasons.insert(APPROVAL_REVIEW_UNAVAILABLE.to_owned());
    }
}

pub(super) fn validate_confirmations(
    version: &PlanVersion,
    summary: &ApprovalReviewSummary,
    proofs: Option<&ApprovalConfirmationProofs>,
) -> Result<(), EngineError> {
    let delete = summary
        .actions
        .contains(&du_core_contracts::job::ChangeAction::Delete);
    let release = version.confirmations.iter().any(|confirmation| {
        matches!(
            confirmation,
            ConfirmationToken::ReleaseDefaultProtection { .. }
        )
    });
    let overwrite = summary.overwrite_summary.as_ref();
    if !delete && !release && overwrite.is_none() && proofs.is_none() {
        return Ok(());
    }
    let Some(proofs) = proofs else {
        return Err(error(APPROVAL_CONFIRMATION_MISSING));
    };
    if delete
        && (proofs.permanent_delete_selected != Some(true)
            || proofs.irreversibility_confirmed != Some(true)
            || proofs.confirmed_target_count != Some(summary.target_count)
            || proofs.confirmed_target_size.as_ref() != Some(&summary.target_size))
    {
        return Err(error(APPROVAL_CONFIRMATION_MISSING));
    }
    if release && proofs.basic_protection_unlock_confirmed != Some(true) {
        return Err(error(APPROVAL_CONFIRMATION_MISSING));
    }
    if let Some(overwrite) = overwrite
        && (proofs.overwrite_confirmed != Some(true)
            || proofs.confirmed_overwrite_item_count != Some(overwrite.overwrite_item_count)
            || proofs.confirmed_displaced_bytes.as_ref() != Some(&overwrite.displaced_bytes))
    {
        return Err(error(APPROVAL_CONFIRMATION_MISSING));
    }
    Ok(())
}

fn sum_sizes<'a>(items: impl IntoIterator<Item = &'a PlanItem>) -> Option<u64> {
    items.into_iter().try_fold(0_u64, |sum, item| {
        let size = item.expected_size.as_ref()?;
        if size.evidence == SizeEvidence::Unknown {
            return None;
        }
        let bytes = match &size.amount {
            SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => bytes.get(),
            SizeAmount::Unknown => return None,
        };
        sum.checked_add(bytes)
    })
}

fn sum_displaced_sizes<'a>(items: impl IntoIterator<Item = &'a PlanItem>) -> Option<u64> {
    items.into_iter().try_fold(0_u64, |sum, item| {
        let size = item.displaced_size.as_ref()?;
        let bytes = match &size.amount {
            SizeAmount::Exact { bytes } | SizeAmount::LowerBound { bytes } => bytes.get(),
            SizeAmount::Unknown => return None,
        };
        sum.checked_add(bytes)
    })
}

fn restoration_summary(items: &[PlanItem]) -> serde_json::Value {
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
