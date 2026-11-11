use du_core_contracts::browse_open_ipc::*;
use du_core_contracts::browse_page_ipc::*;
use du_core_contracts::history_ipc::*;
use du_core_contracts::query_page_ipc::*;
use du_core_contracts::query_region_ipc::*;
use du_core_contracts::result_events::{
    PublishedCompleteness, ResultVersionChange, ResultVersionChanged,
};
use du_core_contracts::scan_failures::*;
use du_core_contracts::settings_ipc::*;
use du_core_contracts::size_units::{
    DISPLAY_DECIMAL_PLACES, IEC_UNITS, SI_UNITS, UNIT_TABLE_REVISION,
};
use du_core_contracts::space_detail_ipc::*;
use du_core_contracts::visualization::cancel::{
    VizCancelRequest, VizCancelResponse, VizCancelTarget, VizControlResult,
};
use du_core_contracts::visualization::geometry::{Geometry, Point};
use du_core_contracts::visualization::locate::{VizLocate, VizLocateRequest};
use du_core_contracts::visualization::model::{
    ShapeMeta, VizLegendEntry, VizMatchKey, VizModel, VizRelationship, VizRepresentation, VizShape,
    VizStaleness, VizStalenessState, VizSummary, VizTableRevisions, VizTotal, VizTotals,
};
use du_core_contracts::visualization::quantity::{VizAmount, VizDecimal, VizQuantity};
use du_core_contracts::visualization::render::{
    VizColorMode, VizFallback, VizLabelMode, VizRenderNonModelResponse,
};
use du_core_contracts::visualization::request::{
    VizCondition, VizExpansion, VizRenderRequest, VizSourceRef, VizViewChannel, VizViewOpenRequest,
    VizViewport, VizVisibleWindow,
};
use du_core_contracts::visualization::response::{VizRenderModelResponse, VizRenderResponse};
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
        DisplayPreferencesRequest::decl(&config),
        DisplayUnitSystem::decl(&config),
        DisplayPreferences::decl(&config),
        SettingsRestoreRequest::decl(&config),
        SettingsRestoreSource::decl(&config),
        SettingsRestoreItem::decl(&config),
        SettingsRestoreStatus::decl(&config),
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
        HistoryDetailRequest::decl(&config),
        HistoryWorkflow::decl(&config),
        HistoryFollowUp::decl(&config),
        HistoryEffectValue::decl(&config),
        HistoryEffects::decl(&config),
        HistoryDetailCompleteness::decl(&config),
        HistoryDetail::decl(&config),
        HistoryItemOutcome::decl(&config),
        HistoryItemsRequest::decl(&config),
        HistoryDisplacedItem::decl(&config),
        HistoryItemResult::decl(&config),
        HistoryItemRow::decl(&config),
        HistoryItemCounts::decl(&config),
        HistoryItemsResponse::decl(&config),
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
        VizFallback::decl(&config),
        VizRenderNonModelResponse::decl(&config),
        VizColorMode::decl(&config),
        VizLabelMode::decl(&config),
        VizCancelRequest::decl(&config),
        VizCancelTarget::decl(&config),
        VizControlResult::decl(&config),
        VizCancelResponse::decl(&config),
        VizViewOpenRequest::decl(&config),
        VizViewChannel::decl(&config),
        VizSourceRef::decl(&config),
        VizCondition::decl(&config),
        VizViewport::decl(&config),
        VizVisibleWindow::decl(&config),
        VizExpansion::decl(&config),
        VizRenderRequest::decl(&config),
        VizStalenessState::decl(&config),
        VizStaleness::decl(&config),
        VizRepresentation::decl(&config),
        VizTotal::decl(&config),
        VizTotals::decl(&config),
        VizTableRevisions::decl(&config),
        VizMatchKey::decl(&config),
        VizShape::decl(&config),
        VizLegendEntry::decl(&config),
        VizSummary::decl(&config),
        VizModel::decl(&config),
        VizRenderModelResponse::decl(&config),
        VizRenderResponse::decl(&config),
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
        VizLocateRequest::decl(&config),
        VizLocate::decl(&config),
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
        du_core_contracts::job::JobControl::decl(&config),
        AnalysisJobRequest::decl(&config),
        AnalysisJobListRequest::decl(&config),
        AnalysisJobPriority::decl(&config),
        AnalysisJobWaitReason::decl(&config),
        AnalysisJobListRow::decl(&config),
        ScanResult::decl(&config),
        AnalysisJobTerminal::decl(&config),
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
        BrowseSourceKind::decl(&config),
        BrowseSourceRef::decl(&config),
        BrowseViewKind::decl(&config),
        BrowseSortKey::decl(&config),
        QueryNameOp::decl(&config),
        QueryNameEncoding::decl(&config),
        QueryItemType::decl(&config),
        QuerySizeBasis::decl(&config),
        QuerySizeOp::decl(&config),
        QueryPredicate::decl(&config),
        QueryCondition::decl(&config),
        BrowseOpenRequest::decl(&config),
        ResultOrigin::decl(&config),
        QueryStrategy::decl(&config),
        IdentityCapability::decl(&config),
        CorrespondenceKey::decl(&config),
        FolderFlags::decl(&config),
        FolderQueryRow::decl(&config),
        SearchQueryRow::decl(&config),
        QueryTotal::decl(&config),
        FolderRowKind::decl(&config),
        SearchRowKind::decl(&config),
        QueryPage::<FolderQueryRow>::decl(&config),
        FolderQueryPage::decl(&config),
        SearchQueryPage::decl(&config),
        QueryPageResponse::decl(&config),
        BrowseBreadcrumb::decl(&config),
        BrowseRootKind::decl(&config),
        BrowseContext::decl(&config),
        BrowseOpenResponse::decl(&config),
        BrowseDetailRequest::decl(&config),
        BrowseDetail::decl(&config),
        BrowseCopyPathRequest::decl(&config),
        BrowseCopyPathRefusal::decl(&config),
        BrowseCopyPathOutcome::decl(&config),
        QueryPagePosition::decl(&config),
        QueryPageContinuation::decl(&config),
        QueryPageRequest::decl(&config),
        QueryRegionKind::decl(&config),
        QueryRegionOpenRequest::decl(&config),
        QueryRegionOpened::decl(&config),
        QueryRegionCloseRequest::decl(&config),
        QueryRegionClosed::decl(&config),
        QueryCancelTarget::decl(&config),
        QueryCancelRequest::decl(&config),
        QueryCancelOutcome::decl(&config),
        QueryCancelled::decl(&config),
        SpaceDetailRequest::decl(&config),
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
