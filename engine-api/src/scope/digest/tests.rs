use super::*;
use du_core_contracts::{PathBytes, analysis_ipc::ScopeSource};

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
