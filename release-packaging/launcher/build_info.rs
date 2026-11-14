// Assembly of the build-info JSON embedded into the launcher (F15 FR5.HD1).
//
// build.rs includes this file as a module; the "build_info" test target in
// Cargo.toml compiles it on its own to run the tests below.
#![allow(dead_code)]

use serde_json::{Value, json};

/// Raw inputs gathered by the build script from the environment and git.
#[derive(Clone, Debug, Default)]
pub struct BuildInputs {
    /// Product version from Cargo.
    pub version: Option<String>,
    /// Target operating system from Cargo.
    pub target_os: Option<String>,
    /// Target CPU architecture from Cargo.
    pub target_cpu: Option<String>,
    /// Requested build kind (official or developer); unset means developer.
    pub requested_kind: Option<String>,
    /// Source commit supplied by the release pipeline.
    pub supplied_commit: Option<String>,
    /// Commit found with git rev-parse HEAD at build time.
    pub detected_commit: Option<String>,
    /// Whether the working tree differs from the detected commit.
    pub detected_dirty: bool,
    /// Release tag the public CI builds from.
    pub release_tag: Option<String>,
    /// Build time in UTC supplied by the release pipeline.
    pub built_at_utc: Option<String>,
    /// Application database format version supplied by the release pipeline.
    pub app_db_format: Option<String>,
    /// Snapshot file format version supplied by the release pipeline.
    pub snapshot_format: Option<String>,
    /// Repository LICENSE text.
    pub license: String,
    /// Parsed release resources produced by build-product-resources.
    pub resources: Option<Value>,
}

fn present(value: &Option<String>) -> Option<&str> {
    value.as_deref().map(str::trim).filter(|v| !v.is_empty())
}

/// Returns true for a lower-case 40 digit hexadecimal commit id.
pub fn is_full_commit(value: &str) -> bool {
    value.len() == 40
        && value
            .bytes()
            .all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

/// Returns true for YYYY-MM-DDTHH:MM:SSZ.
pub fn is_utc_timestamp(value: &str) -> bool {
    let b = value.as_bytes();
    b.len() == 20
        && b[19] == b'Z'
        && b[4] == b'-'
        && b[7] == b'-'
        && b[10] == b'T'
        && b[13] == b':'
        && b[16] == b':'
        && [0, 1, 2, 3, 5, 6, 8, 9, 11, 12, 14, 15, 17, 18]
            .iter()
            .all(|&i| b[i].is_ascii_digit())
}

fn format_number(name: &str, value: &Option<String>) -> Result<Option<u32>, String> {
    match present(value) {
        None => Ok(None),
        Some(text) => text
            .parse::<u32>()
            .map(Some)
            .map_err(|_| format!("{name} must be an unsigned integer, got {text:?}")),
    }
}

/// Values read from the release resources file.
struct ReleaseResources<'a> {
    source_commit: Option<&'a str>,
    gate_passed: bool,
    third_party: Option<&'a str>,
    matrix: Option<&'a Value>,
}

impl<'a> ReleaseResources<'a> {
    fn read(resources: Option<&'a Value>) -> Self {
        let text = |pointer: &str| resources.and_then(|r| r.pointer(pointer)?.as_str());
        Self {
            source_commit: text("/sourceCommit"),
            // Only a release-mode run whose gate had no blockers may mark a build official.
            gate_passed: resources.and_then(|r| r.pointer("/releaseGate/passed")?.as_bool())
                == Some(true)
                && text("/releaseGate/mode") == Some("release"),
            third_party: text("/thirdPartyNotices").filter(|t| !t.trim().is_empty()),
            matrix: resources
                .and_then(|r| r.get("supportMatrix"))
                .filter(|m| !m.is_null()),
        }
    }

    fn matrix_revision(&self) -> Option<&'a str> {
        self.matrix?.get("revision")?.as_str()
    }
}

fn validate_inputs(inputs: &BuildInputs) -> Result<(), String> {
    if let Some(time) = present(&inputs.built_at_utc)
        && !is_utc_timestamp(time)
    {
        return Err(format!(
            "DU_BUILT_AT_UTC must be YYYY-MM-DDTHH:MM:SSZ, got {time:?}"
        ));
    }
    if let Some(supplied) = present(&inputs.supplied_commit)
        && !is_full_commit(supplied)
    {
        return Err("DU_SOURCE_COMMIT must be a 40 digit lower-case hex commit".to_owned());
    }
    Ok(())
}

fn official_commit(
    inputs: &BuildInputs,
    resources: &ReleaseResources<'_>,
) -> Result<String, String> {
    let commit = present(&inputs.supplied_commit)
        .ok_or("an official build needs DU_SOURCE_COMMIT")?
        .to_owned();
    present(&inputs.release_tag).ok_or("an official build needs DU_RELEASE_TAG")?;
    present(&inputs.built_at_utc).ok_or("an official build needs DU_BUILT_AT_UTC")?;
    present(&inputs.app_db_format).ok_or("an official build needs DU_APP_DB_FORMAT")?;
    present(&inputs.snapshot_format).ok_or("an official build needs DU_SNAPSHOT_FORMAT")?;
    if !resources.gate_passed
        || resources.third_party.is_none()
        || resources.matrix_revision().is_none()
    {
        return Err(
            "an official build needs release-mode resources whose release gate passed".to_owned(),
        );
    }
    if resources.source_commit != Some(commit.as_str()) {
        return Err("release resources were generated for a different commit".to_owned());
    }
    Ok(commit)
}

fn developer_commit(inputs: &BuildInputs) -> Option<String> {
    if let Some(supplied) = present(&inputs.supplied_commit) {
        return Some(supplied.to_owned());
    }
    present(&inputs.detected_commit).map(|sha| {
        if inputs.detected_dirty {
            format!("{sha}-dirty")
        } else {
            sha.to_owned()
        }
    })
}

/// Builds the embedded product-info document.
///
/// Developer builds record only values the build can observe. The official
/// marker needs a release tag, a full commit, a UTC build time, both data
/// format versions and release-mode resources generated for that commit
/// whose gate passed.
///
/// # Errors
/// Returns a message when an official build is requested without every
/// required input, or when an input is malformed.
pub fn assemble(inputs: &BuildInputs) -> Result<Value, String> {
    let official = match present(&inputs.requested_kind) {
        None | Some("developer") => false,
        Some("official") => true,
        Some(other) => return Err(format!("unknown build kind {other:?}")),
    };
    validate_inputs(inputs)?;
    let app_db_format = format_number("DU_APP_DB_FORMAT", &inputs.app_db_format)?;
    let snapshot_format = format_number("DU_SNAPSHOT_FORMAT", &inputs.snapshot_format)?;
    let resources = ReleaseResources::read(inputs.resources.as_ref());
    let commit = if official {
        Some(official_commit(inputs, &resources)?)
    } else {
        developer_commit(inputs)
    };

    Ok(json!({
        "build": {
            "productVersion": inputs.version,
            "buildKind": if official { "official" } else { "developer" },
            "commit": commit,
            "builtAtUtc": present(&inputs.built_at_utc),
            "targetOs": inputs.target_os,
            "targetCpu": inputs.target_cpu,
            "appDbFormat": app_db_format,
            "snapshotFormat": snapshot_format,
            "supportMatrixRevision": resources.matrix_revision()
        },
        "notices": {
            "license": inputs.license,
            "thirdParty": resources.third_party,
            "sourceCommit": resources.source_commit
        },
        "supportMatrix": resources.matrix.cloned().unwrap_or(Value::Null)
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    const COMMIT: &str = "0123456789abcdef0123456789abcdef01234567";

    fn resources(commit: &str, passed: bool, mode: &str) -> Value {
        json!({
            "sourceCommit": commit,
            "releaseGate": {"passed": passed, "mode": mode, "blockers": []},
            "thirdPartyNotices": "THIRD-PARTY-NOTICES\n",
            "supportMatrix": {"revision": "2026-11-13.0123456789ab", "asOf": "2026-11-13", "combinations": []}
        })
    }

    fn official_inputs() -> BuildInputs {
        BuildInputs {
            version: Some("0.0.1".into()),
            requested_kind: Some("official".into()),
            supplied_commit: Some(COMMIT.into()),
            release_tag: Some("v0.0.1".into()),
            built_at_utc: Some("2026-11-13T01:02:03Z".into()),
            app_db_format: Some("3".into()),
            snapshot_format: Some("1".into()),
            license: "MIT".into(),
            resources: Some(resources(COMMIT, true, "release")),
            ..BuildInputs::default()
        }
    }

    #[test]
    fn developer_build_stays_developer_and_marks_dirty_trees() -> Result<(), String> {
        let info = assemble(&BuildInputs {
            detected_commit: Some(COMMIT.into()),
            detected_dirty: true,
            license: "MIT".into(),
            ..BuildInputs::default()
        })?;
        assert_eq!(info["build"]["buildKind"], "developer");
        assert_eq!(info["build"]["commit"], format!("{COMMIT}-dirty"));
        for key in [
            "builtAtUtc",
            "appDbFormat",
            "snapshotFormat",
            "supportMatrixRevision",
        ] {
            assert!(info["build"][key].is_null(), "{key} must stay unavailable");
        }
        assert!(info["notices"]["thirdParty"].is_null());
        assert!(info["supportMatrix"].is_null());
        Ok(())
    }

    #[test]
    fn official_build_embeds_release_values() -> Result<(), String> {
        let info = assemble(&official_inputs())?;
        assert_eq!(info["build"]["buildKind"], "official");
        assert_eq!(info["build"]["commit"], COMMIT);
        assert_eq!(info["build"]["appDbFormat"], 3);
        assert_eq!(
            info["build"]["supportMatrixRevision"],
            "2026-11-13.0123456789ab"
        );
        assert_eq!(info["notices"]["sourceCommit"], COMMIT);
        assert_eq!(info["notices"]["thirdParty"], "THIRD-PARTY-NOTICES\n");
        Ok(())
    }

    #[test]
    fn official_build_requires_every_release_input() {
        type Break = fn(&mut BuildInputs);
        let cases: [(&str, Break); 9] = [
            ("tag", |i| i.release_tag = None),
            ("commit", |i| i.supplied_commit = None),
            ("time", |i| i.built_at_utc = None),
            ("db format", |i| i.app_db_format = None),
            ("snapshot format", |i| i.snapshot_format = None),
            ("resources", |i| i.resources = None),
            ("gate", |i| {
                i.resources = Some(resources(COMMIT, false, "release"))
            }),
            ("gate mode", |i| {
                i.resources = Some(resources(COMMIT, true, "development"))
            }),
            ("other commit", |i| {
                i.resources = Some(resources(&"f".repeat(40), true, "release"));
            }),
        ];
        for (name, apply) in cases {
            let mut inputs = official_inputs();
            apply(&mut inputs);
            assert!(assemble(&inputs).is_err(), "{name} must be rejected");
        }
    }

    #[test]
    fn malformed_values_are_rejected() {
        type Break = fn(&mut BuildInputs);
        let cases: [Break; 4] = [
            |i| i.requested_kind = Some("beta".into()),
            |i| i.supplied_commit = Some("abc".into()),
            |i| i.built_at_utc = Some("yesterday".into()),
            |i| i.app_db_format = Some("x".into()),
        ];
        for apply in cases {
            let mut inputs = official_inputs();
            apply(&mut inputs);
            assert!(assemble(&inputs).is_err());
        }
    }

    #[test]
    fn developer_build_may_embed_resources_without_claiming_official() -> Result<(), String> {
        let mut inputs = official_inputs();
        inputs.requested_kind = None;
        inputs.resources = Some(resources(COMMIT, false, "development"));
        let info = assemble(&inputs)?;
        assert_eq!(info["build"]["buildKind"], "developer");
        assert_eq!(
            info["build"]["supportMatrixRevision"],
            "2026-11-13.0123456789ab"
        );
        Ok(())
    }
}
