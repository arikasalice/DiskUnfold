use super::*;
use du_analysis_index::visualization::projection::Location;
use du_engine_api::viz_pages::Error as PageError;

pub(super) fn publish_hierarchy(
    sink: &IngestSink,
    revision: Id,
) -> Result<Arc<ResultVersion>, String> {
    let lineage = sink
        .bind_current_analysis("scope", id(1))
        .map_err(|error| format!("{error:?}"))?;
    let root = ResultScopeRoot {
        path: ResultNativePath {
            encoding: PathEncoding::Unix,
            raw: PathBytes::from_bytes(b"/fixture"),
        },
        volume_id: vec![1],
        file_id: vec![2],
    };
    let mut scan = sink
        .open_forest_in_lineage(id(2), vec![root], lineage)
        .map_err(|error| format!("{error:?}"))?;
    for path in [
        vec![],
        vec![b"focus".to_vec()],
        vec![b"outside".to_vec()],
        vec![b"third".to_vec()],
        vec![b"fourth".to_vec()],
        vec![b"focus".to_vec(), b"deeper".to_vec()],
    ] {
        scan.add_directory(path)
            .map_err(|error| format!("{error:?}"))?;
    }
    for (path, bytes) in [
        (vec!["focus", "same"], 25_u64),
        (vec!["focus", "zero"], 0),
        (vec!["focus", "deeper", "nested"], 5),
        (vec!["outside", "same"], 75),
        (vec!["third", "file"], 15),
        (vec!["fourth", "file"], 10),
    ] {
        scan.add_file(Observation {
            path: path
                .into_iter()
                .map(|name| name.as_bytes().to_vec())
                .collect(),
            size: Some(SizeValue {
                basis: SizeBasis::Logical,
                evidence: SizeEvidence::Measured,
                amount: SizeAmount::Exact {
                    bytes: bytes.into(),
                },
            }),
        })
        .map_err(|error| format!("{error:?}"))?;
    }
    scan.seal(revision, ResultCompleteness::Complete)
        .map_err(|error| format!("{error:?}"))
}

#[test]
fn locates_exact_items_and_separates_visibility_scope_and_lifetime() -> Result<(), String> {
    let sink = IngestSink::default();
    let version = publish_hierarchy(&sink, id(3))?;
    let item = |name: &[u8], parent| {
        version
            .items()
            .iter()
            .find(|item| item.name == name && item.parent == Some(parent))
            .map(|item| ItemRef {
                result_version: version.version_id().clone(),
                item_no: item.item_no,
            })
            .ok_or_else(|| format!("missing fixture name {name:?}"))
    };
    let focus = item(b"focus", 0)?;
    let outside = item(b"outside", 0)?;
    let deeper = item(b"deeper", focus.item_no)?;
    let drawn = item(b"same", focus.item_no)?;
    let zero = item(b"zero", focus.item_no)?;
    let nested = item(b"nested", deeper.item_no)?;
    let same_name = item(b"same", outside.item_no)?;
    let session = Session::new(
        sink.clone(),
        NonZeroUsize::MIN,
        Arc::new(Identity(AtomicU8::new(11))),
        bundle::implemented_dispatcher().map_err(|error| format!("{error:?}"))?,
    );
    let render = |width| {
        session
            .render_selected(
                request(&focus, width),
                |_, item, _| Ok(policy(item)),
                &mut || false,
            )
            .map_err(|error| format!("{error:?}"))
    };
    let model = render(1_000_000)?;
    let found = session
        .locate(&model.model_ref, &drawn)
        .map_err(|error| format!("{error:?}"))?;
    let Location::Shape(index) = found else {
        return Err("expected a rendered shape".into());
    };
    let page = session
        .rows(&VizRowsRequest {
            model_ref: model.model_ref.clone(),
            result_revision: drawn.result_version.clone(),
            start: 0,
            count: 10,
        })
        .map_err(|error| format!("{error:?}"))?;
    let row = &page.rows[model.model.shapes[index].row_index];
    assert_eq!(row.item_ref.as_ref(), Some(&drawn));
    assert_eq!(
        session.locate(&model.model_ref, &zero),
        Ok(Location::OutsideView)
    );
    assert_eq!(
        session.locate(&model.model_ref, &nested),
        Ok(Location::OutsideView)
    );
    assert_eq!(
        session.locate(&model.model_ref, &same_name),
        Ok(Location::OutsideScope)
    );
    let unknown = ItemRef {
        item_no: u32::MAX,
        ..drawn.clone()
    };
    assert_eq!(
        session.locate(&model.model_ref, &unknown),
        Err(PageError::ItemUnavailable)
    );
    let stale = ItemRef {
        result_version: id(9),
        ..drawn.clone()
    };
    assert_eq!(
        session.locate(&model.model_ref, &stale),
        Err(PageError::StaleVersion)
    );
    assert_eq!(session.locate("", &drawn), Err(PageError::InvalidRange));

    let next = render(500_000)?;
    assert_eq!(
        session.locate(&model.model_ref, &drawn),
        Err(PageError::ModelExpired)
    );
    publish(&sink, id(2), id(4))?;
    assert_eq!(
        session.locate(&next.model_ref, &drawn),
        Err(PageError::StaleVersion)
    );
    Ok(())
}
