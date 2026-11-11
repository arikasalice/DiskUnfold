//! Internal F09 assembly. This module does not register a wire use case.
use crate::duplicate_scope::Selection;
use du_core_contracts::{
    Id, ItemRef,
    duplicate_content::{
        ContentIdentity, ContentObservation, DuplicateContentPort, PlaceholderEvidence,
    },
    scan_observation::NativeModifiedTime,
};
use du_duplicate_engine::{
    RegisteredHash,
    candidates::{EntryKind, FileIdentity, Placeholder, Size, Target},
    content_port::{Adapter, SelectedContent},
    job::{self, Input},
    result_store::{ResultStore, ScopeEvidence},
    verification::{OpenFile, ReadError, ReadPort, Snapshot, VerificationTarget},
};
use du_job_runtime::{
    DeduplicationKey, JobPriority, JobRuntime, RuntimeError, Submission, SubmitResult,
};
use std::sync::Arc;

/// Metadata-only evidence from a trusted platform composition. Implementations must
/// not fetch placeholder data or read content while producing this observation.
pub trait Preflight {
    fn observe(
        &mut self,
        item: &ItemRef,
        expected: &ContentIdentity,
    ) -> Option<(PlaceholderEvidence, ContentObservation)>;
}

/// Admission data supplied by the process owning the result and job identities.
pub struct Request {
    pub result: Id,
    pub correlation: Id,
    pub priority: JobPriority,
}

#[derive(Debug)]
pub enum Error {
    Hash,
    InvalidSelection,
    Prepare(job::PrepareError),
    Runtime(RuntimeError),
    Key,
}

/// Registers the published descriptor on the process-owned runtime.
/// # Errors
/// Returns the runtime registration failure, including duplicate registration.
pub fn register(runtime: &JobRuntime) -> Result<(), RuntimeError> {
    runtime.register(job::descriptor())
}

/// Prepares and submits a selected scope; no content is read before submission.
/// The source must be registered with `store` and retired by its owning session.
/// The platform provider and port must be independently trusted by composition.
/// # Errors
/// Returns a preparation or runtime error without publishing a result.
pub fn submit<P: DuplicateContentPort + Send + 'static>(
    runtime: &JobRuntime,
    store: Arc<ResultStore>,
    selection: Selection,
    mut platform: (P, impl Preflight),
    request: Request,
) -> Result<SubmitResult, Error> {
    let hash = RegisteredHash::resolve("blake3", Some(1)).map_err(|_| Error::Hash)?;
    let scope = selection.scope_evidence();
    let key = dedup_key(&selection)?;
    let scope = ScopeEvidence {
        roots: scope.roots.clone(),
        exclusions: scope.exclusions.clone(),
        minimum_bytes: scope.minimum_bytes,
        outside: scope.counts.outside,
        excluded: scope.counts.excluded,
        below_minimum: scope.counts.below_minimum,
        unavailable: scope.counts.unavailable,
        source_completeness: scope.source_completeness,
        observed_at: scope.observed_at.clone(),
    };
    let mut targets = Vec::with_capacity(selection.files().len());
    let mut selected = Vec::with_capacity(selection.files().len());
    for file in selection.files() {
        let evidence = selection
            .content_evidence(&file.item)
            .ok_or(Error::InvalidSelection)?;
        let identity = FileIdentity {
            volume: evidence.identity.volume.clone(),
            file: evidence.identity.file.clone(),
        };
        let native = evidence.native_modified.map(|value| match value {
            NativeModifiedTime::WindowsFiletime(t) => t.get(),
        });
        let current = if native.is_some() && evidence.identity.enumerated_inode.is_some() {
            platform.1.observe(&file.item, &evidence.identity)
        } else {
            None
        };
        let (placeholder, snapshot) = match current {
            Some((locality, observation))
                if observation.identity == evidence.identity
                    && observation.size == file.size
                    && Some(observation.modified) == native =>
            {
                let snapshot = Snapshot {
                    identity: identity.clone(),
                    size: file.size,
                    modified: observation.modified,
                    change_stamp: observation.change_stamp,
                };
                (locality, Some(snapshot))
            }
            _ => (PlaceholderEvidence::Unknown, None),
        };
        let status = match placeholder {
            PlaceholderEvidence::Local => Placeholder::Local,
            PlaceholderEvidence::DownloadRequired => Placeholder::DownloadRequired,
            PlaceholderEvidence::Unknown => Placeholder::Unknown,
        };
        targets.push(VerificationTarget {
            target: Target {
                item_no: file.item.item_no,
                kind: EntryKind::File,
                size: Size::Exact(file.size),
                identity: Some(identity),
                placeholder: status,
            },
            snapshot,
        });
        selected.push(SelectedContent {
            item: file.item.clone(),
            identity: evidence.identity,
            placeholder,
        });
    }
    let source = selection.version().clone();
    let adapter =
        Adapter::new_owned(platform.0, &source, selected).map_err(|_| Error::InvalidSelection)?;
    let reader = PinnedReader {
        adapter,
        _selection: selection,
    };
    let prepared = job::prepare(
        Input {
            source,
            result: request.result,
            scope,
            targets,
            reader,
        },
        hash,
        store,
    )
    .map_err(Error::Prepare)?;
    let submission = Submission {
        kind_id: "duplicate.check".into(),
        deduplication_key: Some(key),
        body: prepared.body,
        priority: request.priority,
        scope_summary: "duplicate.check".into(),
        correlation_id: request.correlation,
        sample: false,
    };
    runtime
        .submit_duplicate_terminal(submission, prepared.queued, prepared.commit)
        .map_err(Error::Runtime)
}

fn dedup_key(selection: &Selection) -> Result<DeduplicationKey, Error> {
    let scope = selection.scope_evidence();
    let mut roots: Vec<_> = scope.roots.iter().map(|x| x.item_no).collect();
    let mut exclusions: Vec<_> = scope.exclusions.iter().map(|x| x.item_no).collect();
    roots.sort_unstable();
    roots.dedup();
    exclusions.sort_unstable();
    exclusions.dedup();
    let bytes = serde_json::to_vec(&(selection.version(), roots, exclusions, scope.minimum_bytes))
        .map_err(|_| Error::Key)?;
    Ok(DeduplicationKey {
        scope: selection.version().as_str().into(),
        input: blake3::hash(&bytes).to_hex().to_string(),
        channel: "duplicate.check".into(),
        key: "blake3:1".into(),
    })
}

struct PinnedReader<P: 'static> {
    adapter: Adapter<'static, P>,
    _selection: Selection,
}
impl<P: DuplicateContentPort + 'static> ReadPort for PinnedReader<P> {
    fn open_bound<'a>(
        &'a mut self,
        item_no: u32,
        expected: &FileIdentity,
    ) -> Result<Box<dyn OpenFile + 'a>, ReadError> {
        self.adapter.open_bound(item_no, expected)
    }
}
