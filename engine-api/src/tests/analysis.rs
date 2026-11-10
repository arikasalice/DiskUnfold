use super::*;
use du_analysis_index::{FileIdentity, IngestSink, Observation, SharingEvidence, SpaceObservation};
use du_core_contracts::{
    ByteSize, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_job_runtime::{JobClock, JobEvents, JobIdentity, JobRuntime, Submission};
use std::sync::Arc;

#[test]
fn space_detail_preserves_observation_time_and_leaves_legacy_time_unknown()
-> Result<(), Box<dyn std::error::Error>> {
    let scope = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let version = Id::parse("0195f856-ab83-7a59-9343-10d29775c9aa")?;
    let store = IngestSink::default();
    let mut build = store
        .open_forest(
            scope.clone(),
            vec![ResultScopeRoot {
                path: ResultNativePath {
                    encoding: PathEncoding::Unix,
                    raw: PathBytes::from_bytes(b"/root"),
                },
                volume_id: vec![1],
                file_id: vec![2],
            }],
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    for (name, file_id, links) in [(b"item".as_slice(), 3, 1), (b"outside", 4, 2)] {
        build
            .add_with_space_at(
                0,
                Observation {
                    path: vec![name.to_vec()],
                    size: Some(SizeValue {
                        basis: SizeBasis::Logical,
                        evidence: SizeEvidence::Measured,
                        amount: SizeAmount::Exact { bytes: 0.into() },
                    }),
                },
                SpaceObservation {
                    allocated: Some(SizeValue {
                        basis: SizeBasis::Allocated,
                        evidence: SizeEvidence::OsReported,
                        amount: SizeAmount::Exact { bytes: 0.into() },
                    }),
                    identity: Some(FileIdentity {
                        volume: vec![1],
                        file: vec![file_id],
                    }),
                    link_count: Some(links),
                    sharing: SharingEvidence::None,
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    let outside_path = [b"outside".to_vec()];
    let observed = du_core_contracts::Timestamp::parse("2026-01-01T12:00:01.002Z")?;
    let modified = du_core_contracts::Timestamp::parse("2020-01-01T12:00:00.000Z")?;
    build
        .set_item_observed_at(0, &outside_path, observed.clone())
        .map_err(|e| format!("{e:?}"))?;
    build
        .set_modified_at(0, &outside_path, modified)
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal_at(
            version.clone(),
            ResultCompleteness::Complete,
            du_core_contracts::Timestamp::from_unix_millis(10),
        )
        .map_err(|e| format!("{e:?}"))?;
    let mut builder = EngineBuilder::default();
    crate::space::detail::register(&mut builder, store);
    let engine = builder.build()?;
    let read = |item_version: &Id, item_no: u32| {
        call(
            &engine,
            "space.detail",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"resultVersion":version,"item":{"resultVersion":item_version,"itemNo":item_no}}),
        )
    };
    let file = read(&version, 1)?;
    assert!(file.error.is_none(), "{:?}", file.error);
    assert_eq!(file.body["logical"]["value"]["size"]["bytes"], "0");
    assert_eq!(file.body["shared"]["value"]["size"]["bytes"], "0");
    assert_eq!(file.body["exclusiveDecision"]["confirmed"], true);
    assert_eq!(file.body["linkGroup"]["value"]["reportedLinkCount"], "1");
    assert_eq!(
        file.body["linkGroup"]["value"]["inScopePaths"][0]["itemNo"],
        1
    );
    assert_eq!(file.body["observedAt"]["state"], "unknown");
    assert_eq!(
        file.body["auxiliary"]["reasonKey"],
        "space.aux_not_included"
    );
    assert!(file.body.get("placeholderLocalState").is_none());
    let outside = read(&version, 2)?;
    assert!(outside.error.is_none(), "{:?}", outside.error);
    assert_eq!(
        outside.body["observedAt"],
        json!({"state":"present","value":observed})
    );
    assert_eq!(outside.body["shared"]["value"]["size"]["bytes"], "0");
    assert_eq!(outside.body["exclusive"]["state"], "unknown");
    assert_eq!(
        outside.body["exclusiveDecision"]["reasonKey"],
        "space.out_of_range_links"
    );
    assert_eq!(read(&version, 0)?.body["exclusive"]["state"], "absent");
    assert_eq!(
        read(&version, 9)?
            .error
            .ok_or("missing item accepted")?
            .reason_key,
        "ref.unavailable"
    );
    assert_eq!(
        read(&scope, 1)?
            .error
            .ok_or("wrong version accepted")?
            .reason_key,
        "result.requery_required"
    );
    Ok(())
}

struct JobFixture(Id);

impl JobIdentity for JobFixture {
    fn next_id(&self) -> Id {
        self.0.clone()
    }
}

impl JobClock for JobFixture {
    fn now_ms(&self) -> i64 {
        1
    }
}

impl JobEvents for JobFixture {
    fn publish(&self, _: du_job_runtime::JobEvent) {}
}

impl du_core_contracts::job::JobHistory for JobFixture {
    fn append(
        &self,
        _: &du_core_contracts::job::JobSummary,
    ) -> Result<du_core_contracts::job::HistoryAppend, String> {
        Err("test fixture has no history".into())
    }
}

impl du_core_contracts::job::JobBody for JobFixture {
    fn kind_id(&self) -> &str {
        "scan.metadata"
    }

    fn run(
        &self,
        _: Arc<dyn du_core_contracts::job::JobContext>,
    ) -> du_core_contracts::job::JobOutcome {
        du_core_contracts::job::JobOutcome {
            counts: Default::default(),
            completed_scope: true,
            failure_reason_key: None,
            unprocessed_summary: None,
            result_version_id: Some(self.0.clone()),
        }
    }
}

#[test]
fn job_commands_report_the_committed_state_and_reject_unknown_references()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::job::{HistoryMode, JobControl, JobDescriptor, ProgressUnit};
    let job = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let result = Id::parse("0195f856-ab83-7a59-9343-10d29775c9aa")?;
    let fixture = Arc::new(JobFixture(job.clone()));
    let runtime = Arc::new(JobRuntime::new(
        fixture.clone(),
        fixture.clone(),
        fixture.clone(),
        fixture,
    )?);
    runtime.register(JobDescriptor {
        kind_id: "scan.metadata".into(),
        version: 1,
        display_name_key: "scan.metadata".into(),
        controls: vec![JobControl::Cancel],
        safe_points: vec!["item".into()],
        progress_unit: ProgressUnit::Items,
        resource_classes: vec![],
        history: HistoryMode::None,
    })?;
    let mut builder = EngineBuilder::default();
    crate::jobs::register(&mut builder, runtime.clone());
    let engine = builder.build()?;
    let command = |name: &str, id: &Id| {
        call(
            &engine,
            name,
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({ "jobRef": id }),
        )
    };

    assert_eq!(
        command("job.status", &job)?
            .error
            .ok_or("missing error")?
            .reason_key,
        "job.not_found"
    );
    runtime.submit(Submission {
        kind_id: "scan.metadata".into(),
        deduplication_key: None,
        body: Arc::new(JobFixture(result.clone())),
        priority: du_job_runtime::JobPriority::Normal,
        scope_summary: "fixture".into(),
        correlation_id: job.clone(),
        sample: false,
    })?;
    let queued = command("job.status", &job)?.body;
    assert_eq!(queued["state"], "queued");
    assert_eq!(queued["sequence"], 1);
    assert!(queued["resultVersion"].is_null());
    assert!(queued.get("scanResult").is_none());

    assert_eq!(runtime.run_next()?, Some(job.clone()));
    let finished = command("job.status", &job)?.body;
    assert_eq!(finished["state"], "done");
    assert_eq!(finished["resultVersion"], json!(result));
    let cancel = command("job.cancel", &job)?.body;
    assert_eq!(cancel["outcome"], "already_terminal");
    assert_eq!(cancel["state"], finished["state"]);
    assert_eq!(cancel["sequence"], finished["sequence"]);
    Ok(())
}

#[test]
fn space_total_honors_admission_version_and_preserves_unknown_basis()
-> Result<(), Box<dyn std::error::Error>> {
    let scope = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let version = Id::parse("0195f856-ab83-7a59-9343-10d29775c9aa")?;
    let store = IngestSink::default();
    let mut builder = EngineBuilder::default();
    crate::space::register_total(&mut builder, store.clone());
    let engine = builder.build()?;
    let read = |result: &Id| {
        call(
            &engine,
            "space.total",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({
                "resultVersion": result,
                "target": {"kind": "scope_root"},
                "requestSeq": 9
            }),
        )
    };
    assert_eq!(
        read(&version)?.error.ok_or("missing error")?.reason_key,
        "ref.unavailable"
    );

    let mut ingest = store.open(scope.clone()).map_err(|e| format!("{e:?}"))?;
    ingest
        .add_structure(Vec::new())
        .map_err(|e| format!("{e:?}"))?;
    ingest
        .add(Observation {
            path: vec![b"file".to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::OsReported,
                amount: SizeAmount::Exact {
                    bytes: ByteSize::from(47),
                },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    ingest
        .seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;

    let response = read(&version)?;
    assert!(response.error.is_none());
    assert_eq!(response.body["resultVersion"], json!(version));
    assert_eq!(response.body["requestSeq"], 9);
    assert_eq!(response.body["sizes"][0]["size"]["bytes"], "47");
    assert_eq!(response.body["sizes"][1]["size"]["state"], "unknown");
    assert_eq!(response.body["sizes"][2]["size"]["state"], "unknown");
    assert_eq!(
        read(&scope)?.error.ok_or("missing stale error")?.reason_key,
        "result.requery_required"
    );
    let allocated = Id::parse("0195f856-ab83-7a59-9343-10d29775c9ab")?;
    let mut ingest = store.open(scope).map_err(|e| format!("{e:?}"))?;
    ingest.add_structure(vec![]).map_err(|e| format!("{e:?}"))?;
    ingest
        .add_with_space(
            Observation {
                path: vec![b"file".to_vec()],
                size: None,
            },
            SpaceObservation {
                allocated: Some(SizeValue {
                    basis: SizeBasis::Allocated,
                    evidence: SizeEvidence::OsReported,
                    amount: SizeAmount::Exact {
                        bytes: ByteSize::from(4096),
                    },
                }),
                identity: Some(FileIdentity {
                    volume: vec![1],
                    file: vec![2],
                }),
                link_count: Some(1),
                sharing: SharingEvidence::Unknown,
            },
        )
        .map_err(|e| format!("{e:?}"))?;
    ingest
        .seal(allocated.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let response = read(&allocated)?;
    assert_eq!(response.body["sizes"][1]["size"]["bytes"], "4096");
    assert_eq!(response.body["sizes"][1]["completeness"], "complete");
    assert_eq!(response.body["sizes"][2]["size"]["state"], "unknown");
    Ok(())
}

#[test]
fn folder_total_accepts_only_directory_references_from_the_requested_version()
-> Result<(), Box<dyn std::error::Error>> {
    let scope = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let version = Id::parse("0195f856-ab83-7a59-9343-10d29775c9aa")?;
    let store = IngestSink::default();
    let mut ingest = store.open(scope.clone()).map_err(|e| format!("{e:?}"))?;
    let dir = b"folder".to_vec();
    ingest.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    ingest
        .add_directory(vec![dir.clone()])
        .map_err(|e| format!("{e:?}"))?;
    ingest
        .add(Observation {
            path: vec![dir, b"file".to_vec()],
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::OsReported,
                amount: SizeAmount::Exact {
                    bytes: ByteSize::from(53),
                },
            }),
        })
        .map_err(|e| format!("{e:?}"))?;
    ingest
        .seal(version.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;

    let mut builder = EngineBuilder::default();
    crate::space::register_total(&mut builder, store);
    let engine = builder.build()?;
    let read = |item_version: &Id, item_no| {
        call(
            &engine,
            "space.total",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({
                "resultVersion": version,
                "target": {
                    "kind": "folder",
                    "item": {"resultVersion": item_version, "itemNo": item_no}
                },
                "requestSeq": 10
            }),
        )
    };
    assert_eq!(read(&version, 1)?.body["sizes"][0]["size"]["bytes"], "53");
    for (item_version, item_no, reason) in [
        (&version, 2, "space.target_out_of_scope"),
        (&scope, 1, "result.requery_required"),
        (&version, 3, "ref.unavailable"),
    ] {
        assert_eq!(
            read(item_version, item_no)?
                .error
                .ok_or("expected rejection")?
                .reason_key,
            reason
        );
    }
    Ok(())
}

#[test]
fn scope_total_uses_the_entire_forest_and_deduplicates_per_volume()
-> Result<(), Box<dyn std::error::Error>> {
    use du_core_contracts::{
        PathBytes, PathEncoding,
        result::{ResultNativePath, ResultScopeRoot},
    };
    let scope = Id::parse("0195f856-ab83-7a59-9343-10d29775c9a9")?;
    let version = Id::parse("0195f856-ab83-7a59-9343-10d29775c9aa")?;
    let store = IngestSink::default();
    let roots = [(b"/a", 1, 11), (b"/b", 1, 12), (b"/c", 2, 13)].map(|(path, volume, file)| {
        ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(path),
            },
            volume_id: vec![volume],
            file_id: vec![file],
        }
    });
    let mut ingest = store
        .open_forest(scope, roots.into())
        .map_err(|e| format!("{e:?}"))?;
    for (root_index, volume, bytes) in [(0, 1, 32), (1, 1, 32), (2, 2, 64)] {
        ingest
            .add_directory_at(root_index, vec![])
            .map_err(|e| format!("{e:?}"))?;
        ingest
            .add_with_space_at(
                root_index,
                Observation {
                    path: vec![b"same-name".to_vec()],
                    size: None,
                },
                SpaceObservation {
                    allocated: Some(SizeValue {
                        basis: SizeBasis::Allocated,
                        evidence: SizeEvidence::OsReported,
                        amount: SizeAmount::Exact {
                            bytes: ByteSize::from(bytes),
                        },
                    }),
                    identity: Some(FileIdentity {
                        volume: vec![volume],
                        file: vec![3],
                    }),
                    link_count: Some(if volume == 1 { 2 } else { 1 }),
                    sharing: SharingEvidence::Unknown,
                },
            )
            .map_err(|e| format!("{e:?}"))?;
    }
    ingest
        .record_gap_at(2, vec![], "scan.path_denied".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    ingest
        .seal(version.clone(), ResultCompleteness::Missing)
        .map_err(|e| format!("{e:?}"))?;
    let mut builder = EngineBuilder::default();
    crate::space::register_total(&mut builder, store);
    let engine = builder.build()?;
    let read = |target| {
        call(
            &engine,
            "space.total",
            Channel::Gui,
            ENVELOPE_VERSION,
            json!({"resultVersion": version, "target": target, "requestSeq": 11}),
        )
    };
    let total = read(json!({"kind": "scope_root"}))?;
    assert!(total.error.is_none());
    assert_eq!(total.body["sizes"][1]["size"]["bytes"], "96");
    assert_eq!(total.body["sizes"][1]["size"]["state"], "lower_bound");
    assert_eq!(total.body["inaccessibleRanges"], "1");
    let first = read(json!({"kind": "folder", "item": {"resultVersion": version, "itemNo": 0}}))?;
    assert!(first.error.is_none());
    assert_eq!(first.body["sizes"][1]["size"]["bytes"], "32");
    Ok(())
}
