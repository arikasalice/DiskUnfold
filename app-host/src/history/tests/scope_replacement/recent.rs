use super::*;
use du_core_contracts::analysis_ipc::ScopeSource;

/// Proves the explicit GUI admission accepts the restored fingerprint and rechecks identity.
pub(super) fn scan_restored_scope(
    engine: &Engine,
    events: &du_engine_api::desktop::DesktopEvents,
    restored: RestoredScope,
    target: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    assert_eq!(restored.draft.source, ScopeSource::Recent);
    let mut input = restored.draft;
    input.source = ScopeSource::Gui;
    let input = serde_json::to_value(input)?;
    fs::write(target.join("item.bin"), [0; 23])?;
    let (_, _, scan) = scan_response(engine, &input, &restored.review)?;
    assert!(!scan.existing);
    assert_logical_total(engine, events, &scan, 23)?;

    fs::rename(target, target.with_file_name("previous-directory"))?;
    fs::create_dir(target)?;
    let rejected = ipc(
        engine,
        "scan.start",
        json!({
            "draft": input, "reviewFingerprint": restored.review.review_fingerprint,
        }),
    )?;
    assert_eq!(
        rejected.error.ok_or("replacement was admitted")?.code,
        ErrorCode::StaleVersion
    );
    Ok(())
}
