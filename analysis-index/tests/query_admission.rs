use du_analysis_index::{
    IngestSink, Observation,
    query::{
        self,
        page::{MultiFolderKey, PageError, ResultSets},
    },
};
use du_core_contracts::{Id, result::ResultCompleteness};
fn id(n: u8) -> Result<Id, String> {
    Id::parse(&format!("018f0000-0000-7000-8000-{n:012x}"))
}
fn candidate(admission: &query::page::SourceAdmission) -> Result<query::QueryResult, String> {
    query::run_folder(
        &query::Registry::baseline(),
        admission.version(),
        None,
        &query::Condition::All(vec![]),
        query::Sort {
            id: "sort.path",
            revision: 1,
            basis: query::Basis::Logical,
            descending: false,
        },
        || false,
    )
    .map_err(|e| format!("{e:?}"))
}
fn key(version: Id, result: &query::QueryResult) -> MultiFolderKey {
    MultiFolderKey {
        version,
        folder: query::FolderLocation::Item(0),
        tree: false,
        condition: result.condition.clone(),
        sorts: result.sorts.clone(),
    }
}
#[test]
fn admitted_page_survives_release_but_cannot_admit_another_page() -> Result<(), String> {
    let source = IngestSink::default();
    let mut build = source.open(id(1)?).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    build
        .add_file(Observation {
            path: vec![b"x".to_vec()],
            size: None,
        })
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(2)?, ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let sets = ResultSets::new(source.clone());
    let other = ResultSets::new(source);
    let token = sets
        .admit_source(Some(&id(2)?))
        .map_err(|e| format!("{e:?}"))?;
    let result = candidate(&token)?;
    let wrong_key = key(id(2)?, &result);
    assert!(matches!(
        other.register_admitted_folder(token, wrong_key, id(3)?, result, Some(1)),
        Err(PageError::InvalidResult)
    ));
    let token = sets
        .admit_source(Some(&id(2)?))
        .map_err(|e| format!("{e:?}"))?;
    let result = candidate(&token)?;
    let key = key(id(2)?, &result);
    sets.release_source(&id(2)?).map_err(|e| format!("{e:?}"))?;
    let (q, first) = sets
        .register_admitted_folder(token, key, id(4)?, result, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(first.items().len(), 1);
    assert!(matches!(
        sets.open_page(&q, &id(2)?, 0, Some(1)),
        Err(PageError::StaleVersion)
    ));
    assert!(matches!(
        sets.admit_source(Some(&id(2)?)),
        Err(PageError::StaleVersion)
    ));
    Ok(())
}
