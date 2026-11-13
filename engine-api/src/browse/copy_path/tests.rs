use super::*;
use du_core_contracts::{
    Id, ItemRef, PathBytes, PathEncoding,
    result::{ResultCompleteness, ResultScopeRoot},
};

fn id(value: u8) -> Id {
    Id::from_v7_bytes([value; 16])
}

fn request(version: &Id, numbers: impl IntoIterator<Item = u32>) -> BrowseCopyPathRequest {
    BrowseCopyPathRequest {
        result_version: version.clone(),
        items: numbers
            .into_iter()
            .map(|item_no| ItemRef {
                result_version: version.clone(),
                item_no,
            })
            .collect(),
    }
}

fn text(path: &ResultNativePath) -> Option<String> {
    String::from_utf8(path.raw.bytes().ok()?).ok()
}

fn seeded() -> Result<(PathCopy, Id), String> {
    let store = IngestSink::default();
    let revision = id(2);
    let names: [&[u8]; 4] = [
        "/한글 ".as_bytes(),
        "/e\u{301}".as_bytes(),
        b"/a\nb",
        b"/\xff",
    ];
    let roots = names
        .iter()
        .map(|name| ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(name),
            },
            volume_id: vec![1],
            file_id: name.to_vec(),
        })
        .collect();
    let mut build = store
        .open_forest(id(1), roots)
        .map_err(|e| format!("{e:?}"))?;
    for index in 0..4 {
        build
            .add_directory_at(index, vec![])
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(revision.clone(), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    Ok((PathCopy(store), revision))
}

#[test]
fn preserves_original_text_and_uses_pinned_item_order() -> Result<(), String> {
    let (copy, revision) = seeded()?;
    let prepared = copy
        .prepare(request(&revision, [1, 0]), "\r\n", text)
        .map_err(|e| format!("{e:?}"))?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(prepared.text, "/한글 \r\n/e\u{301}");
    assert_eq!(prepared.count, 2);
    let single = copy
        .prepare(request(&revision, [2]), "\r\n", text)
        .map_err(|e| format!("{e:?}"))?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(single.text, "/a\nb");
    Ok(())
}

#[test]
fn refuses_whole_batch_when_any_path_is_ambiguous_or_unrepresentable() -> Result<(), String> {
    let (copy, revision) = seeded()?;
    for (numbers, reason) in [
        ([0, 2], BrowseCopyPathRefusal::Multiline),
        ([0, 3], BrowseCopyPathRefusal::Unrepresentable),
    ] {
        let outcome = copy
            .prepare(request(&revision, numbers), "\n", text)
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(outcome.err(), Some(reason));
    }
    Ok(())
}

#[test]
fn bounds_and_reference_errors_precede_conversion() -> Result<(), String> {
    let (copy, revision) = seeded()?;
    let mut mixed = request(&revision, [0, 1]);
    mixed.items[1].result_version = id(3);
    let outcomes = [
        (request(&revision, []), "command.body_invalid"),
        (request(&revision, [0; 1_001]), "command.body_invalid"),
        (mixed, "result.requery_required"),
        (request(&id(3), [0]), "result.requery_required"),
        (request(&revision, [0, 99]), "ref.unavailable"),
    ];
    let mut conversions = 0;
    for (request, reason) in outcomes {
        assert_eq!(
            copy.prepare(request, "\n", |_| {
                conversions += 1;
                None
            })
            .err(),
            Some(EngineError::reason(reason))
        );
    }
    assert_eq!(conversions, 0);
    let limit = copy
        .prepare(request(&revision, [0; 1_000]), "\n", text)
        .map_err(|e| format!("{e:?}"))?
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(limit.count, 1_000);
    Ok(())
}
