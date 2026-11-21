use super::*;
use du_core_contracts::{
    CallEnvelope, ENVELOPE_VERSION, ErrorCode, Id,
    snapshot_index::{
        SnapshotIndexCompleteness, SnapshotIndexCursor, SnapshotIndexPage, SnapshotIndexPort,
        SnapshotIndexRegisterResult, SnapshotIndexRow, SnapshotIndexSourceKind, SnapshotIndexState,
        SnapshotIndexVolume, SnapshotVolumeIdentityCapability, SnapshotVolumeIdentityKind,
    },
};
use serde_json::{Value, json};
use std::sync::MutexGuard;

#[derive(Clone)]
struct MemoryIndex(Arc<Mutex<IndexState>>);

struct IndexState {
    page: SnapshotIndexPage,
    error: Option<SnapshotIndexError>,
    last_limit: Option<usize>,
    last_cursor: Option<SnapshotIndexCursor>,
    calls: usize,
}

impl Default for MemoryIndex {
    fn default() -> Self {
        Self(Arc::new(Mutex::new(IndexState {
            page: SnapshotIndexPage {
                rows: Vec::new(),
                list_seq: 1,
                next_cursor: None,
            },
            error: None,
            last_limit: None,
            last_cursor: None,
            calls: 0,
        })))
    }
}

impl MemoryIndex {
    fn state(&self) -> MutexGuard<'_, IndexState> {
        self.0.lock().unwrap_or_else(|error| error.into_inner())
    }
}

impl SnapshotIndexPort for MemoryIndex {
    fn register(
        &mut self,
        _: &SnapshotIndexRow,
        _: &[SnapshotIndexVolume],
        _: Option<i64>,
    ) -> Result<SnapshotIndexRegisterResult, SnapshotIndexError> {
        Err(SnapshotIndexError::Storage)
    }

    fn find_active_by_source(&mut self, _: &Id) -> Result<Option<Id>, SnapshotIndexError> {
        Err(SnapshotIndexError::Storage)
    }

    fn list_active(
        &mut self,
        limit: usize,
        after: Option<&SnapshotIndexCursor>,
    ) -> Result<SnapshotIndexPage, SnapshotIndexError> {
        let mut state = self.state();
        state.calls += 1;
        state.last_limit = Some(limit);
        state.last_cursor = after.cloned();
        match state.error {
            Some(error) => Err(error),
            None => Ok(state.page.clone()),
        }
    }
}

fn id(number: u32) -> Result<Id, Box<dyn std::error::Error>> {
    Ok(Id::parse(&format!(
        "018f0000-0000-7000-8000-{number:012x}"
    ))?)
}

fn fixed_id(number: u32) -> Id {
    let mut bytes = [0; 16];
    bytes[6] = 0x70;
    bytes[8] = 0x80;
    bytes[12..].copy_from_slice(&number.to_be_bytes());
    Id::from_v7_bytes(bytes)
}

fn row(number: u32, observed_at_ms: Option<i64>, scope_label: Vec<u8>) -> SnapshotIndexRow {
    SnapshotIndexRow {
        snapshot_id: fixed_id(number),
        file_name: format!("{}.dusnap", fixed_id(number).as_str()),
        source_version_id: fixed_id(number + 100),
        lineage_id: fixed_id(number + 200),
        source_kind: SnapshotIndexSourceKind::Local,
        observed_at_ms,
        created_at_ms: 1,
        scope_label,
        condition_summary: r#"{"private":"condition-secret"}"#.into(),
        completeness: Some(SnapshotIndexCompleteness::Complete),
        format_version: 2,
        body_size: 64,
        body_hash: [0; 32],
        state: SnapshotIndexState::Active,
        verify_failed_at_ms: None,
        updated_at_ms: 1,
        row_version: 1,
    }
}

fn volume(index: u16, observed_volume_id: Option<Vec<u8>>) -> SnapshotIndexVolume {
    SnapshotIndexVolume {
        volume_index: index,
        observed_volume_id,
        identity_kind: SnapshotVolumeIdentityKind::None,
        identity_capability: SnapshotVolumeIdentityCapability::Unknown,
    }
}

fn call(
    index: &MemoryIndex,
    primary_window: bool,
    body: Value,
) -> Result<Value, Box<dyn std::error::Error>> {
    let mut builder = EngineBuilder::default();
    builder.register(ListCommand {
        index: Arc::new(Mutex::new(index.clone())),
        primary_window,
    });
    let engine = builder.build()?;
    let correlation_id = id(999)?;
    let envelope: CallEnvelope = serde_json::from_value(json!({
        "name": "snapshot.list",
        "contractVersion": ENVELOPE_VERSION,
        "correlationId": correlation_id,
        "body": body,
        "unknownEnvelopeField": true
    }))?;
    Ok(serde_json::to_value(engine.call(
        envelope,
        &CallerCtx::new(Channel::Gui, correlation_id, None),
    ))?)
}

#[test]
fn defaults_to_two_hundred_and_accepts_unknown_request_fields()
-> Result<(), Box<dyn std::error::Error>> {
    let index = MemoryIndex::default();
    let response = call(&index, true, json!({"future": true}))?;
    assert!(response["error"].is_null());
    assert_eq!(index.state().last_limit, Some(200));
    Ok(())
}

#[test]
fn clamps_large_limits_and_rejects_zero_or_negative_limits()
-> Result<(), Box<dyn std::error::Error>> {
    let index = MemoryIndex::default();
    assert!(call(&index, true, json!({"limit": 1_001}))?["error"].is_null());
    assert_eq!(index.state().last_limit, Some(1_000));
    for limit in [0, -1] {
        let response = call(&index, true, json!({"limit": limit}))?;
        assert_eq!(response["error"]["code"], "INVALID_ARGUMENT");
        assert_eq!(
            response["error"]["reasonKey"],
            "snapshot.list_limit_invalid"
        );
    }
    assert_eq!(index.state().calls, 1);
    Ok(())
}

#[test]
fn cursor_pages_are_opaque_and_can_use_a_different_limit() -> Result<(), Box<dyn std::error::Error>>
{
    let index = MemoryIndex::default();
    let expected = SnapshotIndexCursor {
        list_seq: 9,
        observed_at_ms: Some(123),
        snapshot_id: id(4)?,
    };
    index.state().page.next_cursor = Some(expected.clone());
    let first = call(&index, true, json!({}))?;
    let token = first["body"]["nextCursor"]
        .as_str()
        .ok_or("missing cursor")?;
    assert!(!token.contains("snapshotId"));
    assert_eq!(decode_cursor(token), Ok(expected.clone()));

    index.state().page.next_cursor = None;
    let second = call(&index, true, json!({"cursor": token, "limit": 1}))?;
    assert!(second["error"].is_null());
    let state = index.state();
    assert_eq!(state.last_limit, Some(1));
    assert_eq!(state.last_cursor.as_ref(), Some(&expected));
    Ok(())
}

#[test]
fn malformed_and_stale_cursors_have_their_published_error_codes()
-> Result<(), Box<dyn std::error::Error>> {
    let malformed = MemoryIndex::default();
    let response = call(&malformed, true, json!({"cursor": "!"}))?;
    assert_eq!(response["error"]["code"], "INVALID_ARGUMENT");
    assert_eq!(response["error"]["reasonKey"], "snapshot.cursor_invalid");
    assert_eq!(malformed.state().calls, 0);

    let stale = MemoryIndex::default();
    stale.state().error = Some(SnapshotIndexError::StaleCursor);
    let token = encode_cursor(&SnapshotIndexCursor {
        list_seq: 1,
        observed_at_ms: None,
        snapshot_id: id(1)?,
    })
    .map_err(|_| "cursor encode failed")?;
    let response = call(&stale, true, json!({"cursor": token}))?;
    assert_eq!(response["error"]["code"], "STALE_VERSION");
    assert_eq!(response["error"]["reasonKey"], "snapshot.cursor_stale");
    Ok(())
}

#[test]
fn summary_omits_empty_scope_and_hex_encodes_private_volume_ids()
-> Result<(), Box<dyn std::error::Error>> {
    let index = MemoryIndex::default();
    index.state().page.rows = vec![
        SnapshotIndexEntry {
            row: row(1, Some(1_600_000_000_000), Vec::new()),
            volumes: vec![volume(3, Some(vec![0x01, 0xab])), volume(4, None)],
            pinned: true,
        },
        SnapshotIndexEntry {
            row: row(2, None, vec![0xab; 32]),
            volumes: Vec::new(),
            pinned: false,
        },
    ];
    let response = call(&index, true, json!({}))?;
    let item = &response["body"]["items"][0];
    assert!(item.get("scopeLabel").is_none());
    assert_eq!(item["volumes"][0]["observedVolumeId"], "01ab");
    assert!(item["volumes"][1].get("observedVolumeId").is_none());
    assert_eq!(
        item["conditionSummary"],
        r#"{"private":"condition-secret"}"#
    );
    assert_eq!(item["sourceKind"], "local");
    assert_eq!(item["completeness"], "complete");
    assert_eq!(item["pinned"], true);
    assert_eq!(response["body"]["items"][1]["scopeLabel"], "ab".repeat(32));
    Ok(())
}

#[test]
fn non_main_window_is_denied_before_reading_the_index() -> Result<(), Box<dyn std::error::Error>> {
    let index = MemoryIndex::default();
    let response = call(&index, false, json!({}))?;
    assert_eq!(response["error"]["code"], "ORIGIN_DENIED");
    assert_eq!(response["error"]["reasonKey"], "command.window_denied");
    assert_eq!(index.state().calls, 0);
    Ok(())
}

#[test]
fn malformed_scope_markers_are_not_returned_as_valid_summaries()
-> Result<(), Box<dyn std::error::Error>> {
    let index = MemoryIndex::default();
    index.state().page.rows = vec![SnapshotIndexEntry {
        row: row(1, None, vec![0x01]),
        volumes: Vec::new(),
        pinned: false,
    }];
    let response = call(&index, true, json!({}))?;
    assert_eq!(response["error"]["code"], "INTERNAL");
    Ok(())
}

#[test]
fn wire_error_reasons_are_registered_with_typed_codes() -> Result<(), Box<dyn std::error::Error>> {
    let correlation = id(500)?;
    for (reason, code) in [
        ("snapshot.list_limit_invalid", ErrorCode::InvalidArgument),
        ("snapshot.cursor_invalid", ErrorCode::InvalidArgument),
        ("snapshot.cursor_stale", ErrorCode::StaleVersion),
    ] {
        let error =
            du_core_contracts::ErrorEnvelope::from_reason(reason, correlation.clone(), Vec::new())
                .ok_or("unregistered snapshot list reason")?;
        assert_eq!(error.code, code);
    }
    Ok(())
}
