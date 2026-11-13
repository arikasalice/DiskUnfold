use super::*;
use serde_json::{Value, json};

fn path(encoding: PathEncoding, raw: &[u8]) -> OsPath {
    OsPath {
        encoding,
        raw: PathBytes::from_bytes(raw),
        display: "label".into(),
        lossy: true,
    }
}

fn win(text: &str) -> OsPath {
    let raw: Vec<_> = text.encode_utf16().flat_map(u16::to_le_bytes).collect();
    path(PathEncoding::WindowsUtf16le, &raw)
}

fn unix(raw: &[u8]) -> OsPath {
    path(PathEncoding::Unix, raw)
}

fn sample() -> Result<(BodyV1, [u8; 32]), BodyError> {
    let windows = win(r"\\?\C:\Folder");
    let unix_root = unix(b"/mount/\xff");
    let applied = ScopeDraft {
        includes: vec![windows.clone(), unix_root.clone()],
        exclusions: vec![
            ScopeExclusion::Path {
                path: win(r"C:\Folder\Private"),
            },
            ScopeExclusion::NamePattern {
                pattern: "*.tmp".into(),
            },
        ],
        boundary: ScopeBoundary::IncludeSelected,
        included_mounts: vec![unix(b"/mount/Sub"), unix(b"/mount/Sub")],
        source: ScopeSource::Gui,
    };
    let mut hashed = applied.clone();
    hashed.includes[0] = path(PathEncoding::WindowsUtf16le, b"opaque key\0\xff");
    hashed.exclusions[0] = ScopeExclusion::Path {
        path: win(r"C:\FOLDER\PRIVATE"),
    };
    hashed.included_mounts.fill(unix(b"mount-key"));
    let fingerprint = recent_scope_fingerprint_v1(&hashed).map_err(|_| BodyError::Malformed)?;
    let body = BodyV1::from_conditions(
        &applied,
        &hashed,
        vec![
            RecentTarget {
                applied_path: windows,
                volume_id: PathBytes::from_bytes(&[0, 0xff]),
                entry_path: win(r"\\?\C:\"),
                relative_path: win(r"\Folder"),
            },
            RecentTarget {
                applied_path: unix_root.clone(),
                volume_id: PathBytes::from_bytes(&[1]),
                entry_path: unix_root,
                relative_path: unix(b""),
            },
        ],
    )?;
    Ok((body, fingerprint))
}

#[test]
fn mixed_native_paths_and_opaque_keys_round_trip_without_substitution() -> Result<(), BodyError> {
    let (body, fingerprint) = sample()?;
    let json = serde_json::to_string(&body).map_err(|_| BodyError::Malformed)?;
    let decoded = RecentScopeBody::decode(&json, 1, &fingerprint, 2)?;
    let draft = decoded.scope_draft();
    assert_eq!(draft.source, ScopeSource::Recent);
    assert_eq!(
        draft.includes,
        [win(r"\\?\C:\Folder"), unix(b"/mount/\xff")]
    );
    assert_eq!(
        draft.included_mounts,
        [unix(b"/mount/Sub"), unix(b"/mount/Sub")]
    );
    assert_eq!(
        draft.exclusions[0],
        ScopeExclusion::Path {
            path: win(r"C:\Folder\Private")
        }
    );
    assert_eq!(
        decoded.targets()[1].relative_path.raw,
        PathBytes::from_bytes(b"")
    );
    assert!(decoded.targets()[1].entry_path.lossy);
    assert_eq!(decoded.targets()[1].entry_path.display, "label");
    assert_ne!(recent_scope_fingerprint_v1(&draft).ok(), Some(fingerprint));
    Ok(())
}

#[test]
fn rejects_unsupported_versions_and_conflicting_row_metadata() -> Result<(), BodyError> {
    let (body, fingerprint) = sample()?;
    let json = serde_json::to_string(&body).map_err(|_| BodyError::Malformed)?;
    assert!(matches!(
        RecentScopeBody::decode(&json, 2, &fingerprint, 2),
        Err(BodyError::UnsupportedVersion)
    ));
    assert!(matches!(
        RecentScopeBody::decode(
            &json.replace("\"bodyVersion\":1", "\"bodyVersion\":2"),
            1,
            &fingerprint,
            2
        ),
        Err(BodyError::UnsupportedVersion)
    ));
    assert!(matches!(
        RecentScopeBody::decode(&json, 1, &fingerprint, 1),
        Err(BodyError::Malformed)
    ));
    assert!(matches!(
        RecentScopeBody::decode(&json, 1, &[0; 32], 2),
        Err(BodyError::FingerprintMismatch)
    ));
    Ok(())
}

#[test]
fn rejects_unknown_duplicate_missing_and_malformed_fields() -> Result<(), BodyError> {
    let (body, fingerprint) = sample()?;
    let valid = serde_json::to_string(&body).map_err(|_| BodyError::Malformed)?;
    for bad in [
        valid.replace("\"bodyVersion\":1,", ""),
        valid.replace("\"bodyVersion\":1", "\"bodyVersion\":null"),
        valid.replace("\"bodyVersion\":1", "\"bodyVersion\":1,\"bodyVersion\":1"),
        valid.replace(
            "\"display\":\"label\"",
            "\"display\":\"label\",\"display\":\"label\"",
        ),
        valid.replace("\"lossy\":true", "\"lossy\":true,\"extra\":false"),
        valid.replace("\"kind\":\"path\"", "\"kind\":\"unregistered\""),
        valid.replace("\"volumeId\":\"AP8\"", "\"volumeId\":\"AP8=\""),
    ] {
        assert_ne!(bad, valid);
        assert!(matches!(
            RecentScopeBody::decode(&bad, 1, &fingerprint, 2),
            Err(BodyError::Malformed)
        ));
    }
    Ok(())
}

#[test]
fn rejects_target_count_and_native_path_corruption() -> Result<(), BodyError> {
    let (body, fingerprint) = sample()?;
    let valid = serde_json::to_value(&body).map_err(|_| BodyError::Malformed)?;
    let reject = |bad: &Value| {
        assert!(matches!(
            RecentScopeBody::decode(&bad.to_string(), 1, &fingerprint, 2),
            Err(BodyError::Malformed)
        ));
    };
    let mut bad = valid.clone();
    bad["normalizedScope"]["includes"] = json!([]);
    reject(&bad);
    let mut bad = valid.clone();
    bad["targets"][0]["volumeId"] = json!("");
    reject(&bad);
    let mut bad = valid.clone();
    bad["targets"][0]["relativePath"]["raw"] = json!(win(r"\..\escape").raw);
    reject(&bad);
    let mut bad = valid;
    bad["targets"][0]["entryPath"]["raw"] = json!(win(r"C:relative").raw);
    reject(&bad);
    Ok(())
}
