use du_analysis_index::{IngestSink, Observation, query};
use du_core_contracts::ports::{PortError, PortErrorKind, Random};
use du_core_contracts::{Id, ItemRef, result::ResultCompleteness};
use du_engine_api::browse::folder::cursor::{CursorError, Session};
use du_engine_api::browse::folder::{Error, FolderBrowse, OpenRequest, Sort, View};
use du_job_runtime::JobIdentity;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

struct Ids(AtomicU8);
impl JobIdentity for Ids {
    fn next_id(&self) -> Id {
        let n = self.0.fetch_add(1, Ordering::SeqCst);
        Id::from_v7_bytes([1, 2, 3, 4, 5, 6, 0x70, n, 0x80, 0, 0, 0, 0, 0, 0, n])
    }
}
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn fixture(partial: bool) -> Result<(IngestSink, FolderBrowse), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    for path in [
        vec![],
        vec![b"a".to_vec()],
        vec![b"a".to_vec(), b"empty".to_vec()],
    ] {
        build.add_directory(path).map_err(|e| format!("{e:?}"))?;
    }
    build
        .add_file(Observation {
            path: vec![b"a".to_vec(), b"file".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    build
        .add_structure(vec![b"ambiguous".to_vec()])
        .map_err(|e| format!("{e:?}"))?;
    let completeness = if partial {
        ResultCompleteness::Partial
    } else {
        ResultCompleteness::Complete
    };
    build
        .seal(id(2)?, completeness)
        .map_err(|e| format!("{e:?}"))?;
    let api = FolderBrowse::new(sink.clone(), Arc::new(Ids(AtomicU8::new(1))));
    Ok((sink, api))
}
fn request(location: Option<ItemRef>) -> OpenRequest {
    OpenRequest {
        result_version: None,
        location,
        view: View::List,
        condition: query::Condition::All(vec![]),
        sort: Sort {
            id: "sort.path".into(),
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        },
        page_size: Some(1),
    }
}

struct SequenceRandom(AtomicU8);
impl Random for SequenceRandom {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        assert_eq!(bytes.len(), 32);
        bytes.fill(self.0.fetch_add(1, Ordering::SeqCst));
        Ok(())
    }
}

struct FailingRandom;
impl Random for FailingRandom {
    fn fill(&self, bytes: &mut [u8]) -> Result<(), PortError> {
        bytes.fill(42);
        Err(PortError {
            kind: PortErrorKind::Io,
            reason_key: "os.random_unavailable".into(),
            os_code: None,
        })
    }
}

#[test]
fn positional_page_emits_a_bound_cursor_and_checks_bounds() -> Result<(), String> {
    let (_, api) = fixture(false)?;
    let session = Session::new(Arc::new(api), [7; 32]);
    let opened = session
        .open(request(None), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    let q = &opened.opened.page.query_ref;
    let version = &opened.opened.page.result_version;
    let first = session
        .page_at(q, version, 0, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.page, opened.opened.page);
    let token = first.next_cursor.ok_or("missing cursor")?;
    let next = session
        .page(&token, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    let direct = session
        .page_at(q, version, 1, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(next.page, direct.page);
    assert!(matches!(
        session.page_at(q, version, 0, Some(1001)),
        Err(CursorError::InvalidPage)
    ));
    assert!(matches!(
        session.page_at(q, version, 999, Some(1)),
        Err(CursorError::Invalid)
    ));
    assert!(session.page_at(q, &id(99)?, 0, Some(1)).is_err());
    Ok(())
}

#[test]
fn session_generation_fails_closed_and_fresh_sessions_reject_prior_cursors() -> Result<(), String> {
    let (_, api) = fixture(false)?;
    let shared = Arc::new(api);
    let error = match Session::try_new(shared.clone(), &FailingRandom) {
        Ok(_) => return Err("failed entropy admitted a session".into()),
        Err(error) => error,
    };
    assert_eq!(error.reason_key, "os.random_unavailable");
    let random = SequenceRandom(AtomicU8::new(1));
    let first = Session::try_new(shared.clone(), &random).map_err(|e| format!("{e:?}"))?;
    let second = Session::try_new(shared, &random).map_err(|e| format!("{e:?}"))?;
    let cursor = first
        .open(request(None), &[], || false)
        .map_err(|e| format!("{e:?}"))?
        .next_cursor
        .ok_or("missing cursor")?;
    assert!(first.page(&cursor, None).is_ok());
    assert!(matches!(
        second.page(&cursor, None),
        Err(CursorError::Invalid)
    ));
    Ok(())
}
#[test]
fn authenticated_session_cursor_replays_and_rejects_tamper_or_other_context() -> Result<(), String>
{
    use du_core_contracts::PathBytes;
    let (_, api) = fixture(false)?;
    let shared = Arc::new(api);
    let session = Session::new(shared.clone(), [7; 32]);
    let first = session
        .open(request(None), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    let cursor = first.next_cursor.ok_or("missing next cursor")?;
    let parts: Vec<_> = cursor.split('.').collect();
    assert_eq!(parts.len(), 3);
    assert_eq!(parts[0], "s1");
    let bytes = PathBytes::parse(parts[1])?
        .bytes()
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(bytes.len(), 56);
    assert_eq!(&bytes[48..], &1u64.to_le_bytes());
    let next = session
        .page(&cursor, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(next.page.rows[0].observation.name, b"ambiguous");
    assert!(next.next_cursor.is_none());
    assert_eq!(
        session
            .page(&cursor, Some(1))
            .map_err(|e| format!("{e:?}"))?
            .page,
        next.page
    );
    assert!(matches!(
        session.page_for(
            &cursor,
            &id(99)?,
            &first.opened.page.result_version,
            Some(1)
        ),
        Err(CursorError::Invalid)
    ));
    assert!(matches!(
        session.page(&cursor, Some(1001)),
        Err(CursorError::InvalidPage)
    ));
    let other = Session::new(shared, [8; 32]);
    assert!(matches!(
        other.page(&cursor, None),
        Err(CursorError::Invalid)
    ));
    assert!(matches!(
        session.page(&format!("{cursor}="), None),
        Err(CursorError::Invalid)
    ));
    assert!(matches!(
        session.page(&"x".repeat(2049), None),
        Err(CursorError::Invalid)
    ));
    let mut changed = bytes;
    changed[48] ^= 1;
    let altered = format!(
        "s1.{}.{}",
        PathBytes::from_bytes(&changed).as_str(),
        parts[2]
    );
    assert!(matches!(
        session.page(&altered, None),
        Err(CursorError::Invalid)
    ));
    Ok(())
}

#[test]
fn authenticated_retired_query_is_stale_but_condition_mismatch_is_invalid() -> Result<(), String> {
    use du_core_contracts::PathBytes;
    let (sink, api) = fixture(false)?;
    let session = Session::new(Arc::new(api), [9; 32]);
    let first = session
        .open(request(None), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    let cursor = first.next_cursor.ok_or("missing next cursor")?;
    let parts: Vec<_> = cursor.split('.').collect();
    let mut payload = PathBytes::parse(parts[1])?
        .bytes()
        .map_err(|e| format!("{e:?}"))?;
    payload[32] ^= 1;
    let mut mac = blake3::Hasher::new_keyed(&[9; 32]);
    mac.update(b"diskunfold.cursor.s1\0");
    mac.update(&payload);
    let mismatch = format!(
        "s1.{}.{}",
        PathBytes::from_bytes(&payload).as_str(),
        PathBytes::from_bytes(mac.finalize().as_bytes()).as_str()
    );
    assert!(matches!(
        session.page(&mismatch, None),
        Err(CursorError::Invalid)
    ));
    payload[32] ^= 1;
    payload[48..56].copy_from_slice(&99u64.to_le_bytes());
    let mut mac = blake3::Hasher::new_keyed(&[9; 32]);
    mac.update(b"diskunfold.cursor.s1\0");
    mac.update(&payload);
    let bad_position = format!(
        "s1.{}.{}",
        PathBytes::from_bytes(&payload).as_str(),
        PathBytes::from_bytes(mac.finalize().as_bytes()).as_str()
    );
    assert!(matches!(
        session.page(&bad_position, None),
        Err(CursorError::Invalid)
    ));
    let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(matches!(
        session.page(&cursor, None),
        Err(CursorError::Stale)
    ));
    Ok(())
}

#[test]
fn folder_rejects_unbounded_condition_before_normalization() -> Result<(), String> {
    let (_, api) = fixture(false)?;
    let mut condition = query::Condition::All(vec![]);
    for _ in 0..65 {
        condition = query::Condition::Not(Box::new(condition));
    }
    let mut request = request(None);
    request.condition = condition;
    assert_eq!(api.open(request, || false), Err(Error::InvalidCondition));
    Ok(())
}

#[test]
fn authenticated_cursor_is_stale_when_its_query_is_evicted() -> Result<(), String> {
    let (_, api) = fixture(false)?;
    let session = Session::new(Arc::new(api), [11; 32]);
    let first = session
        .open(request(None), &[], || false)
        .map_err(|e| format!("{e:?}"))?;
    let cursor = first.next_cursor.ok_or("missing next cursor")?;
    for n in 0..16u8 {
        let mut req = request(None);
        req.condition = query::Condition::Predicate(query::Predicate::Name(
            query::NameTest::Equal(vec![n]),
            query::NameEncoding::Bytes,
        ));
        session
            .open(req, &[], || false)
            .map_err(|e| format!("{e:?}"))?;
    }
    assert!(matches!(
        session.page(&cursor, None),
        Err(CursorError::Stale)
    ));
    Ok(())
}
