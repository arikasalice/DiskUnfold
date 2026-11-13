use super::*;

struct ScanStartCounter(Arc<AtomicUsize>);

impl UseCase for ScanStartCounter {
    const NAME: &'static str = "scan.start";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = ScanStartRequest;
    type Response = Value;

    fn check_scope(&self, _: &ScanStartRequest, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: ScanStartRequest, _: &CallerCtx) -> Result<Value, EngineError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"admitted": true}))
    }
}

struct ResumeCounter(Arc<AtomicUsize>);

impl UseCase for ResumeCounter {
    const NAME: &'static str = "job.resume";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = AnalysisJobRequest;
    type Response = Value;

    fn check_scope(&self, _: &AnalysisJobRequest, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: AnalysisJobRequest, _: &CallerCtx) -> Result<Value, EngineError> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(json!({"resumed": true}))
    }
}

struct StatusKind {
    kind: Option<&'static str>,
    fail: bool,
    revoke_access: Option<Arc<MutableStartupCapabilities>>,
}

impl UseCase for StatusKind {
    const NAME: &'static str = "job.status";
    const CHANNELS: &'static [Channel] = &[Channel::Gui];
    type Request = AnalysisJobRequest;
    type Response = Value;

    fn check_scope(&self, _: &AnalysisJobRequest, _: &CallerCtx) -> Result<(), EngineError> {
        Ok(())
    }

    fn execute(&self, _: AnalysisJobRequest, _: &CallerCtx) -> Result<Value, EngineError> {
        if self.fail {
            return Err(EngineError::new("job.not_found", Vec::new()));
        }
        if let Some(access) = &self.revoke_access {
            access.set(FullDiskAccessStatus::Denied);
        }
        Ok(json!({"kind": self.kind}))
    }
}

struct MutableStartupCapabilities(AtomicUsize);

impl MutableStartupCapabilities {
    fn new(access: FullDiskAccessStatus) -> Self {
        Self(AtomicUsize::new(encode_access(access)))
    }

    fn set(&self, access: FullDiskAccessStatus) {
        self.0.store(encode_access(access), Ordering::SeqCst);
    }
}

impl StartupCapabilitiesPort for MutableStartupCapabilities {
    fn full_disk_access_status(&self) -> FullDiskAccessStatus {
        match self.0.load(Ordering::SeqCst) {
            0 => FullDiskAccessStatus::Granted,
            1 => FullDiskAccessStatus::Denied,
            2 => FullDiskAccessStatus::Unknown,
            _ => FullDiskAccessStatus::NotApplicable,
        }
    }

    fn webview_availability(&self) -> WebViewAvailability {
        WebViewAvailability::Unknown
    }

    fn open_privacy_settings(&self) -> PrivacySettingsOpenOutcome {
        PrivacySettingsOpenOutcome::Unsupported
    }

    fn show_pre_window_error_dialog(&self, _: &str, _: &str) -> StartupDialogOutcome {
        StartupDialogOutcome::Unsupported
    }
}

fn encode_access(access: FullDiskAccessStatus) -> usize {
    match access {
        FullDiskAccessStatus::Granted => 0,
        FullDiskAccessStatus::Denied => 1,
        FullDiskAccessStatus::Unknown => 2,
        FullDiskAccessStatus::NotApplicable => 3,
    }
}

fn denied_access() -> crate::startup_checks::StartupAccessSession {
    let capabilities = Arc::new(FakeStartupCapabilities::new(
        FullDiskAccessStatus::Denied,
        WebViewAvailability::Unknown,
        PrivacySettingsOpenOutcome::RequestAccepted,
        StartupDialogOutcome::Acknowledged,
    ));
    crate::startup_checks::StartupAccessSession::new(capabilities)
}

fn call_message(name: &str, body: Value) -> Value {
    json!({"command":"du_call", "args":{"envelope":{
        "name":name,
        "contractVersion":1,
        "correlationId":"0195f856-ab83-7a59-9343-10d29775c9a9",
        "body":body,
    }}})
}

#[test]
fn denied_full_disk_access_blocks_valid_scan_start_before_engine_admission()
-> Result<(), Box<dyn std::error::Error>> {
    let calls = Arc::new(AtomicUsize::new(0));
    let mut builder = EngineBuilder::default();
    builder.register(ScanStartCounter(calls.clone()));
    let engine = builder.build()?;
    let access = denied_access();
    let body = json!({
        "draft": {
            "includes": [], "exclusions": [], "boundary": "include_selected",
            "includedMounts": [], "source": "gui"
        },
        "reviewFingerprint": "reviewed"
    });
    let reply: Value = serde_json::from_str(
        &dispatch_with_startup_access(
            &engine,
            &call_message("scan.start", body).to_string(),
            &access,
        )
        .ok_or("missing denied scan response")?,
    )?;

    assert_eq!(
        reply["error"]["reasonKey"],
        "startup.full_disk_access_required"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn scan_start_refreshes_permission_and_keeps_unknown_permissive()
-> Result<(), Box<dyn std::error::Error>> {
    let body = json!({
        "draft": {
            "includes": [], "exclusions": [], "boundary": "include_selected",
            "includedMounts": [], "source": "gui"
        },
        "reviewFingerprint": "reviewed"
    });
    for (initial, current, expected_reason, expected_starts) in [
        (
            FullDiskAccessStatus::Granted,
            FullDiskAccessStatus::Denied,
            Some("startup.full_disk_access_required"),
            0,
        ),
        (
            FullDiskAccessStatus::Granted,
            FullDiskAccessStatus::Granted,
            None,
            1,
        ),
        (
            FullDiskAccessStatus::Unknown,
            FullDiskAccessStatus::Unknown,
            None,
            1,
        ),
    ] {
        let capabilities = Arc::new(MutableStartupCapabilities::new(initial));
        let access = crate::startup_checks::StartupAccessSession::new(capabilities.clone());
        capabilities.set(current);
        let starts = Arc::new(AtomicUsize::new(0));
        let mut builder = EngineBuilder::default();
        builder.register(ScanStartCounter(starts.clone()));
        let engine = builder.build()?;
        let reply: Value = serde_json::from_str(
            &dispatch_with_startup_access(
                &engine,
                &call_message("scan.start", body.clone()).to_string(),
                &access,
            )
            .ok_or("missing scan response")?,
        )?;

        if let Some(reason) = expected_reason {
            assert_eq!(reply["error"]["reasonKey"], reason);
        } else {
            assert!(reply.get("error").is_none());
        }
        assert_eq!(starts.load(Ordering::SeqCst), expected_starts);
    }
    Ok(())
}

#[test]
fn denied_access_allows_only_confirmed_non_analysis_resume_kinds()
-> Result<(), Box<dyn std::error::Error>> {
    let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
    for (kind, expected_reason, expected_resumes) in [
        (
            Some("scan.metadata"),
            "startup.full_disk_access_required",
            0,
        ),
        (Some("target.probe"), "", 1),
        (Some("query.evaluate"), "", 1),
        (
            Some("duplicate.check"),
            "startup.full_disk_access_required",
            0,
        ),
        (
            Some("future.analysis"),
            "startup.full_disk_access_required",
            0,
        ),
        (None, "startup.full_disk_access_required", 0),
    ] {
        let resumes = Arc::new(AtomicUsize::new(0));
        let mut builder = EngineBuilder::default();
        builder.register(StatusKind {
            kind,
            fail: false,
            revoke_access: None,
        });
        builder.register(ResumeCounter(resumes.clone()));
        let engine = builder.build()?;
        let reply: Value = serde_json::from_str(
            &dispatch_with_startup_access(
                &engine,
                &call_message("job.resume", json!({"jobRef":id})).to_string(),
                &denied_access(),
            )
            .ok_or("missing resume response")?,
        )?;

        if expected_reason.is_empty() {
            assert!(reply.get("error").is_none());
        } else {
            assert_eq!(reply["error"]["reasonKey"], expected_reason);
        }
        assert_eq!(resumes.load(Ordering::SeqCst), expected_resumes);
    }
    Ok(())
}

#[test]
fn granted_and_unknown_access_allow_analysis_resume() -> Result<(), Box<dyn std::error::Error>> {
    let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
    for state in [FullDiskAccessStatus::Granted, FullDiskAccessStatus::Unknown] {
        let capabilities = Arc::new(MutableStartupCapabilities::new(state));
        let access = crate::startup_checks::StartupAccessSession::new(capabilities);
        let resumes = Arc::new(AtomicUsize::new(0));
        let mut builder = EngineBuilder::default();
        builder.register(StatusKind {
            kind: Some("scan.metadata"),
            fail: false,
            revoke_access: None,
        });
        builder.register(ResumeCounter(resumes.clone()));
        let engine = builder.build()?;

        let reply: Value = serde_json::from_str(
            &dispatch_with_startup_access(
                &engine,
                &call_message("job.resume", json!({"jobRef":id})).to_string(),
                &access,
            )
            .ok_or("missing resume response")?,
        )?;

        assert!(reply.get("error").is_none());
        assert_eq!(resumes.load(Ordering::SeqCst), 1);
    }
    Ok(())
}

#[test]
fn denied_access_fails_closed_when_resume_kind_cannot_be_read()
-> Result<(), Box<dyn std::error::Error>> {
    let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
    let resumes = Arc::new(AtomicUsize::new(0));
    let mut builder = EngineBuilder::default();
    builder.register(StatusKind {
        kind: None,
        fail: true,
        revoke_access: None,
    });
    builder.register(ResumeCounter(resumes.clone()));
    let engine = builder.build()?;
    let reply: Value = serde_json::from_str(
        &dispatch_with_startup_access(
            &engine,
            &call_message("job.resume", json!({"jobRef":id})).to_string(),
            &denied_access(),
        )
        .ok_or("missing status failure response")?,
    )?;

    assert_eq!(reply["error"]["reasonKey"], "job.not_found");
    assert_eq!(reply["name"], "job.resume");
    assert_eq!(resumes.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn analysis_resume_rechecks_access_after_status_lookup() -> Result<(), Box<dyn std::error::Error>> {
    let id = "0195f856-ab83-7a59-9343-10d29775c9a9";
    let capabilities = Arc::new(MutableStartupCapabilities::new(
        FullDiskAccessStatus::Granted,
    ));
    let access = crate::startup_checks::StartupAccessSession::new(capabilities.clone());
    let resumes = Arc::new(AtomicUsize::new(0));
    let mut builder = EngineBuilder::default();
    builder.register(StatusKind {
        kind: Some("scan.metadata"),
        fail: false,
        revoke_access: Some(capabilities),
    });
    builder.register(ResumeCounter(resumes.clone()));
    let engine = builder.build()?;

    let reply: Value = serde_json::from_str(
        &dispatch_with_startup_access(
            &engine,
            &call_message("job.resume", json!({"jobRef":id})).to_string(),
            &access,
        )
        .ok_or("missing resume response")?,
    )?;

    assert_eq!(
        reply["error"]["reasonKey"],
        "startup.full_disk_access_required"
    );
    assert_eq!(resumes.load(Ordering::SeqCst), 0);
    Ok(())
}

#[test]
fn startup_access_commands_reject_non_gui_callers() -> Result<(), Box<dyn std::error::Error>> {
    let capabilities = Arc::new(FakeStartupCapabilities::new(
        FullDiskAccessStatus::Denied,
        WebViewAvailability::Unknown,
        PrivacySettingsOpenOutcome::RequestAccepted,
        StartupDialogOutcome::Acknowledged,
    ));
    let access = Arc::new(crate::startup_checks::StartupAccessSession::new(
        capabilities.clone(),
    ));
    let report = crate::startup_checks::status(false, StartupAccessState::Denied, &[])
        .map_err(|error| format!("{error:?}"))?;
    let mut builder = EngineBuilder::default();
    crate::startup_checks::register(&mut builder, report, access);
    let engine = builder.build()?;
    let correlation_id = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;

    let response = engine.call(
        CallEnvelope {
            name: "app.startup_access".into(),
            contract_version: 1,
            correlation_id: correlation_id.clone(),
            body: json!({"action":"open_privacy_settings"}),
        },
        &CallerCtx::new(Channel::Mcp, correlation_id, None),
    );

    assert!(response.error.is_some());
    assert_eq!(capabilities.privacy_settings_requests(), 0);
    Ok(())
}
