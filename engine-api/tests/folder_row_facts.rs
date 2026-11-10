use du_analysis_index::{IngestSink, Observation, query};
use du_core_contracts::{Id, Timestamp, result::ResultCompleteness};
use du_engine_api::browse::folder::{FolderBrowse, OpenRequest, Sort, View};
use du_job_runtime::JobIdentity;
use std::sync::Arc;

struct Identity;
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes([8; 16])
    }
}
fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}

#[test]
fn opened_and_subsequent_pages_preserve_only_observed_facts() -> Result<(), String> {
    for completeness in [ResultCompleteness::Complete, ResultCompleteness::Partial] {
        let sink = IngestSink::default();
        let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
        for path in [
            vec![],
            vec![b"a".to_vec()],
            vec![b"a".to_vec(), b"sub".to_vec()],
            vec![b"b".to_vec()],
        ] {
            build.add_directory(path).map_err(|e| format!("{e:?}"))?;
        }
        build
            .add_file(Observation {
                path: vec![b"c".to_vec()],
                size: None,
            })
            .map_err(|e| format!("{e:?}"))?;
        let time = Timestamp::parse("2020-01-02T03:04:05.006Z")?;
        build
            .set_modified_at(0, &[b"a".to_vec()], time.clone())
            .map_err(|e| format!("{e:?}"))?;
        build
            .seal(id(2), completeness)
            .map_err(|e| format!("{e:?}"))?;
        let api = FolderBrowse::new(sink, Arc::new(Identity));
        let first = api
            .open(
                OpenRequest {
                    result_version: None,
                    location: None,
                    view: View::List,
                    condition: query::Condition::All(vec![]),
                    sort: Sort {
                        id: "sort.path".into(),
                        revision: 1,
                        basis: query::Basis::Logical,
                        descending: false,
                    },
                    page_size: Some(1),
                },
                || false,
            )
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(first.page.rows[0].observation.name, b"a");
        assert_eq!(first.page.rows[0].modified_at.as_ref(), Some(&time));
        assert_eq!(first.page.rows[0].has_observed_child_folder, Some(true));
        let next = api
            .page(
                &first.page.query_ref,
                &first.page.result_version,
                1,
                Some(2),
            )
            .map_err(|e| format!("{e:?}"))?;
        assert_eq!(next.rows.len(), 2);
        for row in next.rows {
            assert_eq!(row.modified_at, None);
            assert_eq!(row.has_observed_child_folder, None);
        }
    }
    Ok(())
}
