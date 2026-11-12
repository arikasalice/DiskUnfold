use super::*;
use du_core_contracts::{CallEnvelope, Id};
use du_engine_api::api::EngineBuilder;

#[test]
fn registration_table_has_the_fixed_order_and_only_permitted_blocking_rows() {
    assert_eq!(validate(&REGISTRATIONS), Ok(()));
    assert_eq!(
        REGISTRATIONS.map(|row| row.check),
        [
            StartupCheck::ResourceIntegrity,
            StartupCheck::Webview,
            StartupCheck::DataFormatAndOpen,
            StartupCheck::TemporaryFiles,
            StartupCheck::SupportMatrix,
            StartupCheck::Capability,
            StartupCheck::FullDiskAccess,
        ]
    );
}

#[test]
fn reject_missing_duplicate_reordered_network_and_misclassified_rows() {
    assert_eq!(
        validate(&REGISTRATIONS[..6]),
        Err(RegistrationError::MissingOrExtra)
    );

    let mut duplicate = REGISTRATIONS;
    duplicate[1] = duplicate[0];
    assert_eq!(validate(&duplicate), Err(RegistrationError::Duplicate));

    let mut reordered = REGISTRATIONS;
    reordered.swap(0, 1);
    assert_eq!(validate(&reordered), Err(RegistrationError::Order));

    let mut online = REGISTRATIONS;
    online[4].network_dependent = true;
    assert_eq!(validate(&online), Err(RegistrationError::NetworkDependency));

    let mut advisory_blocks = REGISTRATIONS;
    advisory_blocks[4].blocking = true;
    assert_eq!(
        validate(&advisory_blocks),
        Err(RegistrationError::BlockingPolicy)
    );

    let mut required_does_not_block = REGISTRATIONS;
    required_does_not_block[2].blocking = false;
    assert_eq!(
        validate(&required_does_not_block),
        Err(RegistrationError::BlockingPolicy)
    );
}

#[test]
fn missing_observations_are_unverifiable_and_do_not_become_a_pass()
-> Result<(), Box<dyn std::error::Error>> {
    let report =
        status(true, StartupAccessState::Unknown, &[]).map_err(|error| format!("{error:?}"))?;
    assert_eq!(report.grade, StartupGrade::Unverifiable);
    assert_eq!(report.checks.len(), 7);
    assert!(report.checks.iter().all(|row| {
        row.check == StartupCheck::FullDiskAccess || row.result == StartupCheckOutcome::Unverifiable
    }));
    assert!(report.first_run);
    assert!(report.analysis_allowed);
    assert_eq!(report.access, StartupAccessState::Unknown);
    Ok(())
}

#[test]
fn opening_an_existing_database_does_not_prove_data_directory_writability() {
    assert_eq!(
        data_format_and_open_observation(true).result,
        StartupCheckOutcome::Pass
    );
    assert_eq!(
        data_format_and_open_observation(false).result,
        StartupCheckOutcome::Unverifiable
    );
}

#[test]
fn advisory_grade_uses_the_worst_applicable_result_and_ignores_access_row() {
    let rows = [
        StartupCheckResult {
            check: StartupCheck::ResourceIntegrity,
            result: StartupCheckOutcome::Pass,
            reason_key: None,
        },
        StartupCheckResult {
            check: StartupCheck::TemporaryFiles,
            result: StartupCheckOutcome::Limited,
            reason_key: Some("export.temp_file_left".into()),
        },
        StartupCheckResult {
            check: StartupCheck::SupportMatrix,
            result: StartupCheckOutcome::OutsideVerified,
            reason_key: Some("support.outside_verified".into()),
        },
        StartupCheckResult {
            check: StartupCheck::FullDiskAccess,
            result: StartupCheckOutcome::Blocked,
            reason_key: Some("startup.full_disk_access_required".into()),
        },
    ];
    assert_eq!(grade(&rows), Some(StartupGrade::OutsideVerified));
}

#[test]
fn empty_grade_inputs_are_unverifiable_by_the_status_builder() {
    assert_eq!(grade(&[]), None);
    assert_eq!(
        status(false, StartupAccessState::Unknown, &[]).map(|report| report.grade),
        Ok(StartupGrade::Unverifiable)
    );
}

#[test]
fn only_observed_denial_blocks_analysis_entry() {
    assert!(
        status(false, StartupAccessState::Denied, &[]).is_ok_and(
            |report| !report.analysis_allowed && report.access == StartupAccessState::Denied
        )
    );

    assert!(
        status(false, StartupAccessState::Unknown, &[]).is_ok_and(|report| report.analysis_allowed)
    );
}

#[test]
fn technical_blockers_cannot_be_downgraded_to_an_unverifiable_status() {
    let blocked = StartupCheckResult {
        check: StartupCheck::DataFormatAndOpen,
        result: StartupCheckOutcome::Blocked,
        reason_key: Some("startup.data_dir_unwritable".into()),
    };
    assert_eq!(
        status(false, StartupAccessState::NotApplicable, &[blocked]),
        Err(StatusError::RequiredCheckBlocked(
            StartupCheck::DataFormatAndOpen
        ))
    );
}

#[test]
fn advisory_checks_cannot_emit_blocked_and_access_denial_is_analysis_only() {
    let blocked_advisory = StartupCheckResult {
        check: StartupCheck::SupportMatrix,
        result: StartupCheckOutcome::Blocked,
        reason_key: Some("support.outside_verified".into()),
    };
    assert_eq!(
        status(false, StartupAccessState::Unknown, &[blocked_advisory]),
        Err(StatusError::AdvisoryCheckBlocked(
            StartupCheck::SupportMatrix
        ))
    );

    assert!(
        status(false, StartupAccessState::Denied, &[]).is_ok_and(|report| {
            let access = report.checks.last();
            !report.analysis_allowed
                && access.is_some_and(|row| {
                    row.check == StartupCheck::FullDiskAccess
                        && row.result == StartupCheckOutcome::Blocked
                        && row.reason_key.as_deref() == Some("startup.full_disk_access_required")
                })
        })
    );
}

#[test]
fn gui_startup_queries_return_observations_without_claiming_os_success()
-> Result<(), Box<dyn std::error::Error>> {
    let report =
        status(false, StartupAccessState::Unknown, &[]).map_err(|error| format!("{error:?}"))?;
    let mut builder = EngineBuilder::default();
    register(&mut builder, report.clone());
    let engine = builder.build()?;
    let correlation_id = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;

    let status = engine.call(
        CallEnvelope {
            name: "app.startup_status".into(),
            contract_version: 1,
            correlation_id: correlation_id.clone(),
            body: serde_json::json!({}),
        },
        &CallerCtx::new(Channel::Gui, correlation_id.clone(), None),
    );
    assert!(status.error.is_none());
    assert_eq!(status.body, serde_json::to_value(report)?);

    let response = engine.call(
        CallEnvelope {
            name: "app.startup_access".into(),
            contract_version: 1,
            correlation_id: correlation_id.clone(),
            body: serde_json::json!({"action":"open_privacy_settings"}),
        },
        &CallerCtx::new(Channel::Gui, correlation_id.clone(), None),
    );
    assert!(response.error.is_none());
    assert_eq!(response.body["access"], "unknown");
    assert_eq!(response.body["analysisAllowed"], true);
    assert_eq!(response.body["settingsOpened"], false);
    Ok(())
}
