use du_core_contracts::history_ipc::*;
use du_core_contracts::query_page_ipc::*;
use du_core_contracts::result_events::{
    PublishedCompleteness, ResultVersionChange, ResultVersionChanged,
};
use du_core_contracts::scan_failures::*;
use du_core_contracts::size_units::{
    DISPLAY_DECIMAL_PLACES, IEC_UNITS, SI_UNITS, UNIT_TABLE_REVISION,
};
use du_core_contracts::space_detail_ipc::*;
use du_core_contracts::visualization::geometry::{Geometry, Point};
use du_core_contracts::visualization::model::{ShapeMeta, VizRelationship};
use du_core_contracts::visualization::quantity::{VizAmount, VizDecimal, VizQuantity};
use du_core_contracts::visualization::rows::{
    VizDenominator, VizDenominatorBasis, VizRatio, VizRow, VizRows, VizRowsRequest,
};
use du_core_contracts::volume_gap_ipc::*;
use du_core_contracts::*;
use du_core_contracts::{analysis_ipc::*, job::JobState, result::ResultCompleteness};
use du_core_contracts::{browse_ipc::*, result::ResultItemKind};
use std::{env, fs, io};
use ts_rs::TS;
fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = env::args_os()
        .nth(1)
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "output directory required"))?;
    fs::create_dir_all(&dir)?;
    let config = ts_rs::Config::default();
    let declarations = [
        HistoryEndState::decl(&config),
        HistoryRelation::decl(&config),
        HistoryListFilter::decl(&config),
        HistoryListRequest::decl(&config),
        HistoryListRevision::decl(&config),
        HistoryChangedBody::decl(&config),
        HistoryListCounts::decl(&config),
        HistoryListSummary::decl(&config),
        HistoryListRow::decl(&config),
        HistoryListResponse::decl(&config),
        Id::decl(&config),
        ByteSize::decl(&config),
        Timestamp::decl(&config),
        SizeBasis::decl(&config),
        SizeEvidence::decl(&config),
        SizeAmount::decl(&config),
        SizeValue::decl(&config),
        VizDecimal::decl(&config),
        VizAmount::decl(&config),
        VizQuantity::decl(&config),
        Point::decl(&config),
        Geometry::decl(&config),
        ShapeMeta::decl(&config),
        VizRelationship::decl(&config),
        VizDenominatorBasis::decl(&config),
        VizRatio::decl(&config),
        VizDenominator::decl(&config),
        VizRow::decl(&config),
        VizRowsRequest::decl(&config),
        VizRows::decl(&config),
        PathEncoding::decl(&config),
        PathBytes::decl(&config),
        OsPath::decl(&config),
        ItemRef::decl(&config),
        ErrorCode::decl(&config),
        ErrorEnvelope::decl(&config),
        CallEnvelope::decl(&config),
        ResultEnvelope::decl(&config),
        EventEnvelope::decl(&config),
        ResultVersionChange::decl(&config),
        PublishedCompleteness::decl(&config),
        ResultVersionChanged::decl(&config),
        TargetKind::decl(&config),
        IdentitySource::decl(&config),
        TargetAccess::decl(&config),
        TargetListRequest::decl(&config),
        AnalysisTarget::decl(&config),
        TargetList::decl(&config),
        TargetStatusChanged::decl(&config),
        TargetProbeEvent::decl(&config),
        ScopeSource::decl(&config),
        ScopeBoundary::decl(&config),
        ScopeExclusion::decl(&config),
        ScopeDraft::decl(&config),
        ScopeReviewRequest::decl(&config),
        ScopeCorrespondence::decl(&config),
        ScopeReview::decl(&config),
        ScanStartRequest::decl(&config),
        ScanStart::decl(&config),
        ScanFailureKind::decl(&config),
        ScanFailureStage::decl(&config),
        ScanFailurePathRef::decl(&config),
        ScanFailureRow::decl(&config),
        ScanFailuresRequest::decl(&config),
        ScanFailures::decl(&config),
        JobState::decl(&config),
        AnalysisJobRequest::decl(&config),
        ScanResult::decl(&config),
        AnalysisJobStatus::decl(&config),
        JobControlOutcome::decl(&config),
        AnalysisJobControl::decl(&config),
        SpaceTarget::decl(&config),
        SpaceTotalRequest::decl(&config),
        ResultCompleteness::decl(&config),
        SpaceTotalSize::decl(&config),
        SpaceTotal::decl(&config),
        VolumeGapSelection::decl(&config),
        VolumeGapRequest::decl(&config),
        VolumeGapObservation::decl(&config),
        VolumeGapSize::decl(&config),
        VolumeGapAmount::decl(&config),
        VolumeGapRow::decl(&config),
        VolumeGapCategories::decl(&config),
        VolumeGapResponse::decl(&config),
        ResultItemKind::decl(&config),
        BrowseValue::<String>::decl(&config),
        BrowseOrigin::decl(&config),
        BrowseDetailRequest::decl(&config),
        BrowseDetail::decl(&config),
        QueryPagePosition::decl(&config),
        QueryPageContinuation::decl(&config),
        QueryPageRequest::decl(&config),
        SpaceDetailSize::decl(&config),
        SpaceLinkGroup::decl(&config),
        SpaceExclusiveDecision::decl(&config),
        SpacePlaceholderState::decl(&config),
        SpaceAuxiliary::decl(&config),
        SpaceCapabilityState::decl(&config),
        SpaceDetailCapability::decl(&config),
        SpaceDetail::decl(&config),
    ];
    let output = declarations
        .into_iter()
        .map(|d| format!("export {d}\n"))
        .collect::<String>();
    let mut output = output
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n")
        + "\n";
    let tables = serde_json::json!({
        "revision": UNIT_TABLE_REVISION,
        "decimalPlaces": DISPLAY_DECIMAL_PLACES,
        "si": SI_UNITS,
        "iec": IEC_UNITS,
    });
    output.push_str(&format!(
        "export const SIZE_UNIT_RULES = {tables} as const;\n"
    ));
    fs::write(std::path::Path::new(&dir).join("contracts.ts"), output)?;
    Ok(())
}
