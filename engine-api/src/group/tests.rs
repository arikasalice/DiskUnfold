//! F36 use cases over a published in-memory result and an in-memory rule store.
use std::{
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::{Duration, Instant},
};

use du_analysis_index::{IngestSink, Observation};
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, Id, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    group_rules::{
        GroupRulesReadError, GroupRulesSaveOutcome, GroupRulesStore, StoredGroupRuleset,
    },
    job::{HistoryAppend, JobHistory, JobSummary},
    result::ResultCompleteness,
};
use du_job_runtime::{JobClock, JobEvent, JobEvents, JobIdentity, JobRuntime};
use serde_json::{Value, json};

use super::GroupAssembly;
use crate::api::{CallerCtx, Channel, Engine, EngineBuilder};

type TestResult = Result<(), Box<dyn std::error::Error>>;

struct Ids(AtomicUsize);

impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let next = self.0.fetch_add(1, Ordering::Relaxed) as u64;
        let mut bytes = [0u8; 16];
        bytes[6] = 0x70;
        bytes[8] = 0x80;
        bytes[8..16].copy_from_slice(&(next | 0x8000_0000_0000_0000).to_be_bytes());
        Id::from_v7_bytes(bytes)
    }
}

struct Clock;

impl JobClock for Clock {
    fn now_ms(&self) -> i64 {
        1_700_000_000_000
    }
}

struct History;

impl JobHistory for History {
    fn append(&self, _: &JobSummary) -> Result<HistoryAppend, String> {
        Ok(HistoryAppend::Added)
    }
}

struct Events;

impl JobEvents for Events {
    fn publish(&self, _: JobEvent) {}
}

#[derive(Default)]
struct MemoryRules(Mutex<Option<StoredGroupRuleset>>);

impl GroupRulesStore for MemoryRules {
    fn read(&self) -> Result<Option<StoredGroupRuleset>, GroupRulesReadError> {
        self.0
            .lock()
            .map(|stored| stored.clone())
            .map_err(|_| GroupRulesReadError::Unavailable)
    }

    fn replace(&self, expected: u32, next: &StoredGroupRuleset) -> GroupRulesSaveOutcome {
        let Ok(mut stored) = self.0.lock() else {
            return GroupRulesSaveOutcome::Failed;
        };
        let current = stored.as_ref().map_or(0, |ruleset| ruleset.revision);
        if current != expected {
            return GroupRulesSaveOutcome::Conflict {
                current_revision: current,
            };
        }
        *stored = Some(next.clone());
        GroupRulesSaveOutcome::Saved
    }
}

struct Harness {
    engine: Engine,
    sink: IngestSink,
    calls: AtomicUsize,
}

fn id(byte: u8) -> Id {
    Id::from_v7_bytes([byte; 16])
}

fn logical(bytes: u64) -> SizeValue {
    SizeValue {
        basis: SizeBasis::Logical,
        evidence: SizeEvidence::OsReported,
        amount: SizeAmount::Exact {
            bytes: bytes.into(),
        },
    }
}

/// Files: a.mp4 10, b.mp4 20, c.txt 5, sub/d.mp4 7.
fn harness() -> Result<Harness, Box<dyn std::error::Error>> {
    harness_with(Arc::new(MemoryRules::default()))
}

fn harness_with(rules: Arc<MemoryRules>) -> Result<Harness, Box<dyn std::error::Error>> {
    let sink = IngestSink::default();
    publish(&sink, 2)?;
    let runtime = Arc::new(JobRuntime::new(
        Arc::new(Ids(AtomicUsize::new(1))),
        Arc::new(Clock),
        Arc::new(History),
        Arc::new(Events),
    )?);
    let workers = Arc::new(crate::workers::Workers::start(runtime.clone())?);
    let mut builder = EngineBuilder::default();
    GroupAssembly {
        store: sink.clone(),
        runtime,
        workers,
        ids: Arc::new(Ids(AtomicUsize::new(1_000))),
        clock: Arc::new(Clock),
    }
    .register(&mut builder, rules)
    .map_err(|error| format!("{error:?}"))?;
    Ok(Harness {
        engine: builder.build()?,
        sink,
        calls: AtomicUsize::new(1),
    })
}

fn publish(sink: &IngestSink, version: u8) -> Result<(), Box<dyn std::error::Error>> {
    let root = du_core_contracts::result::ResultScopeRoot {
        path: du_core_contracts::result::ResultNativePath {
            encoding: du_core_contracts::PathEncoding::Unix,
            raw: du_core_contracts::PathBytes::from_bytes(b"/data"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let mut build = sink
        .open_observed(id(1), root)
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory(vec![b"sub".to_vec()])
        .map_err(|error| format!("{error:?}"))?;
    for (path, bytes) in [
        (vec![b"a.mp4".to_vec()], 10),
        (vec![b"b.mp4".to_vec()], 20),
        (vec![b"c.txt".to_vec()], 5),
        (vec![b"sub".to_vec(), b"d.mp4".to_vec()], 7),
    ] {
        build
            .add_file(Observation {
                path,
                size: Some(logical(bytes)),
            })
            .map_err(|error| format!("{error:?}"))?;
    }
    build
        .seal(id(version), ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    Ok(())
}

impl Harness {
    fn call(
        &self,
        name: &str,
        body: Value,
    ) -> Result<(Value, Option<String>), Box<dyn std::error::Error>> {
        let next = self.calls.fetch_add(1, Ordering::Relaxed);
        let correlation = Id::parse(&format!("018f1111-0000-7000-8000-{next:012x}"))?;
        let caller = CallerCtx::new(Channel::Gui, correlation.clone(), None);
        let result = self.engine.call(
            CallEnvelope {
                name: name.into(),
                contract_version: ENVELOPE_VERSION,
                correlation_id: correlation,
                body,
            },
            &caller,
        );
        Ok((result.body, result.error.map(|error| error.reason_key)))
    }

    fn ok(&self, name: &str, body: Value) -> Result<Value, Box<dyn std::error::Error>> {
        match self.call(name, body)? {
            (body, None) => Ok(body),
            (_, Some(reason)) => Err(format!("{name} failed: {reason}").into()),
        }
    }
}

fn ends_with(value: &str) -> Value {
    json!({ "kind": "predicate", "predicate": {
        "kind": "name", "op": "ends_with", "encoding": "bytes",
        "value": du_core_contracts::PathBytes::from_bytes(value.as_bytes()) } })
}

fn at_least(bytes: u64) -> Value {
    json!({ "kind": "predicate", "predicate": {
        "kind": "size", "basis": "logical", "op": "at_least", "bytes": bytes.to_string() } })
}

fn tag() -> Value {
    json!({ "kind": "predicate", "predicate": {
        "kind": "registered", "id": "pred.tag", "revision": 1,
        "operand": du_core_contracts::PathBytes::from_bytes(b"keep") } })
}

fn save_video_and_big(harness: &Harness) -> Result<Value, Box<dyn std::error::Error>> {
    let saved = harness.ok(
        "group.rules.save",
        json!({ "expectedRevision": 0, "rules": [
            { "displayName": "Video", "condition": ends_with(".mp4") },
            { "displayName": "Big", "condition": at_least(10) },
        ] }),
    )?;
    assert_eq!(saved["outcome"], "saved");
    Ok(saved["ruleset"].clone())
}

fn classify(
    harness: &Harness,
    seq: u32,
    rules: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    harness.ok(
        "group.classify",
        json!({ "viewRef": id(7), "requestSeq": seq, "rules": rules }),
    )
}

#[test]
fn rules_round_trip_and_conflicts() -> TestResult {
    let harness = harness()?;
    let ruleset = save_video_and_big(&harness)?;
    assert_eq!(ruleset["rulesetVersion"], 1);
    assert_eq!(ruleset["rules"][1]["predicates"][0]["id"], "pred.size");
    let read = harness.ok("group.rules.get", json!({}))?;
    assert_eq!(read, ruleset);
    let (_, error) = harness.call(
        "group.rules.save",
        json!({ "expectedRevision": 0, "rules": [] }),
    )?;
    assert_eq!(error.as_deref(), Some("group.rules_modified"));
    let rejected = harness.ok(
        "group.rules.save",
        json!({ "expectedRevision": 1, "rules": [
            { "displayName": "Same", "condition": ends_with(".a") },
            { "displayName": "same", "condition": tag() },
        ] }),
    )?;
    assert_eq!(rejected["outcome"], "rejected");
    assert_eq!(rejected["reasonKey"], "group.rule_invalid");
    assert_eq!(rejected["issues"][0]["index"], 1);
    assert_eq!(rejected["issues"][0]["reasonKey"], "predicate.unknown");
    Ok(())
}

#[test]
fn classification_counts_overlap_once_and_totals_use_the_union() -> TestResult {
    let harness = harness()?;
    save_video_and_big(&harness)?;
    let response = classify(&harness, 1, json!({ "kind": "saved", "rulesetVersion": 1 }))?;
    assert_eq!(response["kind"], "summary");
    let summary = &response["summary"];
    check_summary(summary);
    let reference = summary["classificationRef"].clone();
    let version = summary["resultVersion"].clone();
    check_overlap(&harness, &reference, &version)?;
    check_totals(&harness, &reference)
}

fn check_summary(summary: &Value) {
    assert_eq!(summary["scopeFileCount"], 4);
    assert_eq!(summary["matchedCount"], 3);
    assert_eq!(summary["overlapCount"], 2);
    assert_eq!(summary["unclassifiedCount"], 1);
    assert_eq!(summary["undecidableCount"], 0);
    assert_eq!(summary["reconciled"], true);
    assert_eq!(summary["origin"], "scan");
    assert_eq!(summary["dataSource"], "real");
    assert_eq!(summary["groups"][0]["memberCount"], 3);
    assert_eq!(summary["groups"][1]["memberCount"], 2);
}

fn check_overlap(harness: &Harness, reference: &Value, version: &Value) -> TestResult {
    let overlap = harness.ok(
        "group.members",
        json!({ "classificationRef": reference, "resultVersion": version,
                "category": { "kind": "overlap" }, "start": 0, "count": 10 }),
    )?;
    assert_eq!(overlap["total"], 2);
    assert_eq!(overlap["dataSource"], "real");
    for row in overlap["rows"].as_array().ok_or("rows")? {
        assert_eq!(row["groups"].as_array().map(Vec::len), Some(2));
        assert_eq!(row["displayPath"]["state"], "present");
    }
    Ok(())
}

fn check_totals(harness: &Harness, reference: &Value) -> TestResult {
    let totals = harness.ok(
        "group.totals",
        json!({ "classificationRef": reference, "groupIds": [] }),
    )?;
    assert_eq!(totals["union"]["sizes"][0]["size"]["bytes"], "37");
    assert_eq!(totals["scope"]["sizes"][0]["size"]["bytes"], "42");
    assert_eq!(totals["overlap"]["sizes"][0]["size"]["bytes"], "30");
    assert_eq!(totals["overlapCount"], 2);
    assert_eq!(totals["ratios"][0]["state"], "value");
    assert_eq!(totals["ratios"][0]["numerator"], "37");
    assert_eq!(totals["dataSource"], "real");
    Ok(())
}

#[test]
fn a_ruleset_format_this_build_does_not_define_is_neither_read_nor_overwritten() -> TestResult {
    let rules = Arc::new(MemoryRules::default());
    let future = StoredGroupRuleset {
        ruleset_id: id(5),
        format_version: 2,
        version_no: 3,
        revision: 3,
        fingerprint: [1; 32],
        created_at_ms: 1,
        updated_at_ms: 2,
        rules: Vec::new(),
    };
    *rules.0.lock().map_err(|_| "poisoned")? = Some(future.clone());
    let harness = harness_with(rules.clone())?;
    let (_, error) = harness.call("group.rules.get", json!({}))?;
    assert_eq!(error.as_deref(), Some("db.schema_mismatch"));
    let (_, error) = harness.call(
        "group.rules.save",
        json!({ "expectedRevision": 3, "rules": [{ "displayName": "A", "condition": ends_with(".a") }] }),
    )?;
    assert_eq!(error.as_deref(), Some("db.schema_mismatch"));
    assert_eq!(*rules.0.lock().map_err(|_| "poisoned")?, Some(future));
    Ok(())
}

#[test]
fn explanation_keeps_rule_decisions_and_wrong_versions_are_refused() -> TestResult {
    let harness = harness()?;
    save_video_and_big(&harness)?;
    let response = classify(&harness, 1, json!({ "kind": "saved", "rulesetVersion": 1 }))?;
    let reference = response["summary"]["classificationRef"].clone();
    let unclassified = harness.ok(
        "group.members",
        json!({ "classificationRef": reference, "resultVersion": id(2),
                "category": { "kind": "unclassified" }, "start": 0, "count": 5 }),
    )?;
    let item = unclassified["rows"][0]["item"].clone();
    let explanation = harness.ok(
        "group.explain",
        json!({ "classificationRef": reference, "item": item }),
    )?;
    assert_eq!(explanation["rules"][0]["decision"], "no_match");
    assert_eq!(explanation["rules"][1]["decision"], "no_match");
    let (_, error) = harness.call(
        "group.members",
        json!({ "classificationRef": reference, "resultVersion": id(9),
                "category": { "kind": "overlap" }, "start": 0, "count": 5 }),
    )?;
    assert_eq!(error.as_deref(), Some("cursor.invalid"));
    let (_, error) = harness.call(
        "group.totals",
        json!({ "classificationRef": reference, "groupIds": [id(9)] }),
    )?;
    assert_eq!(error.as_deref(), Some("group.unknown"));
    Ok(())
}

#[test]
fn unregistered_predicates_make_their_rule_undecidable() -> TestResult {
    let harness = harness()?;
    let response = classify(
        &harness,
        1,
        json!({ "kind": "draft", "rules": [
            { "displayName": "Tagged", "condition": tag() },
            { "displayName": "Video", "condition": ends_with(".mp4") },
        ] }),
    )?;
    let summary = &response["summary"];
    assert_eq!(summary["rulesetVersion"], 0);
    assert_eq!(summary["groups"][0]["ruleUnevaluable"], true);
    assert_eq!(summary["groups"][0]["undecidableCount"], 4);
    assert_eq!(summary["undecidableCount"], 1);
    assert_eq!(summary["unclassifiedCount"], 0);
    assert_eq!(summary["limitations"][0]["key"], "group.limit.undecidable");
    assert_eq!(
        summary["undecidableReasons"][0]["reasonKey"],
        "group.rule_unevaluable"
    );
    Ok(())
}

#[test]
fn a_new_result_expires_classifications_and_stale_versions_are_refused() -> TestResult {
    let harness = harness()?;
    let response = classify(
        &harness,
        1,
        json!({ "kind": "draft", "rules": [{ "displayName": "Video", "condition": ends_with(".mp4") }] }),
    )?;
    let reference = response["summary"]["classificationRef"].clone();
    publish(&harness.sink, 3)?;
    let (_, error) = harness.call("group.summary", json!({ "classificationRef": reference }))?;
    assert_eq!(error.as_deref(), Some("group.result_expired"));
    let (_, error) = harness.call(
        "group.classify",
        json!({ "viewRef": id(7), "requestSeq": 2, "resultVersion": id(2),
                "rules": { "kind": "draft", "rules": [] } }),
    )?;
    assert_eq!(error.as_deref(), Some("result.version_stale"));
    let (_, error) = harness.call(
        "group.classify",
        json!({ "viewRef": id(7), "requestSeq": 3, "rules": { "kind": "saved", "rulesetVersion": 4 } }),
    )?;
    assert_eq!(error.as_deref(), Some("group.rules_modified"));
    Ok(())
}

#[test]
fn job_classification_registers_only_when_done() -> TestResult {
    let harness = harness()?;
    let response = harness.ok(
        "group.classify",
        json!({ "viewRef": super::classify::FORCE_JOB_VIEW, "requestSeq": 1,
                "rules": { "kind": "draft", "rules": [{ "displayName": "Video", "condition": ends_with(".mp4") }] } }),
    )?;
    assert_eq!(response["kind"], "job");
    let reference = response["classificationRef"].clone();
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        match harness.call("group.summary", json!({ "classificationRef": reference }))? {
            (summary, None) => {
                assert_eq!(summary["matchedCount"], 3);
                return Ok(());
            }
            (_, Some(reason)) if reason == "group.result_expired" && Instant::now() < deadline => {
                std::thread::sleep(Duration::from_millis(10));
            }
            (_, Some(reason)) => return Err(reason.into()),
        }
    }
}

#[test]
fn cancel_without_running_work_has_no_effect_and_other_channels_are_refused() -> TestResult {
    let harness = harness()?;
    let cancelled = harness.ok(
        "group.cancel",
        json!({ "kind": "view", "viewRef": id(7), "requestSeq": 1 }),
    )?;
    assert_eq!(cancelled["outcome"], "no_effect");
    let correlation = Id::parse("018f2222-0000-7000-8000-000000000001")?;
    let result = harness.engine.call(
        CallEnvelope {
            name: "group.rules.get".into(),
            contract_version: ENVELOPE_VERSION,
            correlation_id: correlation.clone(),
            body: json!({}),
        },
        &CallerCtx::new(Channel::Mcp, correlation, None),
    );
    assert_eq!(
        result.error.map(|error| error.reason_key).as_deref(),
        Some("command.not_provided")
    );
    Ok(())
}
