use du_analysis_index::{
    IngestSink, Observation,
    query::{self, Basis, Condition, Registry, Sort, page::ResultSets},
};
use du_core_contracts::{
    Id, PathBytes, PathEncoding, SizeAmount, SizeBasis, SizeEvidence, SizeValue, Timestamp,
    browse_ipc::BrowseValue,
    browse_page_ipc::QueryTotal,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_engine_api::search_page::{self, ProjectionError};
use std::sync::Arc;

fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn utf16(units: &[u16]) -> Vec<u8> {
    units.iter().flat_map(|unit| unit.to_le_bytes()).collect()
}
fn sort() -> Sort<'static> {
    Sort {
        id: "sort.path",
        revision: 1,
        basis: Basis::Logical,
        descending: false,
    }
}

#[test]
fn projected_rows_retain_source_and_search_kind_after_replacement() -> Result<(), String> {
    let store = IngestSink::default();
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::WindowsUtf16le,
            raw: PathBytes::from_bytes(&utf16(&[67, 58, 92, 54260, 45908])),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut build = store
        .open_observed(id(1)?, root)
        .map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let first = utf16(&[97]);
    let second = utf16(&[0xdcff]);
    for (name, bytes) in [(&first, 7), (&second, 9)] {
        build
            .add_file(Observation {
                path: vec![name.clone()],
                size: Some(SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: bytes.into(),
                    },
                }),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    let modified = Timestamp::parse("2026-01-01T12:00:00.000Z").map_err(|e| format!("{e:?}"))?;
    build
        .set_modified_at(0, std::slice::from_ref(&first), modified.clone())
        .map_err(|e| format!("{e:?}"))?;
    let observed = Timestamp::from_unix_millis(100).ok_or("time")?;
    let version = build
        .seal_at(id(2)?, ResultCompleteness::Complete, Some(observed.clone()))
        .map_err(|e| format!("{e:?}"))?;
    let registry = Registry::baseline();
    let folder = query::run_folder(
        &registry,
        version,
        None,
        &Condition::All(vec![]),
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let search = query::run_in_result(&registry, &folder, &Condition::All(vec![]), sort(), || {
        false
    })
    .map_err(|e| format!("{e:?}"))?;
    let sets = ResultSets::new(store.clone());
    sets.register(id(3)?, folder)
        .map_err(|e| format!("{e:?}"))?;
    sets.register(id(4)?, search)
        .map_err(|e| format!("{e:?}"))?;
    let a = sets
        .open_page(&id(4)?, &id(2)?, 0, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    let b = sets
        .open_page(&id(4)?, &id(2)?, 1, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    let folder_page = sets
        .open_page(&id(3)?, &id(2)?, 0, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        search_page::project(&folder_page, None),
        Err(ProjectionError::NotSearch)
    );
    let mut replacement = store.open(id(5)?).map_err(|e| format!("{e:?}"))?;
    replacement
        .add_directory(vec![])
        .map_err(|e| format!("{e:?}"))?;
    replacement
        .seal(id(6)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    assert!(sets.open_page(&id(4)?, &id(2)?, 0, None).is_err());
    let a = search_page::project(&a, Some("admitted-cursor".into()))
        .map_err(|e| format!("{e:?}"))?
        .page;
    let b = search_page::project(&b, None)
        .map_err(|e| format!("{e:?}"))?
        .page;
    assert_eq!(a.result_version, id(2)?);
    assert_eq!(a.result_version, b.result_version);
    assert_eq!(a.observed_at, Some(observed));
    assert_eq!(a.total, QueryTotal::Exact { count: 2 });
    assert_eq!(a.next_cursor.as_deref(), Some("admitted-cursor"));
    assert_eq!(
        a.rows[0].display_path,
        BrowseValue::Present {
            value: "C:\\폴더\\a".into()
        }
    );
    assert_eq!(
        a.rows[0].modified_at,
        BrowseValue::Present { value: modified }
    );
    assert_eq!(
        b.rows[0].display_name,
        BrowseValue::Present {
            value: "�".into()
        }
    );
    assert_eq!(
        b.rows[0].display_path,
        BrowseValue::Present {
            value: "C:\\폴더\\�".into()
        }
    );
    assert!(matches!(b.rows[0].created_at, BrowseValue::Unknown { .. }));
    assert!(matches!(
        b.rows[0].allocated_size,
        BrowseValue::Unknown { .. }
    ));
    assert!(b.rows[0].missing_fields.is_empty() && b.rows[0].reason_keys.is_empty());
    Ok(())
}

#[test]
fn absent_root_and_missing_values_remain_unknown() -> Result<(), String> {
    let store = IngestSink::default();
    let mut build = store.open(id(11)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"file".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(12)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let search = query::run(
        &Registry::baseline(),
        version,
        &Condition::Predicate(query::Predicate::Kind(query::Kind::File)),
        sort(),
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    let sets = ResultSets::new(store);
    sets.register(id(13)?, search)
        .map_err(|e| format!("{e:?}"))?;
    let page = sets
        .open_page(&id(13)?, &id(12)?, 0, None)
        .map_err(|e| format!("{e:?}"))?;
    let page = search_page::project(&page, None)
        .map_err(|e| format!("{e:?}"))?
        .page;
    let row = &page.rows[0];
    assert!(matches!(row.display_name, BrowseValue::Unknown { .. }));
    assert!(matches!(row.display_path, BrowseValue::Unknown { .. }));
    assert!(matches!(row.logical_size, BrowseValue::Unknown { .. }));
    assert!(matches!(row.modified_at, BrowseValue::Unknown { .. }));
    assert!(matches!(row.created_at, BrowseValue::Unknown { .. }));
    assert!(row.missing_fields.is_empty() && row.reason_keys.is_empty());
    Ok(())
}

#[test]
fn prepared_search_preserves_indeterminate_counts_through_page_projection() -> Result<(), String> {
    use du_analysis_index::query::{
        Boundary, Kind, Predicate, SortSpec,
        prepared::{PreparedSearch, PreparedSearchError, SearchInput},
    };

    let store = IngestSink::default();
    let mut build = store.open(id(21)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    for (name, size) in [
        (b"match".as_slice(), Some(5_u64)),
        (b"another".as_slice(), Some(4_u64)),
        (b"miss", Some(1)),
        (b"unknown", None),
    ] {
        build
            .add_file(Observation {
                path: vec![name.to_vec()],
                size: size.map(|bytes| SizeValue {
                    basis: SizeBasis::Logical,
                    evidence: SizeEvidence::Measured,
                    amount: SizeAmount::Exact {
                        bytes: bytes.into(),
                    },
                }),
            })
            .map_err(|e| format!("{e:?}"))?;
    }
    build
        .seal(id(22)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let sets = Arc::new(ResultSets::new(store));
    let registry = Registry::baseline();
    let input = || SearchInput {
        condition: Condition::All(vec![
            Condition::Predicate(Predicate::Kind(Kind::File)),
            Condition::Predicate(Predicate::Size(Basis::Logical, Boundary::AtLeast(3))),
        ]),
        sorts: vec![SortSpec::from(&sort())],
        count: Some(1),
    };
    let admission = sets
        .admit_search(Some(&id(22)?), None)
        .map_err(|e| format!("{e:?}"))?;
    let candidate = PreparedSearch::new(sets.clone(), admission, &registry, input())
        .map_err(|e| format!("{e:?}"))?
        .evaluate(|| false)
        .map_err(|e| format!("{e:?}"))?;
    let first = sets
        .register_admitted_search(id(23)?, candidate)
        .map_err(|e| format!("{e:?}"))?;
    let page = search_page::project(&first, None)
        .map_err(|e| format!("{e:?}"))?
        .page;
    assert_eq!(page.result_version, id(22)?);
    assert_eq!(page.total, QueryTotal::LowerBound { count: 2 });
    assert_eq!(page.unknown_count, 1);
    assert_eq!(page.rows.len(), 1);
    assert_eq!(page.rows[0].item.item_no, 1);
    let second = sets
        .open_page(&id(23)?, &id(22)?, 1, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    let second = search_page::project(&second, None)
        .map_err(|e| format!("{e:?}"))?
        .page;
    assert_eq!(second.total, QueryTotal::LowerBound { count: 2 });
    assert_eq!(second.unknown_count, 1);
    assert_eq!(second.rows[0].item.item_no, 2);

    let admission = sets
        .admit_search(Some(&id(22)?), None)
        .map_err(|e| format!("{e:?}"))?;
    let cancelled = PreparedSearch::new(sets.clone(), admission, &registry, input())
        .map_err(|e| format!("{e:?}"))?
        .evaluate(|| true);
    assert!(matches!(
        cancelled,
        Err(PreparedSearchError::Query(query::Error::Cancelled))
    ));
    let retained = sets
        .open_page(&id(23)?, &id(22)?, 0, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    let retained = search_page::project(&retained, None)
        .map_err(|e| format!("{e:?}"))?
        .page;
    assert_eq!(retained.total, QueryTotal::LowerBound { count: 2 });
    assert_eq!(retained.unknown_count, 1);
    assert_eq!(retained.rows[0].item, page.rows[0].item);
    Ok(())
}
