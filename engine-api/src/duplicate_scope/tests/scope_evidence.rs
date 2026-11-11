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

#[test]
fn selects_only_observed_files_inside_roots_and_outside_exclusions() -> Result<(), String> {
    let (source, version) = fixture()?;
    let root = ItemRef {
        result_version: id(2),
        item_no: 0,
    };
    let excluded = reference(&version, b"a")?;
    let selection = select(
        &source,
        Scope {
            version: &id(2),
            roots: std::slice::from_ref(&root),
            exclusions: std::slice::from_ref(&excluded),
            min_bytes: 4,
        },
    )
    .map_err(|e| format!("{e:?}"))?;
    assert_eq!(selection.version(), &id(2));
    let evidence = selection.scope_evidence();
    assert_eq!(evidence.roots, vec![root]);
    assert_eq!(evidence.exclusions, vec![excluded]);
    assert_eq!(evidence.minimum_bytes, 4);
    assert_eq!(
        (
            evidence.counts.outside,
            evidence.counts.excluded,
            evidence.counts.below_minimum,
            evidence.counts.unavailable
        ),
        (0, 1, 1, 2)
    );
    assert_eq!(evidence.source_completeness, version.completeness());
    assert_eq!(evidence.observed_at.as_ref(), version.observed_at());
    assert_eq!(selection.files.len(), 1);
    assert_eq!(selection.files[0].item, reference(&version, b"inside")?);
    assert_eq!(selection.files[0].size, 8);
    assert_eq!(
        selection.files[0].native_modified,
        std::num::NonZeroU64::new(123_456_789)
            .map(du_core_contracts::scan_observation::NativeModifiedTime::WindowsFiletime)
    );
    assert!(
        selection
            .selected(&reference(&version, b"inside")?)
            .is_some()
    );
    assert!(
        selection
            .selected(&reference(&version, b"excluded")?)
            .is_none()
    );
    assert!(
        selection
            .selected(&ItemRef {
                result_version: id(3),
                item_no: selection.files[0].item.item_no
            })
            .is_none()
    );
    assert_eq!(
        selection.counts(),
        &Counts {
            outside: 0,
            excluded: 1,
            below_minimum: 1,
            unavailable: 2
        }
    );
    Ok(())
}
