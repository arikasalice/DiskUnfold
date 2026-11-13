use du_analysis_index::{IngestSink, query};
use du_core_contracts::{
    Id, PathBytes, PathEncoding, Timestamp,
    result::{ResultCompleteness, ResultNativePath, ResultScopeRoot},
};
use du_engine_api::browse::folder::{FolderBrowse, OpenRequest, Sort, View};
use du_job_runtime::JobIdentity;
use std::sync::{
    Arc,
    atomic::{AtomicU8, Ordering},
};

struct Identity(AtomicU8);
impl JobIdentity for Identity {
    fn next_id(&self) -> Id {
        Id::from_v7_bytes([self.0.fetch_add(1, Ordering::SeqCst); 16])
    }
}
fn id(n: u8) -> Id {
    Id::from_v7_bytes([n; 16])
}
fn request() -> OpenRequest {
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
    }
}

#[test]
fn exact_reported_gaps_stay_bound_to_the_observed_root_and_page() -> Result<(), String> {
    let sink = IngestSink::default();
    let roots = [b"/a".as_slice(), b"/b".as_slice()]
        .into_iter()
        .enumerate()
        .map(|(index, path)| ResultScopeRoot {
            path: ResultNativePath {
                encoding: PathEncoding::Unix,
                raw: PathBytes::from_bytes(path),
            },
            volume_id: vec![index as u8 + 1],
            file_id: vec![index as u8 + 1],
        })
        .collect::<Vec<_>>();
    let mut build = sink
        .open_forest(id(1), roots)
        .map_err(|e| format!("{e:?}"))?;
    for root in 0..2 {
        build
            .add_directory_at(root, vec![])
            .map_err(|e| format!("{e:?}"))?;
        build
            .add_directory_at(root, vec![b"same".to_vec()])
            .map_err(|e| format!("{e:?}"))?;
        build
            .add_directory_at(root, vec![b"aaa".to_vec()])
            .map_err(|e| format!("{e:?}"))?;
    }
    let when = Timestamp::parse("2020-01-02T03:04:05.006Z")?;
    build
        .record_gap_at(
            0,
            vec![b"same".to_vec()],
            "scan.path_denied".into(),
            Some(when.clone()),
        )
        .map_err(|e| format!("{e:?}"))?;
    build
        .record_gap_at(
            1,
            vec![b"same".to_vec(), b"unknown".to_vec()],
            "scan.path_vanished".into(),
            None,
        )
        .map_err(|e| format!("{e:?}"))?;
    let prior = build.publish_partial(id(2)).map_err(|e| format!("{e:?}"))?;
    build
        .record_gap_at(0, vec![b"same".to_vec()], "scan.path_vanished".into(), None)
        .map_err(|e| format!("{e:?}"))?;
    build
        .seal(id(3), ResultCompleteness::Partial)
        .map_err(|e| format!("{e:?}"))?;
    let api = FolderBrowse::new(sink, Arc::new(Identity(AtomicU8::new(8))));
    let top = api
        .open(request(), || false)
        .map_err(|e| format!("{e:?}"))?;
    assert!(top.page.rows[0].reported_gaps.is_empty());
    let second = api
        .page(&top.page.query_ref, &top.page.result_version, 1, Some(1))
        .map_err(|e| format!("{e:?}"))?;
    for (i, root) in [top.page.rows[0].item.clone(), second.rows[0].item.clone()]
        .into_iter()
        .enumerate()
    {
        let mut req = request();
        req.location = Some(root);
        let folder = api.open(req, || false).map_err(|e| format!("{e:?}"))?;
        assert!(folder.page.rows[0].reported_gaps.is_empty());
        let next = api
            .page(
                &folder.page.query_ref,
                &folder.page.result_version,
                1,
                Some(1),
            )
            .map_err(|e| format!("{e:?}"))?;
        let row = &next.rows[0];
        if i == 0 {
            assert_eq!(row.reported_gaps.len(), 2);
            assert_eq!(row.reported_gaps[0].reason_key, "scan.path_denied");
            assert_eq!(row.reported_gaps[0].observed_at.as_ref(), Some(&when));
            assert_eq!(row.reported_gaps[1].reason_key, "scan.path_vanished");
            let prior_item = du_core_contracts::ItemRef {
                result_version: id(2),
                ..row.item.clone()
            };
            let mut old_gaps = prior
                .item_scope_gaps(&prior_item)
                .map_err(|e| format!("{e:?}"))?;
            assert_eq!(
                old_gaps.next().map(|gap| gap.reason_key.as_str()),
                Some("scan.path_denied")
            );
            assert!(old_gaps.next().is_none());
            assert!(matches!(
                prior.item_scope_gaps(&row.item),
                Err(du_analysis_index::QueryError::StaleVersion)
            ));
        } else {
            assert!(row.reported_gaps.is_empty());
        }
    }
    Ok(())
}
