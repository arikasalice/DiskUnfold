use super::*;
use du_core_contracts::{
    PathBytes,
    analysis_ipc::ScopeSource,
    volume::{ConfirmedFirmlink, RootVolumeGroup, RootVolumeGroupObservation},
};

fn key(encoding: PathEncoding, raw: &[u8]) -> OsPath {
    OsPath {
        encoding,
        raw: PathBytes::from_bytes(raw),
        display: "ignored display".into(),
        lossy: false,
    }
}

fn draft(path: OsPath) -> ScopeDraft {
    ScopeDraft {
        includes: vec![path],
        exclusions: vec![],
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: vec![],
        source: ScopeSource::Gui,
    }
}

#[test]
fn matches_the_published_scope_and_review_byte_layout() -> Result<(), EngineError> {
    let root = key(PathEncoding::WindowsUtf16le, b"C\0:\0\\\0");
    let mut expected = b"diskunfold.scope.v1\0\0".to_vec();
    expected.extend(1u64.to_le_bytes());
    expected.push(1);
    expected.extend(6u64.to_le_bytes());
    expected.extend(b"C\0:\0\\\0");
    expected.extend([0; 16]);
    let scope = scope(&draft(root.clone()))?;
    assert_eq!(scope, blake3::hash(&expected));

    let mut expected = b"diskunfold.scope-review.v1\0".to_vec();
    expected.extend(scope.as_bytes());
    expected.extend(1u64.to_le_bytes());
    expected.push(1);
    expected.extend(6u64.to_le_bytes());
    expected.extend(b"C\0:\0\\\0");
    expected.push(1);
    expected.extend(2u64.to_le_bytes());
    expected.extend(b"vo");
    expected.extend(1u64.to_le_bytes());
    expected.push(0xff);
    assert_eq!(
        review(
            &scope,
            &[ReviewRoot {
                path: &root,
                identity: Some((b"vo", &[0xff])),
            }],
        )?,
        blake3::hash(&expected)
    );
    Ok(())
}

#[test]
fn order_and_display_are_irrelevant_but_scope_conditions_are_bound() -> Result<(), EngineError> {
    let root = key(PathEncoding::WindowsUtf16le, b"C\0:\0\\\0");
    let second = key(PathEncoding::Unix, b"/other/\xff");
    let mut first = draft(root.clone());
    first.includes.push(second.clone());
    first.boundary = ScopeBoundary::IncludeSelected;
    first.included_mounts = vec![root, second.clone()];
    first.exclusions = vec![
        ScopeExclusion::NamePattern {
            pattern: "*.tmp".into(),
        },
        ScopeExclusion::Path { path: second },
    ];
    let baseline = scope(&first)?;
    let mut reordered = first;
    reordered.includes.reverse();
    reordered.exclusions.reverse();
    reordered.included_mounts.reverse();
    reordered.source = ScopeSource::Catalog;
    reordered.includes[0].display = "new label".into();
    reordered.includes[0].lossy = true;
    assert_eq!(baseline, scope(&reordered)?);

    for altered in [
        ScopeDraft {
            boundary: ScopeBoundary::CurrentVolume,
            ..reordered.clone()
        },
        ScopeDraft {
            exclusions: vec![],
            ..reordered.clone()
        },
        ScopeDraft {
            included_mounts: vec![],
            ..reordered.clone()
        },
        ScopeDraft {
            includes: vec![],
            ..reordered
        },
    ] {
        assert_ne!(baseline, scope(&altered)?);
    }
    Ok(())
}

#[test]
fn review_binds_identity_and_access_of_every_original_root() -> Result<(), EngineError> {
    let root = key(PathEncoding::Unix, b"/a");
    let absorbed = key(PathEncoding::Unix, b"/a/sub");
    let scope = scope(&draft(root.clone()))?;
    let main = || ReviewRoot {
        path: &root,
        identity: Some((b"vol", b"root")),
    };
    let child = |identity| ReviewRoot {
        path: &absorbed,
        identity,
    };
    let baseline = review(&scope, &[main(), child(Some((b"vol", b"child")))])?;
    assert_eq!(
        baseline,
        review(&scope, &[child(Some((b"vol", b"child"))), main()])?
    );
    assert_ne!(baseline, review(&scope, &[main()])?);
    assert_ne!(baseline, review(&scope, &[main(), child(None)])?);
    assert_ne!(
        baseline,
        review(&scope, &[main(), child(Some((b"vol", b"replacement")))])?
    );
    assert_ne!(
        baseline,
        review(&scope, &[main(), child(Some((b"new", b"child")))])?
    );
    Ok(())
}

#[test]
fn root_volume_group_has_a_fixed_sorted_fingerprint_vector() -> Result<(), EngineError> {
    let scope_draft = draft(key(PathEncoding::Unix, b"/"));
    let group = RootVolumeGroup {
        data_mount: b"/D".to_vec(),
        device: 7,
        data_inode: 9,
        firmlinks: vec![
            ConfirmedFirmlink {
                source: b"/Z".to_vec(),
                target: b"/D/Z".to_vec(),
                device: 7,
                inode: 12,
            },
            ConfirmedFirmlink {
                source: b"/A".to_vec(),
                target: b"/D/A".to_vec(),
                device: 7,
                inode: 11,
            },
        ],
    };
    let mut expected = b"diskunfold.scope.v1\0".to_vec();
    expected.push(2);
    expected.extend(1u64.to_le_bytes());
    expected.push(0);
    expected.extend(1u64.to_le_bytes());
    expected.push(b'/');
    expected.extend([0u64.to_le_bytes(), 0u64.to_le_bytes()].concat());
    expected.extend(2u64.to_le_bytes());
    expected.extend(b"/D");
    expected.extend(8u64.to_le_bytes());
    expected.extend(7u64.to_le_bytes());
    expected.extend(8u64.to_le_bytes());
    expected.extend(9u64.to_le_bytes());
    expected.extend(2u64.to_le_bytes());
    for (source, target, inode) in [
        (b"/A".as_slice(), b"/D/A".as_slice(), 11u64),
        (b"/Z", b"/D/Z", 12),
    ] {
        expected.extend((source.len() as u64).to_le_bytes());
        expected.extend(source);
        expected.extend((target.len() as u64).to_le_bytes());
        expected.extend(target);
        expected.extend(8u64.to_le_bytes());
        expected.extend(7u64.to_le_bytes());
        expected.extend(8u64.to_le_bytes());
        expected.extend(inode.to_le_bytes());
    }
    let confirmed = scope_observed(&scope_draft, &RootVolumeGroupObservation::Confirmed(group))?;
    assert_eq!(confirmed, blake3::hash(&expected));
    assert_ne!(
        confirmed,
        scope_observed(&scope_draft, &RootVolumeGroupObservation::Unconfirmed)?
    );
    assert_eq!(
        scope(&scope_draft)?,
        scope_observed(&scope_draft, &RootVolumeGroupObservation::NotApplicable)?
    );
    Ok(())
}
