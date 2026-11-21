//! The scan fixture the level tests share: folders, files with chosen physical
//! evidence, links and unread entries, sealed into one pinned result.
use super::*;
use du_core_contracts::{
    PathBytes, PathEncoding,
    result::{ResultNativePath, ResultScopeRoot},
};

pub(super) enum Entry<'a> {
    Dir(&'a str),
    /// A file with its own identity; its allocated size equals its logical size.
    File(&'a str, u64),
    /// A file with a chosen identity and link count: path, bytes, identity, links.
    Linked(&'a str, u64, u8, u64),
    /// A link that was not followed.
    Link(&'a str),
    /// A link that was not followed, observed with its own size: path, bytes.
    SizedLink(&'a str, u64),
    /// An entry that could not be read: no size and an unknown kind.
    Unread(&'a str),
    /// A file with a logical size but no allocation evidence: path, bytes.
    NoAllocation(&'a str, u64),
    /// A file whose OS-reported allocation is itself a lower bound: path, bytes.
    LowerAllocation(&'a str, u64),
}

pub(super) fn scan(
    entries: &[Entry<'_>],
    completeness: ResultCompleteness,
    gaps: &[&str],
) -> Arc<ResultVersion> {
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/root"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    };
    let sink = IngestSink::default();
    let mut build = sink.open_observed(id(1), root).unwrap();
    build.add_directory(vec![]).unwrap();
    let path = |text: &str| -> Vec<Vec<u8>> {
        text.split('/')
            .map(|part| part.as_bytes().to_vec())
            .collect()
    };
    let physical = |file: u8, links: u64, bytes: u64| SpaceObservation {
        allocated: Some(size(SizeBasis::Allocated, bytes)),
        identity: Some(FileIdentity {
            volume: vec![1],
            file: vec![file],
        }),
        link_count: Some(links),
        sharing: SharingEvidence::None,
    };
    let mut identity = 100_u8;
    for entry in entries {
        match *entry {
            Entry::Dir(at) => build.add_directory(path(at)).unwrap(),
            Entry::File(at, bytes) => {
                identity += 1;
                build
                    .add_with_space(
                        Observation {
                            path: path(at),
                            size: Some(logical(bytes)),
                        },
                        physical(identity, 1, bytes),
                    )
                    .unwrap();
            }
            Entry::Linked(at, bytes, file, links) => build
                .add_with_space(
                    Observation {
                        path: path(at),
                        size: Some(logical(bytes)),
                    },
                    physical(file, links, bytes),
                )
                .unwrap(),
            Entry::Link(at) => build.add_untraversed_link(path(at)).unwrap(),
            Entry::SizedLink(at, bytes) => {
                identity += 1;
                let own = Observation {
                    path: path(at),
                    size: Some(logical(bytes)),
                };
                build
                    .add_observed_untraversed_link_at(0, own, Some(physical(identity, 1, bytes)))
                    .unwrap();
            }
            Entry::Unread(at) => build
                .add(Observation {
                    path: path(at),
                    size: None,
                })
                .unwrap(),
            Entry::NoAllocation(at, bytes) => {
                identity += 1;
                let mut observed = physical(identity, 1, bytes);
                observed.allocated = None;
                build
                    .add_with_space(
                        Observation {
                            path: path(at),
                            size: Some(logical(bytes)),
                        },
                        observed,
                    )
                    .unwrap();
            }
            Entry::LowerAllocation(at, bytes) => {
                identity += 1;
                let mut observed = physical(identity, 1, bytes);
                observed.allocated = Some(SizeValue {
                    amount: SizeAmount::LowerBound {
                        bytes: bytes.into(),
                    },
                    ..size(SizeBasis::Allocated, bytes)
                });
                build
                    .add_with_space(
                        Observation {
                            path: path(at),
                            size: Some(logical(bytes)),
                        },
                        observed,
                    )
                    .unwrap();
            }
        }
    }
    for gap in gaps {
        build
            .record_gap_at(0, path(gap), "scan.path_denied".into(), None)
            .unwrap();
    }
    build.seal(id(2), completeness).unwrap()
}
