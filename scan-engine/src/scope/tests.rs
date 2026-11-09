use super::*;

#[test]
fn known_excluded_component_still_applies_after_an_unrepresentable_name() -> Result<(), ScopeError>
{
    let mut input = path(Some(1), 1, &["opaque", "cache"]);
    input.key[0] = vec![0xff];
    let result = normalize(
        vec![input],
        vec![Exclusion::NamePattern("cache".into())],
        &['/'],
    )?;
    assert!(result.includes.is_empty());
    assert!(matches!(&result.notices[..], [Notice::Excluded(_)]));
    Ok(())
}

#[test]
fn reports_a_selected_mount_missing_from_the_current_observation() {
    let selected = path(Some(1), 1, &["mount"]);
    let result = boundaries(Vec::new(), &[selected], BoundaryMode::IncludeSelected, true);
    assert!(result.uncertain);
    assert!(result.included.is_empty());
}

fn path(volume: Option<u8>, entry: u8, parts: &[&str]) -> Path {
    Path {
        volume: volume.map(|id| vec![id]),
        entry: vec![entry],
        alias_verified: true,
        boundary_verified: true,
        key: parts.iter().map(|part| part.as_bytes().to_vec()).collect(),
        comparison_verified: true,
    }
}

#[test]
fn deterministic_merge_preserves_exclusions_and_respects_components() {
    let parent = path(Some(1), 2, &["data"]);
    let child = path(Some(1), 2, &["data", "tmp"]);
    let sibling = path(Some(1), 2, &["database"]);
    let exclusion = Exclusion::Path(path(Some(1), 2, &["data", "private"]));
    let first = normalize(
        vec![
            child.clone(),
            sibling.clone(),
            parent.clone(),
            parent.clone(),
        ],
        vec![exclusion.clone()],
        &['/'],
    )
    .unwrap_or_else(|e| unreachable!("{e:?}"));
    let second = normalize(
        vec![
            parent.clone(),
            child.clone(),
            parent.clone(),
            sibling.clone(),
        ],
        vec![exclusion.clone()],
        &['/'],
    )
    .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!(first, second);
    assert_eq!(first.includes, vec![parent, sibling]);
    assert_eq!(first.exclusions, vec![exclusion]);
    assert!(first.notices.contains(&Notice::DescendantAbsorbed(child)));
    assert!(!first.notices.contains(&Notice::IneffectiveExclusion(0)));
}

#[test]
fn only_confirmed_identity_alias_and_boundary_merge() {
    let a = path(Some(1), 1, &["a"]);
    let mut alias = path(Some(1), 2, &["a"]);
    let other_volume = path(Some(2), 1, &["a"]);
    let unknown_volume = path(None, 1, &["a"]);
    let mut unknown_boundary = path(Some(1), 1, &["a", "b"]);
    unknown_boundary.boundary_verified = false;
    let mut unverified = path(Some(1), 1, &["a", "c"]);
    unverified.comparison_verified = false;
    let merged = normalize(
        vec![
            a.clone(),
            alias.clone(),
            other_volume.clone(),
            unknown_volume.clone(),
            unknown_boundary.clone(),
            unverified.clone(),
        ],
        vec![],
        &['/'],
    )
    .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert!(merged.notices.contains(&Notice::AliasMerged(alias.clone())));
    for expected in [
        &a,
        &other_volume,
        &unknown_volume,
        &unknown_boundary,
        &unverified,
    ] {
        assert!(merged.includes.contains(expected));
    }
    alias.alias_verified = false;
    let retained =
        normalize(vec![a, alias.clone()], vec![], &['/']).unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!(retained.includes.len(), 2);
}

#[test]
fn exclusions_validate_and_keep_uncertain_relationships() {
    assert_eq!(normalize(vec![], vec![], &['/']), Err(ScopeError::Empty));
    let include = path(Some(1), 1, &["data", "cache"]);
    let invalid = normalize(
        vec![include.clone()],
        vec![Exclusion::NamePattern("a/b".into())],
        &['/'],
    );
    assert_eq!(invalid, Err(ScopeError::InvalidExclusion { index: 0 }));
    let invalid = normalize(
        vec![include.clone()],
        vec![Exclusion::NamePattern("".into())],
        &['/'],
    );
    assert_eq!(invalid, Err(ScopeError::InvalidExclusion { index: 0 }));
    let exclude = Exclusion::Path(path(Some(1), 1, &["data"]));
    let result = normalize(vec![include.clone()], vec![exclude], &['/'])
        .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert!(result.includes.is_empty());
    assert!(result.notices.contains(&Notice::Excluded(include.clone())));
    let mut uncertain = include.clone();
    uncertain.volume = None;
    let result = normalize(vec![include], vec![Exclusion::Path(uncertain)], &['/'])
        .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert_eq!(result.includes.len(), 1);
    assert!(result.notices.contains(&Notice::ExclusionUncertain(0)));
}

#[test]
fn wildcard_matches_components_and_unicode_scalars() {
    for (pattern, name, expected) in [
        ("*.tmp", "a.tmp", true),
        ("a?c", "a한c", true),
        ("a?c", "ac", false),
        ("*a*b", "xaaab", true),
        ("a*", "a", true),
        ("?", "한글", false),
    ] {
        assert_eq!(wildcard_matches(pattern, name), expected);
    }
    let selected = path(Some(1), 1, &["data", "cache.tmp"]);
    let result = normalize(
        vec![selected],
        vec![Exclusion::NamePattern("*.tmp".into())],
        &['/'],
    )
    .unwrap_or_else(|e| unreachable!("{e:?}"));
    assert!(result.includes.is_empty());
}

#[test]
fn mount_boundary_applies_only_confirmed_selected_points() {
    let one = path(Some(2), 1, &["a"]);
    let two = path(Some(3), 1, &["b"]);
    let unknown = path(None, 1, &["c"]);
    let observations = vec![
        ChildMount {
            path: two.clone(),
            different_volume_confirmed: true,
        },
        ChildMount {
            path: unknown,
            different_volume_confirmed: false,
        },
        ChildMount {
            path: one.clone(),
            different_volume_confirmed: true,
        },
    ];
    let selected = boundaries(
        observations.clone(),
        std::slice::from_ref(&one),
        BoundaryMode::IncludeSelected,
        true,
    );
    assert_eq!(selected.included, vec![one.clone()]);
    assert_eq!(selected.excluded, vec![two.clone()]);
    assert!(selected.uncertain);
    let all = boundaries(
        observations,
        std::slice::from_ref(&one),
        BoundaryMode::CurrentVolume,
        false,
    );
    assert_eq!(all.excluded, vec![one, two]);
    assert!(all.included.is_empty());
    assert!(all.uncertain);
    let _ = Notice::BoundaryUncertain;
}
