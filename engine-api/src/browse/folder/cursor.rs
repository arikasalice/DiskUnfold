//! Authenticated GUI-session cursor handling over admitted folder query results.
//!
//! The session composition supplies the key. This module has no random fallback
//! or IPC registration.
mod condition;
pub use condition::CanonicalCondition;
use std::sync::Arc;

use du_analysis_index::query::page::{Page, PageError};
use du_core_contracts::{
    Id, PathBytes,
    ports::{PortError, Random},
};

use super::{
    AdmittedOpen, Error, FolderBrowse, FolderPage, OpenRequest, Opened, Sort, project,
    validate_count,
};

const PAYLOAD_LEN: usize = 56;
const MAC_LEN: usize = 32;
const MAX_CURSOR_LEN: usize = 2048;
const DOMAIN: &[u8] = b"diskunfold.cursor.s1\0";

/// Cursor admission failures, before any transport-specific error mapping.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CursorError {
    Invalid,
    Stale,
    InvalidPage,
    FingerprintUnavailable,
    Folder(Error),
}

/// An existing folder-open result and its authenticated next cursor, if any.
pub struct OpenedPage {
    pub opened: Opened,
    pub next_cursor: Option<String>,
}

/// A page and its authenticated next cursor, if any.
pub struct CursorPage {
    pub page: FolderPage,
    pub next_cursor: Option<String>,
}

/// One immutable session key and result registry. Retiring this value retires its key.
pub struct Session {
    browse: Arc<FolderBrowse>,
    key: [u8; 32],
}

impl Session {
    /// Creates a session only after obtaining a fresh 32-byte key from the random port.
    /// Production composition must inject cryptographically secure OS randomness.
    pub fn try_new(browse: Arc<FolderBrowse>, random: &dyn Random) -> Result<Self, PortError> {
        let mut key = [0; 32];
        random.fill(&mut key)?;
        Ok(Self::new(browse, key))
    }

    /// Constructs a GUI-session reader. The caller must supply a CSPRNG-generated key.
    pub fn new(browse: Arc<FolderBrowse>, key: [u8; 32]) -> Self {
        Self { browse, key }
    }

    /// Opens a folder and issues a cursor bound to the admitted query and its condition.
    pub fn open(
        &self,
        request: OpenRequest,
        additional: &[Sort],
        cancelled: impl FnMut() -> bool,
    ) -> Result<OpenedPage, CursorError> {
        let admitted = self
            .browse
            .open_admitted(request, additional, cancelled)
            .map_err(CursorError::Folder)?;
        self.finish_open(admitted)
    }

    fn finish_open(&self, admitted: AdmittedOpen) -> Result<OpenedPage, CursorError> {
        let condition = admitted.page.metadata().condition.clone();
        let opened = admitted.project().map_err(CursorError::Folder)?;
        let next_cursor = match opened.page.next_position {
            Some(next) => Some(self.encode(
                &opened.page.query_ref,
                &opened.page.result_version,
                CanonicalCondition.fingerprint(&condition)?,
                next,
            )?),
            None => None,
        };
        Ok(OpenedPage {
            opened,
            next_cursor,
        })
    }

    /// Authenticates a cursor and admits its next page from the same query and version.
    pub fn page(&self, cursor: &str, count: Option<u32>) -> Result<CursorPage, CursorError> {
        validate_count(count).map_err(|_| CursorError::InvalidPage)?;
        let fields = self.decode(cursor)?;
        self.read_page(fields, count)
    }

    /// Reads an explicit position and binds the next cursor to the registered query.
    pub fn page_at(
        &self,
        query: &Id,
        version: &Id,
        start: u64,
        count: Option<u32>,
    ) -> Result<CursorPage, CursorError> {
        validate_count(count).map_err(|_| CursorError::InvalidPage)?;
        let admitted = self.admit(query, version, start, count)?;
        let condition = self.condition(&admitted)?;
        self.finish_page(admitted, query, version, condition)
    }

    /// Reads a cursor while requiring the caller's expected query and result version.
    /// A valid cursor for another context is invalid, even if that query still exists.
    pub fn page_for(
        &self,
        cursor: &str,
        query: &Id,
        version: &Id,
        count: Option<u32>,
    ) -> Result<CursorPage, CursorError> {
        validate_count(count).map_err(|_| CursorError::InvalidPage)?;
        let fields = self.decode(cursor)?;
        if &fields.query != query || &fields.version != version {
            return Err(CursorError::Invalid);
        }
        self.read_page(fields, count)
    }

    fn read_page(&self, fields: Fields, count: Option<u32>) -> Result<CursorPage, CursorError> {
        let page = self.admit(&fields.query, &fields.version, fields.next, count)?;
        if self.condition(&page)? != fields.condition {
            return Err(CursorError::Invalid);
        }
        self.finish_page(page, &fields.query, &fields.version, fields.condition)
    }

    fn finish_page(
        &self,
        page: Page,
        query: &Id,
        version: &Id,
        condition: [u8; 16],
    ) -> Result<CursorPage, CursorError> {
        let page = project(query.clone(), page).map_err(CursorError::Folder)?;
        let next_cursor = page
            .next_position
            .map(|next| self.encode(query, version, condition, next))
            .transpose()?;
        Ok(CursorPage { page, next_cursor })
    }

    fn admit(
        &self,
        query: &Id,
        version: &Id,
        start: u64,
        count: Option<u32>,
    ) -> Result<Page, CursorError> {
        self.browse
            .sets
            .open_page(query, version, start, count)
            .map_err(|error| match error {
                PageError::StaleVersion => CursorError::Stale,
                PageError::InvalidRange => CursorError::Invalid,
                _ => CursorError::Folder(Error::Internal),
            })
    }

    fn condition(&self, page: &Page) -> Result<[u8; 16], CursorError> {
        CanonicalCondition.fingerprint(page.metadata().condition)
    }

    fn encode(
        &self,
        query: &Id,
        version: &Id,
        condition: [u8; 16],
        next: u64,
    ) -> Result<String, CursorError> {
        let query = uuid::Uuid::parse_str(query.as_str()).map_err(|_| CursorError::Invalid)?;
        let version = uuid::Uuid::parse_str(version.as_str()).map_err(|_| CursorError::Invalid)?;
        let mut payload = [0u8; PAYLOAD_LEN];
        payload[..16].copy_from_slice(query.as_bytes());
        payload[16..32].copy_from_slice(version.as_bytes());
        payload[32..48].copy_from_slice(&condition);
        payload[48..].copy_from_slice(&next.to_le_bytes());
        let mac = self.mac(&payload);
        Ok(format!(
            "s1.{}.{}",
            PathBytes::from_bytes(&payload).as_str(),
            PathBytes::from_bytes(mac.as_bytes()).as_str()
        ))
    }

    fn decode(&self, cursor: &str) -> Result<Fields, CursorError> {
        if cursor.len() > MAX_CURSOR_LEN {
            return Err(CursorError::Invalid);
        }
        let mut parts = cursor.split('.');
        if parts.next() != Some("s1") {
            return Err(CursorError::Invalid);
        }
        let payload = decode_part(parts.next())?;
        let received = decode_part(parts.next())?;
        if parts.next().is_some() || payload.len() != PAYLOAD_LEN || received.len() != MAC_LEN {
            return Err(CursorError::Invalid);
        }
        let received: [u8; MAC_LEN] = received.try_into().map_err(|_| CursorError::Invalid)?;
        // BLAKE3 Hash equality uses its constant-time 32-byte comparison.
        if self.mac(&payload) != received {
            return Err(CursorError::Invalid);
        }
        let query = id_from(&payload[..16])?;
        let version = id_from(&payload[16..32])?;
        let condition = payload[32..48]
            .try_into()
            .map_err(|_| CursorError::Invalid)?;
        let next = u64::from_le_bytes(
            payload[48..56]
                .try_into()
                .map_err(|_| CursorError::Invalid)?,
        );
        Ok(Fields {
            query,
            version,
            condition,
            next,
        })
    }

    fn mac(&self, payload: &[u8]) -> blake3::Hash {
        let mut hash = blake3::Hasher::new_keyed(&self.key);
        hash.update(DOMAIN);
        hash.update(payload);
        hash.finalize()
    }
}

struct Fields {
    query: Id,
    version: Id,
    condition: [u8; 16],
    next: u64,
}

fn decode_part(part: Option<&str>) -> Result<Vec<u8>, CursorError> {
    PathBytes::parse(part.ok_or(CursorError::Invalid)?)
        .map_err(|_| CursorError::Invalid)?
        .bytes()
        .map_err(|_| CursorError::Invalid)
}

fn id_from(bytes: &[u8]) -> Result<Id, CursorError> {
    let bytes: [u8; 16] = bytes.try_into().map_err(|_| CursorError::Invalid)?;
    Id::parse(&uuid::Uuid::from_bytes(bytes).to_string()).map_err(|_| CursorError::Invalid)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::browse::folder::View;
    use du_analysis_index::{IngestSink, query};
    use du_core_contracts::result::ResultCompleteness;
    use du_job_runtime::JobIdentity;

    struct Identity;
    impl JobIdentity for Identity {
        fn next_id(&self) -> Id {
            Id::from_v7_bytes([1, 2, 3, 4, 5, 6, 0x70, 1, 0x80, 0, 0, 0, 0, 0, 0, 1])
        }
    }

    #[test]
    fn admitted_open_survives_replacement_and_binds_tree_condition() -> Result<(), String> {
        let id = |n: u8| Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"));
        let sink = IngestSink::default();
        let mut build = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
        for path in [vec![], vec![b"a".to_vec()], vec![b"b".to_vec()]] {
            build.add_directory(path).map_err(|e| format!("{e:?}"))?;
        }
        build
            .seal(id(2)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let browse = Arc::new(FolderBrowse::new(sink.clone(), Arc::new(Identity)));
        let session = Session::new(browse.clone(), [3; 32]);
        let input = query::Condition::All(vec![query::Condition::All(vec![])]);
        let request = OpenRequest {
            result_version: None,
            location: None,
            view: View::Tree,
            condition: input.clone(),
            sort: Sort {
                id: "sort.path".into(),
                revision: 1,
                basis: query::Basis::Logical,
                descending: false,
            },
            page_size: Some(1),
        };
        let admitted = browse
            .open_admitted(request, &[], || false)
            .map_err(|e| format!("{e:?}"))?;
        let expected = query::Condition::All(vec![
            input,
            query::Condition::Predicate(query::Predicate::Kind(query::Kind::Directory)),
        ])
        .normalize();
        let mut replacement = sink.open(id(1)?).map_err(|e| format!("{e:?}"))?;
        replacement
            .add_directory(vec![])
            .map_err(|e| format!("{e:?}"))?;
        replacement
            .seal(id(3)?, ResultCompleteness::Complete)
            .map_err(|e| format!("{e:?}"))?;
        let opened = session
            .finish_open(admitted)
            .map_err(|e| format!("{e:?}"))?;
        let cursor = opened.next_cursor.ok_or("missing cursor")?;
        let fields = session.decode(&cursor).map_err(|e| format!("{e:?}"))?;
        assert_eq!(
            fields.condition,
            CanonicalCondition
                .fingerprint(&expected)
                .map_err(|e| format!("{e:?}"))?
        );
        assert!(matches!(
            session.page(&cursor, None),
            Err(CursorError::Stale)
        ));
        Ok(())
    }
}
