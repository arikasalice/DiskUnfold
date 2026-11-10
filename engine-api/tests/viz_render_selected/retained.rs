use super::*;

#[test]
fn explicit_pin_survives_new_publication_and_release_expires_it() -> Result<(), String> {
    let sink = IngestSink::default();
    publish(&sink, id(2), id(3))?;
    let parent = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    let session = Session::new(
        sink.clone(),
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
        bundle::implemented_dispatcher().map_err(|e| format!("{e:?}"))?,
    );
    let rendered = session
        .render_selected(
            request(&parent, 1000),
            |_, item, _| Ok(policy(item)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    let rows = VizRowsRequest {
        model_ref: rendered.model_ref.clone(),
        result_revision: id(3),
        start: 0,
        count: 10,
    };
    let before = session.rows(&rows).map_err(|e| format!("{e:?}"))?;
    let item = before.rows[0].item_ref.clone().ok_or("missing item")?;
    let location = session
        .locate(&rendered.model_ref, &item)
        .map_err(|e| format!("{e:?}"))?;
    session
        .pin_model(&rendered.model_ref, &id(3))
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        session.pin_model(&rendered.model_ref, &id(3)),
        Err(viz_pages::Error::InvalidModel)
    );
    publish(&sink, id(4), id(5))?;
    assert_eq!(session.rows(&rows), Err(viz_pages::Error::StaleVersion));
    assert_eq!(
        session.pinned_rows(&rows).map_err(|e| format!("{e:?}"))?,
        before
    );
    assert_eq!(
        session
            .pinned_locate(&rendered.model_ref, &item)
            .map_err(|e| format!("{e:?}"))?,
        location
    );
    let wrong = VizRowsRequest {
        result_revision: id(5),
        ..rows.clone()
    };
    assert_eq!(
        session.pinned_rows(&wrong),
        Err(viz_pages::Error::StaleVersion)
    );
    assert_eq!(
        session.pinned_locate("wrong", &item),
        Err(viz_pages::Error::ModelExpired)
    );
    assert_eq!(
        session.release_pinned_model("wrong"),
        Err(viz_pages::Error::ModelExpired)
    );
    let next_parent = ItemRef {
        result_version: id(5),
        item_no: 0,
    };
    let next = session
        .render_selected(
            request(&next_parent, 1000),
            |_, item, _| Ok(policy(item)),
            &mut || false,
        )
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        session.pinned_rows(&rows).map_err(|e| format!("{e:?}"))?,
        before
    );
    assert_eq!(
        session.pin_model(&next.model_ref, &id(5)),
        Err(viz_pages::Error::InvalidModel)
    );
    session
        .release_pinned_model(&rendered.model_ref)
        .map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        session.pinned_rows(&rows),
        Err(viz_pages::Error::ModelExpired)
    );
    assert_eq!(
        session.pin_model(&rendered.model_ref, &id(3)),
        Err(viz_pages::Error::ModelExpired)
    );
    session
        .pin_model(&next.model_ref, &id(5))
        .map_err(|e| format!("{e:?}"))?;
    session
        .release_pinned_model(&next.model_ref)
        .map_err(|e| format!("{e:?}"))?;
    Ok(())
}
