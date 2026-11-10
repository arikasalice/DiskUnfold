use du_core_contracts::result_events::{
    PublishedCompleteness, ResultVersionChange, ResultVersionChanged,
};
use du_core_contracts::scan_failures::*;
use du_core_contracts::visualization::geometry::{Geometry, Point};
use du_core_contracts::visualization::model::{ShapeMeta, VizRelationship};
use du_core_contracts::visualization::quantity::{VizAmount, VizDecimal, VizQuantity};
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
        ResultItemKind::decl(&config),
        BrowseValue::<String>::decl(&config),
        BrowseOrigin::decl(&config),
        BrowseDetailRequest::decl(&config),
        BrowseDetail::decl(&config),
    ];
    let output = declarations
        .into_iter()
        .map(|d| format!("export {d}\n"))
        .collect::<String>();
    fs::write(std::path::Path::new(&dir).join("contracts.ts"), output)?;
    Ok(())
}
