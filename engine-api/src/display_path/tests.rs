use super::*;
use du_core_contracts::{
    PathBytes,
    analysis_ipc::{
        RecentScopeOpenTarget, RecentScopeTargetState, ScopeBoundary, ScopeCorrespondence,
        ScopeSource, TargetAccess,
    },
};

#[test]
fn extended_drive_and_unc_paths_lose_only_the_prefix() {
    for (input, expected) in [
        (r"\\?\C:\Users\x", r"C:\Users\x"),
        (r"\\?\C:\", r"C:\"),
        (r"\\?\z:\lower", r"z:\lower"),
        (r"\\?\UNC\server\share\x", r"\\server\share\x"),
        (r"\\?\UNC\server\share", r"\\server\share"),
        (r"\\?\unc\server\share\x", r"\\server\share\x"),
        (r"\\?\Unc\server\share", r"\\server\share"),
    ] {
        assert_eq!(windows(input), expected, "{input}");
    }
}

#[test]
fn forms_without_a_drive_or_unc_equivalent_are_unchanged() {
    for input in [
        r"\\?\Volume{01234567-89ab-cdef-0123-456789abcdef}\Folder",
        r"\\?\GLOBALROOT\Device\HarddiskVolume1\x",
        r"\\.\C:\x",
        r"\\.\PhysicalDrive0",
        r"\??\C:\x",
        r"\\?\C:",
        r"\\?\C:relative",
        r"\\?\1:\x",
        r"\\?\UNC\server",
        r"\\?\UNC\\share",
        r"\\?\UNC\..\share",
        r"\\?\UNC/server/share",
        r"//?/C:/x",
        r"\\?/C:\x",
        r"\\?\",
        r"C:\already\plain",
        r"\\server\share\x",
        "",
        "/home/user",
        "/",
    ] {
        assert_eq!(windows(input), input, "{input}");
    }
}

fn wide(text: &str) -> Vec<u8> {
    text.encode_utf16().flat_map(u16::to_le_bytes).collect()
}

#[test]
fn decoded_paths_apply_rules_only_to_windows_encoding() {
    assert_eq!(
        path(&PathEncoding::WindowsUtf16le, &wide(r"\\?\C:\x")).as_deref(),
        Some(r"C:\x")
    );
    assert_eq!(
        path(&PathEncoding::Unix, br"\\?\C:\x").as_deref(),
        Some(r"\\?\C:\x")
    );
    assert_eq!(
        path(&PathEncoding::Unix, b"/data/\xff").as_deref(),
        Some("/data/\u{fffd}")
    );
    assert_eq!(path(&PathEncoding::WindowsUtf16le, &[0x41]), None);
    assert_eq!(
        path(&PathEncoding::WindowsUtf16le, &[]).as_deref(),
        Some("")
    );
}

#[test]
fn unpaired_surrogates_stay_lossy_after_the_prefix_is_removed() {
    let mut raw = wide(r"\\?\C:\bad");
    raw.extend_from_slice(&0xd800_u16.to_le_bytes());
    assert_eq!(
        path(&PathEncoding::WindowsUtf16le, &raw).as_deref(),
        Some("C:\\bad\u{fffd}")
    );
    let projected = os_path(OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&raw),
        display: String::from("\\\\?\\C:\\bad\u{fffd}"),
        lossy: true,
    });
    assert_eq!(projected.display, "C:\\bad\u{fffd}");
    assert!(projected.lossy);
    assert_eq!(projected.raw, PathBytes::from_bytes(&raw));
}

fn windows_path(text: &str) -> OsPath {
    OsPath {
        encoding: PathEncoding::WindowsUtf16le,
        raw: PathBytes::from_bytes(&wide(text)),
        display: text.into(),
        lossy: false,
    }
}

fn unix_path(text: &str) -> OsPath {
    OsPath {
        encoding: PathEncoding::Unix,
        raw: PathBytes::from_bytes(text.as_bytes()),
        display: text.into(),
        lossy: false,
    }
}

fn snapshot(path: &OsPath) -> (PathEncoding, PathBytes, bool) {
    (path.encoding.clone(), path.raw.clone(), path.lossy)
}

fn draft() -> ScopeDraft {
    ScopeDraft {
        includes: vec![windows_path(r"\\?\C:\Fixture"), unix_path(r"/\\?\C:\x")],
        exclusions: vec![
            ScopeExclusion::Path {
                path: windows_path(r"\\?\UNC\server\share\skip"),
            },
            ScopeExclusion::NamePattern {
                pattern: r"\\?\C:\*".into(),
            },
        ],
        boundary: ScopeBoundary::CurrentVolume,
        included_mounts: vec![windows_path(r"\\?\D:\")],
        source: ScopeSource::Gui,
    }
}

#[test]
fn review_projection_changes_display_text_only() -> Result<(), String> {
    let original = ScopeReview {
        scope: draft(),
        notices: vec![],
        targets: vec![ScopeCorrespondence {
            path: windows_path(r"\\?\C:\Fixture"),
            access: TargetAccess::Accessible,
            reason_key: None,
        }],
        review_fingerprint: "review".into(),
        observed_at: du_core_contracts::Timestamp::parse("2020-01-02T03:04:05.006Z")?,
        request_seq: 7,
        volume_group: None,
    };
    let projected = scope_review(original.clone());
    let shown: Vec<_> = projected
        .scope
        .includes
        .iter()
        .chain(&projected.scope.included_mounts)
        .chain(projected.targets.iter().map(|target| &target.path))
        .map(|path| path.display.as_str())
        .collect();
    assert_eq!(shown, [r"C:\Fixture", r"/\\?\C:\x", r"D:\", r"C:\Fixture"]);
    let ScopeExclusion::Path { path } = &projected.scope.exclusions[0] else {
        return Err("missing path rule".into());
    };
    assert_eq!(path.display, r"\\server\share\skip");
    assert_eq!(projected.scope.exclusions[1], original.scope.exclusions[1]);
    // Raw bytes and encodings, which drive comparison and hashing, are unchanged.
    assert_eq!(projected.scope, original.scope);
    for (before, after) in original.targets.iter().zip(&projected.targets) {
        assert_eq!(snapshot(&before.path), snapshot(&after.path));
    }
    assert_eq!(projected.review_fingerprint, original.review_fingerprint);
    assert_eq!(
        crate::scope::recent_scope_fingerprint_v1(&projected.scope).ok(),
        crate::scope::recent_scope_fingerprint_v1(&original.scope).ok()
    );
    Ok(())
}

#[test]
fn recent_open_projection_covers_draft_targets_proposals_and_review() {
    let open = recent_scope_open(RecentScopeOpen {
        draft: draft(),
        targets: vec![RecentScopeOpenTarget {
            path: windows_path(r"\\?\C:\Fixture"),
            state: RecentScopeTargetState::Moved,
            proposed_entry: Some(windows_path(r"\\?\E:\")),
        }],
        review: None,
    });
    assert_eq!(open.draft.includes[0].display, r"C:\Fixture");
    assert_eq!(open.targets[0].path.display, r"C:\Fixture");
    assert_eq!(
        open.targets[0]
            .proposed_entry
            .as_ref()
            .map(|entry| entry.display.as_str()),
        Some(r"E:\")
    );
    assert_eq!(open.targets[0].path, windows_path(r"\\?\C:\Fixture"));
}

#[cfg(windows)]
mod os;
mod rules;
