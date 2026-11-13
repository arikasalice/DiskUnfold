use du_analysis_index::{BuildError, IngestSink, Observation};
use du_core_contracts::Id;
use du_core_contracts::result::{ResultCompleteness, ResultVersionRead};

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn add(build: &mut du_analysis_index::BuildHandle, path: &[&str]) -> Result<(), BuildError> {
    build.add(Observation {
        path: path.iter().map(|s| s.as_bytes().to_vec()).collect(),
        size: None,
    })
}

#[test]
fn order_is_deterministic_and_old_reader_is_isolated() -> Result<(), Box<dyn std::error::Error>> {
    // Map typed build failures into a test error without weakening the library lint policy.
    fn err(e: BuildError) -> String {
        format!("{e:?}")
    }
    let sink = IngestSink::default();
    let mut first = sink.open(id(1)?).map_err(err)?;
    for path in [&[][..], &["b"], &["a", "child"], &["a"]] {
        add(&mut first, path).map_err(err)?;
    }
    let pinned = first
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let mut second = sink.open(id(1)?).map_err(err)?;
    for path in [&["a"][..], &["a", "child"], &["b"], &[]] {
        add(&mut second, path).map_err(err)?;
    }
    let partial = second.publish_partial(id(3)?).map_err(err)?;
    assert_eq!(pinned.items(), partial.items());
    assert_eq!(
        partial
            .items()
            .iter()
            .map(|i| (i.item_no, i.parent, i.name.as_slice()))
            .collect::<Vec<_>>(),
        vec![
            (0, None, &b""[..]),
            (1, Some(0), &b"a"[..]),
            (2, Some(0), &b"b"[..]),
            (3, Some(1), &b"child"[..])
        ]
    );
    add(&mut second, &["c"]).map_err(err)?;
    let complete = second
        .seal(id(4)?, ResultCompleteness::Complete)
        .map_err(err)?;
    assert_eq!(pinned.items().len(), 4);
    assert_eq!(partial.completeness(), ResultCompleteness::Partial);
    assert_eq!(partial.items().len(), 4);
    assert_eq!(complete.items().len(), 5);
    Ok(())
}

#[test]
fn failure_withdraws_partial_and_preserves_reader() -> Result<(), Box<dyn std::error::Error>> {
    fn err(e: BuildError) -> String {
        format!("{e:?}")
    }
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    add(&mut build, &[]).map_err(err)?;
    let original = build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let mut failing = sink.open(id(1)?).map_err(err)?;
    assert!(matches!(sink.open(id(1)?), Err(BuildError::Busy)));
    add(&mut failing, &[]).map_err(err)?;
    let partial = failing.publish_partial(id(3)?).map_err(err)?;
    assert_eq!(add(&mut failing, &[]), Err(BuildError::InvalidTree));
    let current = sink.pin().map_err(err)?.ok_or("missing current")?;
    assert_eq!(current.version_id(), original.version_id());
    assert_eq!(partial.version_id(), &id(3)?);
    assert!(matches!(
        failing.publish_partial(id(4)?),
        Err(BuildError::Unavailable)
    ));
    let mut incomplete = sink.open(id(1)?).map_err(err)?;
    add(&mut incomplete, &[]).map_err(err)?;
    add(&mut incomplete, &["missing", "child"]).map_err(err)?;
    assert!(matches!(
        incomplete.seal(id(5)?, ResultCompleteness::Complete),
        Err(BuildError::InvalidTree)
    ));
    assert_eq!(
        sink.pin()
            .map_err(err)?
            .ok_or("missing current")?
            .version_id(),
        original.version_id()
    );
    Ok(())
}

#[test]
fn publication_is_atomic_for_concurrent_readers() -> Result<(), Box<dyn std::error::Error>> {
    fn err(e: BuildError) -> String {
        format!("{e:?}")
    }
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)?).map_err(err)?;
    add(&mut build, &[]).map_err(err)?;
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(err)?;
    let reader = sink.clone();
    let thread = std::thread::spawn(move || -> Result<(), String> {
        for _ in 0..1000 {
            let version = reader.pin().map_err(err)?.ok_or("missing result")?;
            let n = version.items().len();
            if n != 1 && n != 2 {
                return Err("mixed result".into());
            }
        }
        Ok(())
    });
    let mut next = sink.open(id(1)?).map_err(err)?;
    add(&mut next, &[]).map_err(err)?;
    add(&mut next, &["child"]).map_err(err)?;
    next.seal(id(3)?, ResultCompleteness::Partial)
        .map_err(err)?;
    thread.join().map_err(|_| "reader panicked")??;
    assert_eq!(
        sink.pin()
            .map_err(err)?
            .ok_or("missing result")?
            .completeness(),
        ResultCompleteness::Partial
    );
    Ok(())
}
