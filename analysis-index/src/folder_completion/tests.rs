//! Recursive folder promotion from attempt evidence pinned in the same version.
use crate::{
    BuildHandle, FolderAttemptFinal, FolderAttemptInput, FolderAttemptToken, IngestSink,
    Observation, ResultVersion,
};
use du_core_contracts::{
    Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    ports::{PortError, PortErrorKind},
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, SpaceAggregate},
    scan_observation::{
        CorrelatedDirectoryEvent, FilterCallbackPhase, FilterDecision, MetadataObservation,
        ObservedEntry, ObservedKind, TimedObservedEntry,
    },
};
type Test = Result<(), String>;

fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}
fn id(value: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{value:012x}")).map_err(e)
}
fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    }
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}
fn exact(bytes: u64) -> SizeAmount {
    SizeAmount::Exact {
        bytes: bytes.into(),
    }
}
fn observation(name: &str, kind: ObservedKind) -> TimedObservedEntry {
    TimedObservedEntry {
        entry: ObservedEntry {
            name: name.as_bytes().to_vec(),
            enumerated_inode: 1,
            enumerated_kind: kind.clone(),
            metadata: Ok(MetadataObservation {
                kind,
                device: 1,
                inode: 1,
                size: Err(PortError {
                    kind: PortErrorKind::Unsupported,
                    reason_key: "scan.test_size_unavailable".into(),
                    os_code: None,
                }),
                modified_at: None,
                native_modified_at: None,
                allocated: None,
                link_count: None,
                space_identity: None,
            }),
        },
        observed_at: None,
        placeholder_observation: None,
    }
}

/// Sends the callback, observation and terminal of each child, then ends the attempt.
struct Attempt {
    token: FolderAttemptToken,
    seq: u64,
    children: u64,
}

impl Attempt {
    fn begin(build: &mut BuildHandle, folder: &[&str], attempt_id: u64) -> Result<Self, String> {
        Ok(Self {
            token: build
                .begin_folder_attempt(0, &path(folder), attempt_id)
                .map_err(e)?,
            seq: 0,
            children: 0,
        })
    }
    fn send(&mut self, build: &mut BuildHandle, input: FolderAttemptInput) -> Test {
        build
            .record_folder_attempt_event(self.token, self.seq, input)
            .map_err(e)?;
        self.seq += 1;
        Ok(())
    }
    /// Admits and observes one child and returns its child ID.
    fn observe(
        &mut self,
        build: &mut BuildHandle,
        name: &str,
        kind: ObservedKind,
    ) -> Result<u64, String> {
        let child_id = self.children;
        self.children += 1;
        self.send(
            build,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::FilterCallback {
                child_id,
                raw_name: name.as_bytes().to_vec(),
                decision: FilterDecision::Admit,
                phase: FilterCallbackPhase::BeforeMetadata,
            }),
        )?;
        self.send(
            build,
            FolderAttemptInput::Adapter(CorrelatedDirectoryEvent::Observation {
                child_id,
                entry: observation(name, kind),
            }),
        )?;
        Ok(child_id)
    }
    fn recorded(
        &mut self,
        build: &mut BuildHandle,
        child_id: u64,
        child_attempt: Option<FolderAttemptToken>,
    ) -> Test {
        self.send(
            build,
            FolderAttemptInput::Final {
                child_id,
                outcome: FolderAttemptFinal::Recorded { child_attempt },
            },
        )
    }
    fn file(&mut self, build: &mut BuildHandle, name: &str) -> Test {
        let child = self.observe(build, name, ObservedKind::File)?;
        self.recorded(build, child, None)
    }
    fn end(self, build: &mut BuildHandle) -> Test {
        build
            .end_folder_attempt(self.token, self.seq, self.children)
            .map_err(e)
    }
}

/// How the subfolder `sub` and the parent attempt finish.
#[derive(Clone, Copy, PartialEq)]
enum Case {
    AllEnded,
    SubPending,
    SubCancelled,
    ParentSkipsSubTerminal,
}

/// Builds `/` with `a` (10 bytes) and `sub/b` (5 bytes) plus root and `sub` attempts.
fn tree(sink: &IngestSink, case: Case) -> Result<BuildHandle, String> {
    let mut build = sink.open_forest(id(1)?, vec![root()]).map_err(e)?;
    build.add_directory(Vec::new()).map_err(e)?;
    for (parts, bytes) in [(&["a"][..], 10), (&["sub", "b"][..], 5)] {
        if parts.len() == 2 {
            build.add_directory(path(&parts[..1])).map_err(e)?;
        }
        build
            .add_file(Observation {
                path: path(parts),
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: exact(bytes),
                }),
            })
            .map_err(e)?;
    }
    let mut parent = Attempt::begin(&mut build, &[], 10)?;
    parent.file(&mut build, "a")?;
    let sub_child = parent.observe(&mut build, "sub", ObservedKind::Directory)?;
    let mut sub = Attempt::begin(&mut build, &["sub"], 11)?;
    let sub_token = sub.token;
    sub.file(&mut build, "b")?;
    match case {
        Case::SubPending => {}
        Case::SubCancelled => sub.send(&mut build, FolderAttemptInput::Cancelled)?,
        Case::AllEnded | Case::ParentSkipsSubTerminal => sub.end(&mut build)?,
    }
    if case == Case::ParentSkipsSubTerminal {
        // The parent ends without the subfolder's terminal, so its end is rejected.
        let _ = parent.end(&mut build);
    } else {
        parent.recorded(&mut build, sub_child, Some(sub_token))?;
        parent.end(&mut build)?;
    }
    Ok(build)
}

fn space<'a>(version: &'a ResultVersion, name: &str) -> Result<&'a SpaceAggregate, String> {
    let item = if name.is_empty() {
        0
    } else {
        version
            .items
            .iter()
            .find(|item| item.name == name.as_bytes())
            .ok_or("missing item")?
            .item_no
    };
    version
        .space(item)
        .ok_or_else(|| "missing aggregate".into())
}
fn promoted(aggregate: &SpaceAggregate, bytes: u64) -> bool {
    aggregate.completeness == ResultCompleteness::Complete
        && aggregate.logical.amount == exact(bytes)
}

#[test]
fn proven_folders_are_promoted_recursively_per_basis_in_incomplete_versions() -> Test {
    for state in [
        ResultCompleteness::Missing,
        ResultCompleteness::Cancelled,
        ResultCompleteness::Partial,
    ] {
        let sink = IngestSink::default();
        let mut build = tree(&sink, Case::AllEnded)?;
        let version = if state == ResultCompleteness::Partial {
            build.publish_partial(id(2)?).map_err(e)?
        } else {
            build.seal(id(2)?, state).map_err(e)?
        };
        assert_eq!(version.completeness, state);
        assert!(promoted(space(&version, "sub")?, 5), "{state:?}");
        assert!(promoted(space(&version, "")?, 15), "{state:?}");
        assert!(promoted(space(&version, "b")?, 5), "{state:?}");
        // No allocation was observed, so that basis stays conservative.
        let root = space(&version, "")?;
        assert_ne!(
            root.allocated_completeness,
            ResultCompleteness::Complete,
            "{state:?}"
        );
        assert!(version.observation_range(0).is_some());
    }
    Ok(())
}

#[test]
fn an_unfinished_or_held_subfolder_keeps_every_ancestor_conservative() -> Test {
    for (case, state) in [
        (Case::SubPending, ResultCompleteness::Partial),
        (Case::SubPending, ResultCompleteness::Missing),
        (Case::SubCancelled, ResultCompleteness::Cancelled),
    ] {
        let sink = IngestSink::default();
        let mut build = tree(&sink, case)?;
        let version = if state == ResultCompleteness::Partial {
            build.publish_partial(id(2)?).map_err(e)?
        } else {
            build.seal(id(2)?, state).map_err(e)?
        };
        assert!(!promoted(space(&version, "sub")?, 5), "{state:?}");
        assert!(!promoted(space(&version, "")?, 15), "{state:?}");
        assert_eq!(space(&version, "")?.completeness, state, "{state:?}");
    }
    Ok(())
}

#[test]
fn a_missing_direct_child_terminal_keeps_the_parent_conservative() -> Test {
    let sink = IngestSink::default();
    let version = tree(&sink, Case::ParentSkipsSubTerminal)?
        .seal(id(2)?, ResultCompleteness::Missing)
        .map_err(e)?;
    // The subfolder's own subtree is proven; its parent's enumeration is not.
    assert!(promoted(space(&version, "sub")?, 5));
    assert!(!promoted(space(&version, "")?, 15));
    Ok(())
}

#[test]
fn a_gap_lowers_its_folder_and_ancestors_even_with_ended_attempts() -> Test {
    let sink = IngestSink::default();
    let mut build = tree(&sink, Case::AllEnded)?;
    build
        .record_gap_at(0, path(&["sub", "denied"]), "scan.path_denied".into(), None)
        .map_err(e)?;
    let version = build.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    assert_eq!(
        space(&version, "sub")?.completeness,
        ResultCompleteness::Missing
    );
    assert_eq!(
        space(&version, "")?.completeness,
        ResultCompleteness::Missing
    );
    Ok(())
}

#[test]
fn discarded_evidence_and_a_complete_version_promote_nothing_new() -> Test {
    let sink = IngestSink::default();
    let build = tree(&sink, Case::AllEnded)?;
    let blocker = sink
        .folder_evidence_budget
        .reserve_remaining()
        .ok_or("reserve remaining global evidence budget failed")?;
    let version = build.seal(id(2)?, ResultCompleteness::Missing).map_err(e)?;
    drop(blocker);
    assert!(version.folder_attempt_summaries().is_none());
    assert_eq!(
        space(&version, "sub")?.completeness,
        ResultCompleteness::Missing
    );
    assert_eq!(
        space(&version, "")?.completeness,
        ResultCompleteness::Missing
    );

    // A complete version keeps its existing exact totals with or without evidence.
    let sink = IngestSink::default();
    let version = tree(&sink, Case::SubPending)?
        .seal(id(3)?, ResultCompleteness::Complete)
        .map_err(e)?;
    assert!(promoted(space(&version, "")?, 15));
    Ok(())
}

#[test]
fn a_replacement_build_promotes_nothing() -> Test {
    let sink = IngestSink::default();
    sink.bind_current_analysis("folder-promotion-replacement", id(500)?)
        .map_err(e)?;
    let mut original = sink
        .open_forest_in_lineage(id(50)?, vec![root()], id(500)?)
        .map_err(e)?;
    original.add_directory(Vec::new()).map_err(e)?;
    let previous = original
        .seal(id(51)?, ResultCompleteness::Missing)
        .map_err(e)?;
    let mut replacement = sink.open_replacement(&previous).map_err(e)?;
    replacement.add_directory(Vec::new()).map_err(e)?;
    let attempt = Attempt::begin(&mut replacement, &[], 4)?;
    attempt.end(&mut replacement)?;
    let current = replacement
        .seal(id(52)?, ResultCompleteness::Missing)
        .map_err(e)?;
    assert!(current.folder_attempt_summaries().is_some());
    assert_eq!(
        space(&current, "")?.completeness,
        ResultCompleteness::Missing
    );
    // The previously published version is unchanged.
    assert_eq!(
        space(&previous, "")?.completeness,
        ResultCompleteness::Missing
    );
    Ok(())
}
