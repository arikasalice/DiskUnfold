use super::*;

#[test]
fn session_enumerates_pinned_children_and_requires_policy_evidence() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let session = Session::new(
        sink,
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(21))),
        bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
    );
    let mut seen = Vec::new();
    let rendered = session
        .render_selected(
            request(&parent, 1000),
            |source, item, _| {
                assert_eq!(source.result_version(), &id(3));
                assert_eq!(item.parent, Some(parent.item_no));
                seen.push((item.item_no, item.name.clone()));
                Ok(policy(item))
            },
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(seen.len(), 2);
    assert_eq!(rendered.model.rows.len(), seen.len());
    assert!(
        rendered
            .model
            .rows
            .iter()
            .all(|row| row.item.result_version == id(3))
    );
    let error = session.render_selected(
        request(&parent, 500),
        |_, _, _| Err(Error::InvalidEvidence),
        &mut || false,
    );
    assert!(matches!(error, Err(Error::InvalidEvidence)));
    let visited = std::cell::Cell::new(0);
    let error = session.render_selected(
        request(&parent, 500),
        |_, _, _| {
            visited.set(visited.get() + 1);
            Ok(ChildPolicyEvidence {
                path_compare_key: b"key".to_vec(),
                legend_class_key: "fixture.class".into(),
                display_flags: vec![],
            })
        },
        &mut || visited.get() > 0,
    );
    assert!(matches!(error, Err(Error::Cancelled)));

    let file = ItemRef {
        result_version: id(3),
        item_no: seen[0].0,
    };
    let error = session.render_selected(
        request(&file, 500),
        |_, _, _| unreachable!("a file has no admitted folder children"),
        &mut || false,
    );
    assert!(matches!(
        error,
        Err(Error::Projection(
            du_analysis_index::visualization::projection::Error::InvalidSource
        ))
    ));
    Ok(())
}
