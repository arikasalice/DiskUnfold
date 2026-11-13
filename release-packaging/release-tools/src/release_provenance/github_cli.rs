use super::{ReleaseProvenanceError, ReleaseProvenanceExpectation};
use crate::release_provenance::staged_file::StagedFile;
use std::{path::Path, process::Command};

const MINIMUM_GH_VERSION: (u64, u64, u64) = (2, 102, 0);

/// Stages and checks the exact GitHub CLI executable selected by trusted caller policy.
pub(super) fn prepare_gh(
    executable: &Path,
    expected_sha256: &str,
) -> Result<StagedFile, ReleaseProvenanceError> {
    if !executable.is_absolute() {
        return Err(ReleaseProvenanceError::new(
            "GitHub CLI executable path must be absolute",
        ));
    }
    if !is_hex_digest(expected_sha256) {
        return Err(ReleaseProvenanceError::new(
            "expected GitHub CLI SHA-256 must be 64 lower-case hexadecimal characters",
        ));
    }
    #[cfg(windows)]
    let staged_name = "gh.exe";
    #[cfg(not(windows))]
    let staged_name = "gh";
    let staged = StagedFile::from_path(executable, staged_name, true).map_err(|error| {
        ReleaseProvenanceError::new(format!("failed to stage GitHub CLI executable: {error}"))
    })?;
    if staged.sha256() != expected_sha256 {
        return Err(ReleaseProvenanceError::new(
            "GitHub CLI executable does not match the caller-pinned SHA-256",
        ));
    }
    ensure_supported_version(&staged)?;
    Ok(staged)
}

/// Invokes only the staged executable and uses the public GitHub host explicitly.
pub(super) fn verify_artifact(
    gh: &StagedFile,
    artifact: &StagedFile,
    expected: &ReleaseProvenanceExpectation,
    source_ref: &str,
) -> Result<Vec<u8>, ReleaseProvenanceError> {
    let signer_workflow = format!(
        "{}/{}@{}",
        expected.repository, expected.workflow_path, source_ref
    );
    let mut command = Command::new(gh.path());
    command
        .arg("attestation")
        .arg("verify")
        .arg(artifact.path())
        .arg("--hostname")
        .arg("github.com")
        .arg("--repo")
        .arg(&expected.repository)
        .arg("--predicate-type")
        .arg(super::SLSA_PROVENANCE_V1)
        .arg("--source-ref")
        .arg(source_ref)
        .arg("--source-digest")
        .arg(&expected.commit)
        .arg("--signer-workflow")
        .arg(signer_workflow)
        .arg("--cert-oidc-issuer")
        .arg("https://token.actions.githubusercontent.com");
    if expected.deny_self_hosted_runners {
        command.arg("--deny-self-hosted-runners");
    }
    let output = command.arg("--format=json").output().map_err(|error| {
        ReleaseProvenanceError::new(format!(
            "failed to run pinned `gh attestation verify`: {error}"
        ))
    })?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr);
        let detail = detail.trim();
        return Err(ReleaseProvenanceError::new(if detail.is_empty() {
            "`gh attestation verify` rejected the artifact or its signer".to_owned()
        } else {
            format!("`gh attestation verify` rejected the artifact: {detail}")
        }));
    }
    Ok(output.stdout)
}

fn ensure_supported_version(gh: &StagedFile) -> Result<(), ReleaseProvenanceError> {
    let output = Command::new(gh.path())
        .arg("--version")
        .output()
        .map_err(|error| {
            ReleaseProvenanceError::new(format!(
                "failed to query pinned GitHub CLI version: {error}"
            ))
        })?;
    if !output.status.success() {
        return Err(ReleaseProvenanceError::new(
            "pinned GitHub CLI did not report a successful version",
        ));
    }
    let version = parse_version(&output.stdout)
        .or_else(|| parse_version(&output.stderr))
        .ok_or_else(|| {
            ReleaseProvenanceError::new("pinned GitHub CLI returned an unknown version")
        })?;
    if version < MINIMUM_GH_VERSION {
        return Err(ReleaseProvenanceError::new(format!(
            "GitHub CLI {}.{}.{} is too old; version 2.102.0 or newer is required",
            version.0, version.1, version.2
        )));
    }
    Ok(())
}

fn parse_version(output: &[u8]) -> Option<(u64, u64, u64)> {
    let text = std::str::from_utf8(output).ok()?;
    let line = text.lines().find(|line| line.starts_with("gh version "))?;
    let version = line.split_whitespace().nth(2)?;
    let mut parts = version.split('.');
    let major = parts.next()?.parse().ok()?;
    let minor = parts.next()?.parse().ok()?;
    let patch = parts.next()?.parse().ok()?;
    if parts.next().is_some() {
        return None;
    }
    Some((major, minor, patch))
}

fn is_hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
mod tests {
    use super::{MINIMUM_GH_VERSION, parse_version, prepare_gh};
    use std::{io::Write, path::Path};
    use tempfile::NamedTempFile;

    #[test]
    fn parses_supported_gh_version_format() {
        assert_eq!(
            parse_version(b"gh version 2.103.3 (2026-10-26)\nhttps://github.com/cli/cli"),
            Some((2, 103, 3))
        );
    }

    #[test]
    fn rejects_unknown_and_prerelease_version_formats() {
        assert_eq!(parse_version(b"gh version 2.102.0-rc.1"), None);
        assert_eq!(parse_version(b"gh version nightly"), None);
    }

    #[test]
    fn supported_version_floor_includes_cli_signer_match_fix() {
        assert_eq!(MINIMUM_GH_VERSION, (2, 102, 0));
        assert!(parse_version(b"gh version 2.101.9").is_some_and(|v| v < MINIMUM_GH_VERSION));
        assert!(parse_version(b"gh version 2.102.0").is_some_and(|v| v >= MINIMUM_GH_VERSION));
    }

    #[test]
    fn rejects_relative_executable_path_instead_of_searching_path() {
        let result = prepare_gh(Path::new("gh"), &"c".repeat(64));
        let error = result
            .err()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "unexpectedly accepted a relative executable path".to_owned());
        assert!(error.contains("path must be absolute"));
    }

    #[test]
    fn rejects_unpinned_executable_before_version_probe() -> Result<(), Box<dyn std::error::Error>>
    {
        let mut fake_gh = NamedTempFile::new()?;
        fake_gh.write_all(b"not a GitHub CLI executable")?;
        fake_gh.flush()?;

        let result = prepare_gh(fake_gh.path(), &"c".repeat(64));
        let error = result
            .err()
            .map(|error| error.to_string())
            .unwrap_or_else(|| "unexpectedly accepted an unpinned executable".to_owned());
        assert!(error.contains("does not match the caller-pinned SHA-256"));
        Ok(())
    }
}
