use du_analysis_index::{BuildError, BuildHandle, IngestSink, Observation, ResultVersion};
use du_core_contracts::{
    Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot, ResultVersionRead},
};

type Test = Result<(), String>;

fn e<T: std::fmt::Debug>(error: T) -> String {
    format!("{error:?}")
}
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn root() -> ResultScopeRoot {
    ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/r"),
        },
        volume_id: vec![1],
        file_id: vec![1],
    }
}
fn path(parts: &[&str]) -> Vec<Vec<u8>> {
    parts.iter().map(|part| part.as_bytes().to_vec()).collect()
}
fn dir(build: &mut BuildHandle, parts: &[&str]) -> Test {
    build.add_directory_at(0, path(parts)).map_err(e)
}
fn file(build: &mut BuildHandle, parts: &[&str], bytes: u64) -> Test {
    build
        .add_file(Observation {
            path: path(parts),
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: bytes.into(),
                },
            }),
        })
        .map_err(e)
}
fn space<'a>(
    version: &'a ResultVersion,
    name: &str,
) -> Result<&'a du_core_contracts::result::SpaceAggregate, String> {
    let item = version
        .items()
        .iter()
        .find(|item| item.name == name.as_bytes())
        .ok_or_else(|| format!("missing {name}"))?;
    version
        .space(item.item_no)
        .ok_or_else(|| format!("missing {name} aggregate"))
}

#[test]
fn abort_after_partial_publication_withdraws_the_failed_generation() -> Test {
    let sink = IngestSink::default();
    let lineage = sink
        .bind_current_analysis("failed-generation", id(4)?)
        .map_err(e)?;
    let mut original = sink
        .open_forest_in_lineage(id(1)?, vec![root()], lineage.clone())
        .map_err(e)?;
    dir(&mut original, &[])?;
    dir(&mut original, &["old"])?;
    file(&mut original, &["old", "file"], 30)?;
    let base = original
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(e)?;

    let mut next = sink
        .open_forest_in_lineage(id(1)?, vec![root()], lineage)
        .map_err(e)?;
    dir(&mut next, &[])?;
    let published = next.publish_partial(id(3)?).map_err(e)?;
    assert_eq!(published.completeness(), ResultCompleteness::Partial);
    assert_eq!(
        sink.pin_current(published.version_id())
            .map_err(e)?
            .version_id(),
        published.version_id()
    );
    assert_eq!(
        next.add_directory_at(0, path(&["bad", ""])),
        Err(BuildError::InvalidTree)
    );
    assert_eq!(
        next.seal(id(5)?, ResultCompleteness::Missing).err(),
        Some(BuildError::Unavailable)
    );
    assert_eq!(
        sink.pin_current(base.version_id()).map_err(e)?.version_id(),
        base.version_id()
    );
    assert_eq!(
        space(&base, "old")?.logical.amount,
        SizeAmount::Exact { bytes: 30.into() }
    );
    Ok(())
}
