//! FR1.HD1.app-host.DD1: results carry the frozen scope version and fingerprint.
use du_analysis_index::{BuildError, IngestSink, ScopeHeader};
use du_core_contracts::{
    Id, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

fn id(n: u32) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}

fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&[67, 0]),
        },
        volume_id: vec![1],
        file_id: vec![1],
    }
}

#[test]
fn every_publication_and_replacement_repeats_the_scope_header() -> Result<(), String> {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("scope-header", id(1)?)
        .map_err(|error| format!("{error:?}"))?;
    let header = ScopeHeader {
        version_id: id(2)?,
        fingerprint: "a1b2".into(),
    };
    let mut build = sink
        .open_scoped_forest(header.clone(), vec![root()], Some(lineage.clone()))
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let partial = build
        .publish_partial(id(3)?)
        .map_err(|error| format!("{error:?}"))?;
    let sealed = build
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    for version in [&partial, &sealed] {
        assert_eq!(version.scope_version_id(), &header.version_id);
        assert_eq!(version.scope_fingerprint(), Some("a1b2"));
        assert_eq!(version.lineage_id(), Some(&lineage));
    }
    let mut replacement = sink
        .open_replacement(&sealed)
        .map_err(|error| format!("{error:?}"))?;
    replacement
        .add_directory_at(0, Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let replaced = replacement
        .seal(id(5)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(replaced.scope_version_id(), &header.version_id);
    assert_eq!(replaced.scope_fingerprint(), Some("a1b2"));
    Ok(())
}

#[test]
fn unscoped_builds_have_no_fingerprint_and_empty_one_is_rejected() -> Result<(), String> {
    let sink = IngestSink::default();
    let empty = ScopeHeader {
        version_id: id(6)?,
        fingerprint: String::new(),
    };
    assert_eq!(
        sink.open_scoped_forest(empty, vec![root()], None).err(),
        Some(BuildError::InvalidTree)
    );
    let mut build = sink
        .open_forest(id(7)?, vec![root()])
        .map_err(|error| format!("{error:?}"))?;
    build
        .add_directory_at(0, Vec::new())
        .map_err(|error| format!("{error:?}"))?;
    let version = build
        .seal(id(8)?, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))?;
    assert_eq!(version.scope_fingerprint(), None);
    Ok(())
}
