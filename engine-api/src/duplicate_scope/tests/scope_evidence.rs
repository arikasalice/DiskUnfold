use super::*;

#[test]
fn overlapping_roots_and_exclusions_keep_the_admitted_scope_evidence() -> Result<(), String> {
    let (source, version) = fixture()?;
    let root = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let ab = reference(&version, b"ab")?;
    let excluded = reference(&version, b"a")?;
    let roots = [root, ab];
    let exclusions = [excluded];
    let selection = select_scope(
        &source,
        Scope {
            version: &id(2),
            roots: &roots,
            exclusions: &exclusions,
            min_bytes: 4,
        },
        || false,
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(selection.scope_evidence().roots, roots);
    assert_eq!(selection.scope_evidence().exclusions, exclusions);
    assert_eq!(selection.files().len(), 1);
    assert_eq!(selection.scope_evidence().counts.excluded, 1);
    assert_eq!(selection.scope_evidence().counts.below_minimum, 1);
    Ok(())
}

#[test]
fn whole_scan_binds_root_and_rejects_foreign_exclusion() -> Result<(), String> {
    let (source, _) = fixture()?;
    let selection =
        select_whole_scan(&source, &id(2), 4, || false).map_err(|e| format!("{e:?}"))?;
    assert_eq!(
        selection.scope_evidence().roots,
        vec![ItemRef {
            result_version: id(2),
            item_no: 0
        }]
    );
    assert!(selection.scope_evidence().exclusions.is_empty());
    assert_eq!(selection.scope_evidence().minimum_bytes, 4);
    let foreign = ItemRef {
        result_version: id(3),
        item_no: 0,
    };
    assert!(matches!(
        select(
            &source,
            Scope {
                version: &id(2),
                roots: &selection.scope_evidence().roots,
                exclusions: &[foreign],
                min_bytes: 4
            }
        ),
        Err(Error::InvalidScope)
    ));
    Ok(())
}
