use super::*;

/// The shape of the ui-smoke treemap fixture (#242), scaled to this file's fixed
/// axis of 50: two large folders, a small folder and a file. With two bars the
/// small folder and the file form Other.
#[test]
fn other_with_a_small_folder_and_a_file_is_drawn_from_the_partition() -> Result<(), String> {
    let sink = IngestSink::default();
    let mut build = sink.open(id(1)).map_err(|e| format!("{e:?}"))?;
    build.add_directory(vec![]).map_err(|e| format!("{e:?}"))?;
    let file = |path: Vec<Vec<u8>>, bytes: u64| Observation {
        path,
        size: Some(SizeValue {
            basis: SizeBasis::Logical,
            evidence: SizeEvidence::Measured,
            amount: SizeAmount::Exact {
                bytes: bytes.into(),
            },
        }),
    };
    for (folder, files) in [
        (b"Videos".to_vec(), vec![(b"clip.bin".to_vec(), 40_u64)]),
        (
            b"Photos".to_vec(),
            vec![(b"album-a.bin".to_vec(), 20), (b"album-b.bin".to_vec(), 15)],
        ),
        (b"Tiny".to_vec(), vec![(b"tiny.bin".to_vec(), 1)]),
    ] {
        build
            .add_directory(vec![folder.clone()])
            .map_err(|e| format!("{e:?}"))?;
        for (name, bytes) in files {
            build
                .add_file(file(vec![folder.clone(), name], bytes))
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    build
        .add_file(file(vec![b"notes.bin".to_vec()], 6))
        .map_err(|e| format!("{e:?}"))?;
    let version = build
        .seal(id(2), ResultCompleteness::Complete)
        .map_err(|e| format!("{e:?}"))?;
    let selected: Vec<Row> = version
        .items()
        .iter()
        .filter(|item| item.parent == Some(0))
        .map(|item| {
            Ok(Row {
                item: reference(&version, item.item_no),
                size: version
                    .space(item.item_no)
                    .ok_or("missing row")?
                    .logical
                    .clone(),
            })
        })
        .collect::<Result<_, &str>>()?;
    let scope = reference(&version, 0);
    let aggregate = aggregate(&version)?;
    let result = bar_projection::project(
        request(&version, &selected, &scope, &aggregate, 2, 3),
        &mut || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(result.bars.len(), 3);
    assert!(matches!(
        &result.bars[2].identity,
        BarIdentity::Other { members, size }
            if members.len() == 2 && size.amount == SizeAmount::Exact { bytes: 7.into() }
    ));
    Ok(())
}
