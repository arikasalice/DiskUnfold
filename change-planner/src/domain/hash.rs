use crate::ports::{
    ConfirmationToken, EstimateQuality, PlanAction, PlanItem, PlanItemState, PlanVersion,
    ProtectionSummary, ProtectionVerdict, RestoreRestriction, VolumeEstimate,
};
use du_core_contracts::{
    PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue, result::ResultItemKind,
};

/// Computes the deterministic BLAKE3 digest for the v1 canonical plan scope.
pub(crate) fn plan_hash(plan: &PlanVersion) -> [u8; 32] {
    let mut writer = CanonicalWriter::default();
    writer.string("du.plan.hash.v1");
    writer.id(&plan.source_version_id);
    writer.byte(match plan.data_source {
        du_core_contracts::data_source::DataSource::Real => 0,
        du_core_contracts::data_source::DataSource::Sample => 1,
    });
    writer.byte(match plan.source_kind {
        crate::ports::PlanSourceKind::UserSelection => 0,
        crate::ports::PlanSourceKind::GoalAlternative => 1,
        crate::ports::PlanSourceKind::KeepSuggestion => 2,
        crate::ports::PlanSourceKind::Scenario => 3,
    });
    match &plan.ruleset {
        Some(ruleset) => {
            writer.byte(1);
            writer.u64(ruleset.version);
            writer.bytes(&ruleset.hash);
            writer.u32(ruleset.classification_version);
            writer.u32(ruleset.known_location_version);
        }
        None => writer.byte(0),
    }

    writer.count(plan.items.len());
    for item in &plan.items {
        write_item(&mut writer, item);
    }
    write_confirmations(&mut writer, &plan.confirmations);
    writer.string("change_estimate");
    writer.u16(1);
    write_effects(&mut writer, &plan.expected_effect);
    *blake3::hash(&writer.bytes).as_bytes()
}

fn write_item(writer: &mut CanonicalWriter, item: &PlanItem) {
    writer.u32(item.item_no);
    writer.id(&item.item.result_version);
    writer.u32(item.item.item_no);
    write_path(writer, &item.path);
    writer.bytes(&item.volume_id);
    match &item.expected_identity {
        Some(identity) => {
            writer.byte(1);
            writer.bytes(&identity.volume_id);
            writer.bytes(&identity.file_id);
        }
        None => writer.byte(0),
    }
    writer.byte(kind_code(item.expected_kind));
    write_optional_size(writer, item.expected_size.as_ref());
    write_optional_timestamp(writer, item.expected_modified_at.as_ref());
    writer.optional_string(item.classification_id.as_deref());
    writer.byte(action_code(item.action));
    write_optional_path(writer, item.destination.as_ref());
    writer.boolean(item.overwrite);
    write_optional_size(writer, item.displaced_size.as_ref());
    writer.bytes(&item.condition);
    let mut selection_evidence = item.selection_evidence.clone();
    selection_evidence.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
    writer.count(selection_evidence.len());
    for evidence in selection_evidence {
        writer.string(&evidence);
    }
    writer.byte(state_code(item.state));
    writer.optional_string(item.reason_key.as_deref());
    writer.optional_u32(item.absorbed_by);
    match &item.protection {
        Some(summary) => {
            writer.byte(1);
            write_protection(writer, summary);
        }
        None => writer.byte(0),
    }
    writer.byte(restore_code(item.restore));
}

fn write_protection(writer: &mut CanonicalWriter, summary: &ProtectionSummary) {
    writer.byte(verdict_code(summary.verdict));
    writer.optional_string(summary.classification_id.as_deref());
    writer.boolean(summary.released);
    writer.count(summary.evaluators.len());
    for (evaluator, version) in &summary.evaluators {
        writer.string(evaluator.as_str());
        writer.u16(*version);
    }
    writer.count(summary.evidence.len());
    for reason in &summary.evidence {
        writer.string(reason);
    }
}

fn write_confirmations(writer: &mut CanonicalWriter, confirmations: &[ConfirmationToken]) {
    let release = confirmations.iter().find_map(|token| match token {
        ConfirmationToken::ReleaseDefaultProtection { classification_ids } => {
            Some(classification_ids)
        }
        ConfirmationToken::Overwrite { .. } => None,
    });
    let overwrite = confirmations.iter().find_map(|token| match token {
        ConfirmationToken::Overwrite {
            item_count,
            displaced_bytes,
        } => Some((*item_count, *displaced_bytes)),
        ConfirmationToken::ReleaseDefaultProtection { .. } => None,
    });
    writer.count(if release.is_some() { 1 } else { 0 } + if overwrite.is_some() { 1 } else { 0 });
    if let Some(classification_ids) = release {
        writer.byte(0);
        let mut ids = classification_ids.clone();
        ids.sort_by(|left, right| left.as_bytes().cmp(right.as_bytes()));
        writer.count(ids.len());
        for id in ids {
            writer.string(&id);
        }
    }
    if let Some((item_count, displaced_bytes)) = overwrite {
        writer.byte(1);
        writer.u64(item_count);
        writer.u64(displaced_bytes);
    }
}

fn write_effects(writer: &mut CanonicalWriter, effects: &[VolumeEstimate]) {
    let mut effects = effects.iter().collect::<Vec<_>>();
    effects.sort_by(|left, right| left.volume_id.cmp(&right.volume_id));
    writer.count(effects.len());
    for effect in effects {
        write_effect(writer, effect);
    }
}

fn write_effect(writer: &mut CanonicalWriter, effect: &VolumeEstimate) {
    writer.bytes(&effect.volume_id);
    writer.u64(effect.known_bytes);
    writer.u64(effect.unconfirmed_items);
    writer.byte(match effect.quality {
        EstimateQuality::Exact => 0,
        EstimateQuality::LowerBound => 1,
        EstimateQuality::Unconfirmed => 2,
    });
}

fn write_path(writer: &mut CanonicalWriter, path: &du_core_contracts::OsPath) {
    writer.byte(match &path.encoding {
        PathEncoding::Unix => 0,
        PathEncoding::WindowsUtf16le => 1,
    });
    let raw = path.raw.bytes().unwrap_or_default();
    writer.bytes(&raw);
}

fn write_optional_path(writer: &mut CanonicalWriter, path: Option<&du_core_contracts::OsPath>) {
    match path {
        Some(path) => {
            writer.byte(1);
            write_path(writer, path);
        }
        None => writer.byte(0),
    }
}

fn write_optional_size(writer: &mut CanonicalWriter, size: Option<&SizeValue>) {
    match size {
        Some(size) => {
            writer.byte(1);
            writer.byte(match &size.basis {
                SizeBasis::Logical => 0,
                SizeBasis::Allocated => 1,
                SizeBasis::UniquePhysical => 2,
            });
            writer.byte(match &size.evidence {
                SizeEvidence::Measured => 0,
                SizeEvidence::OsReported => 1,
                SizeEvidence::Estimated => 2,
                SizeEvidence::Unknown => 3,
            });
            match &size.amount {
                SizeAmount::Exact { bytes } => {
                    writer.byte(0);
                    writer.u64(bytes.get());
                }
                SizeAmount::LowerBound { bytes } => {
                    writer.byte(1);
                    writer.u64(bytes.get());
                }
                SizeAmount::Unknown => writer.byte(2),
            }
        }
        None => writer.byte(0),
    }
}

fn write_optional_timestamp(
    writer: &mut CanonicalWriter,
    value: Option<&du_core_contracts::Timestamp>,
) {
    match value {
        Some(value) => {
            writer.byte(1);
            writer.i64(value.unix_millis());
        }
        None => writer.byte(0),
    }
}

fn kind_code(kind: ResultItemKind) -> u8 {
    match kind {
        ResultItemKind::Directory => 0,
        ResultItemKind::File => 1,
        ResultItemKind::UntraversedLink => 2,
        ResultItemKind::Unknown => 3,
        ResultItemKind::UnknownStructure => 4,
    }
}

fn action_code(action: PlanAction) -> u8 {
    match action {
        PlanAction::Trash => 0,
        PlanAction::Move => 1,
        PlanAction::Delete => 2,
    }
}

fn state_code(state: PlanItemState) -> u8 {
    match state {
        PlanItemState::Included => 0,
        PlanItemState::Held => 1,
        PlanItemState::Rejected => 2,
    }
}

fn verdict_code(verdict: ProtectionVerdict) -> u8 {
    match verdict {
        ProtectionVerdict::Allow => 0,
        ProtectionVerdict::AllowReleased => 1,
        ProtectionVerdict::GuidanceOnly => 2,
        ProtectionVerdict::Undetermined => 3,
        ProtectionVerdict::Deny => 4,
        ProtectionVerdict::SkipClassRows => 5,
    }
}

fn restore_code(restore: RestoreRestriction) -> u8 {
    match restore {
        RestoreRestriction::Supported => 0,
        RestoreRestriction::Unsupported => 1,
        RestoreRestriction::Unconfirmed => 2,
    }
}

#[derive(Default)]
struct CanonicalWriter {
    bytes: Vec<u8>,
}

impl CanonicalWriter {
    fn byte(&mut self, value: u8) {
        self.bytes.push(value);
    }

    fn boolean(&mut self, value: bool) {
        self.byte(u8::from(value));
    }

    fn u16(&mut self, value: u16) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u32(&mut self, value: u32) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn u64(&mut self, value: u64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn i64(&mut self, value: i64) {
        self.bytes.extend_from_slice(&value.to_le_bytes());
    }

    fn count(&mut self, value: usize) {
        self.u64(u64::try_from(value).unwrap_or(u64::MAX));
    }

    fn bytes(&mut self, value: &[u8]) {
        self.count(value.len());
        self.bytes.extend_from_slice(value);
    }

    fn string(&mut self, value: &str) {
        self.bytes(value.as_bytes());
    }

    fn optional_string(&mut self, value: Option<&str>) {
        match value {
            Some(value) => {
                self.byte(1);
                self.string(value);
            }
            None => self.byte(0),
        }
    }

    fn optional_u32(&mut self, value: Option<u32>) {
        match value {
            Some(value) => {
                self.byte(1);
                self.u32(value);
            }
            None => self.byte(0),
        }
    }

    fn id(&mut self, id: &du_core_contracts::Id) {
        match id.v7_bytes() {
            Some(bytes) => self.bytes.extend_from_slice(&bytes),
            None => self.bytes.extend_from_slice(&[0; 16]),
        }
    }
}
