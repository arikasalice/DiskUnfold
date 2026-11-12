use super::*;
use serde_json::{Value, json};
use std::{error::Error, io::Write, path::PathBuf};
use tempfile::NamedTempFile;

fn expectation() -> ReleaseProvenanceExpectation {
    ReleaseProvenanceExpectation {
        repository: "arikasalice/DiskUnfold".to_owned(),
        tag: "v0.1.0".to_owned(),
        commit: "a".repeat(40),
        run_id: "1880241037".to_owned(),
        run_attempt: "2".to_owned(),
        workflow_path: ".github/workflows/release.yml".to_owned(),
        event_name: "push".to_owned(),
        builder_id: "https://github.com/actions/runner/github-hosted".to_owned(),
        gh_executable: PathBuf::from(if cfg!(windows) {
            "C:\\Program Files\\GitHub CLI\\gh.exe"
        } else {
            "/usr/bin/gh"
        }),
        gh_sha256: "c".repeat(64),
        deny_self_hosted_runners: false,
    }
}

fn fixture(digest: &str, expected: &ReleaseProvenanceExpectation) -> Value {
    let repository_uri = format!("https://github.com/{}", expected.repository);
    let source_ref = format!("refs/tags/{}", expected.tag);
    let invocation_id = format!(
        "{repository_uri}/actions/runs/{}/attempts/{}",
        expected.run_id, expected.run_attempt
    );
    json!([{
        "attestation": {"synthetic_fixture": true},
        "verificationResult": {
            "signature": {"certificate": {"synthetic_fixture": true}},
            "verifiedTimestamps": [],
            "statement": {
                "_type": IN_TOTO_STATEMENT_V1,
                "subject": [{"name": "DiskUnfold-0.1.0-x64.msi", "digest": {"sha256": digest}}],
                "predicateType": SLSA_PROVENANCE_V1,
                "predicate": {
                    "buildDefinition": {
                        "buildType": GITHUB_ACTIONS_WORKFLOW_BUILD_TYPE,
                        "externalParameters": {
                            "workflow": {
                                "repository": repository_uri,
                                "ref": source_ref,
                                "path": expected.workflow_path
                            }
                        },
                        "internalParameters": {"github": {"event_name": expected.event_name}},
                        "resolvedDependencies": [{
                            "uri": format!("git+{repository_uri}@{source_ref}"),
                            "digest": {"gitCommit": expected.commit}
                        }]
                    },
                    "runDetails": {
                        "builder": {"id": expected.builder_id},
                        "metadata": {"invocationId": invocation_id}
                    }
                }
            }
        }
    }])
}

fn set_path(document: &mut Value, path: &[&str], value: Value) -> Result<(), Box<dyn Error>> {
    if path.is_empty() {
        return Err("fixture path is empty".into());
    }
    let path = std::iter::once("0")
        .chain(path.iter().copied())
        .collect::<Vec<_>>();
    let mut node = document;
    for key in &path[..path.len() - 1] {
        node = match key.parse::<usize>() {
            Ok(index) => node.get_mut(index).ok_or("fixture array index is absent")?,
            Err(_) => node.get_mut(*key).ok_or("fixture field is absent")?,
        };
    }
    let last = path.last().ok_or("fixture path is empty")?;
    let object = node
        .as_object_mut()
        .ok_or("fixture parent is not an object")?;
    object.insert((*last).to_owned(), value);
    Ok(())
}

#[test]
fn accepts_synthetic_fixture_when_all_bound_values_match() -> Result<(), Box<dyn Error>> {
    // This fixture validates field checks only. It is not signed and proves no real provenance.
    let expected = expectation();
    let digest = "b".repeat(64);
    let output = serde_json::to_vec(&fixture(&digest, &expected))?;
    let result = validate_gh_verified_output(&output, &digest, &expected)?;
    assert_eq!(result.artifact_sha256, digest);
    assert_eq!(result.source_ref, "refs/tags/v0.1.0");
    assert_eq!(result.source_commit, expected.commit);
    assert_eq!(
        result.invocation_id,
        "https://github.com/arikasalice/DiskUnfold/actions/runs/1880241037/attempts/2"
    );
    assert_eq!(result.builder_id, expected.builder_id);
    Ok(())
}

#[test]
fn binds_the_subject_to_the_artifact_bytes() -> Result<(), Box<dyn Error>> {
    let expected = expectation();
    let mut artifact = NamedTempFile::new()?;
    artifact.write_all(b"abc")?;
    let digest = sha256_file(artifact.path())?;
    assert_eq!(
        digest,
        "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
    );
    let output = serde_json::to_vec(&fixture(&digest, &expected))?;
    let verified = validate_gh_verified_output(&output, &digest, &expected)?;
    assert_eq!(verified.artifact_sha256, digest);
    Ok(())
}

#[test]
fn rejects_mismatched_artifact_tag_commit_run_builder_and_workflow() -> Result<(), Box<dyn Error>> {
    let expected = expectation();
    let digest = "b".repeat(64);
    let mismatches: [(&[&str], Value); 8] = [
        (
            &[
                "verificationResult",
                "statement",
                "subject",
                "0",
                "digest",
                "sha256",
            ],
            json!("c".repeat(64)),
        ),
        (
            &[
                "verificationResult",
                "statement",
                "predicate",
                "buildDefinition",
                "externalParameters",
                "workflow",
                "ref",
            ],
            json!("refs/tags/v0.1.1"),
        ),
        (
            &[
                "verificationResult",
                "statement",
                "predicate",
                "buildDefinition",
                "resolvedDependencies",
                "0",
                "digest",
                "gitCommit",
            ],
            json!("c".repeat(40)),
        ),
        (
            &[
                "verificationResult",
                "statement",
                "predicate",
                "runDetails",
                "metadata",
                "invocationId",
            ],
            json!("https://github.com/arikasalice/DiskUnfold/actions/runs/7/attempts/1"),
        ),
        (
            &[
                "verificationResult",
                "statement",
                "predicate",
                "runDetails",
                "builder",
                "id",
            ],
            json!("https://example.invalid/builder"),
        ),
        (
            &[
                "verificationResult",
                "statement",
                "predicate",
                "buildDefinition",
                "externalParameters",
                "workflow",
                "path",
            ],
            json!(".github/workflows/other.yml"),
        ),
        (
            &[
                "verificationResult",
                "statement",
                "predicate",
                "buildDefinition",
                "internalParameters",
                "github",
                "event_name",
            ],
            json!("workflow_dispatch"),
        ),
        (
            &["verificationResult", "statement", "predicateType"],
            json!("https://slsa.dev/provenance/v0.2"),
        ),
    ];
    for (path, value) in mismatches {
        let mut document = fixture(&digest, &expected);
        set_path(&mut document, path, value)?;
        let output = serde_json::to_vec(&document)?;
        assert!(
            validate_gh_verified_output(&output, &digest, &expected).is_err(),
            "mismatch at {path:?} must fail closed"
        );
    }
    Ok(())
}

#[test]
fn rejects_ambiguous_or_unverified_looking_cli_output() -> Result<(), Box<dyn Error>> {
    let expected = expectation();
    let digest = "b".repeat(64);
    let valid = fixture(&digest, &expected);
    let mut multiple = valid.clone();
    multiple
        .as_array_mut()
        .ok_or("fixture is not an array")?
        .push(valid[0].clone());
    let multiple_output = serde_json::to_vec(&multiple)?;
    assert!(validate_gh_verified_output(&multiple_output, &digest, &expected).is_err());

    let mut missing_certificate = fixture(&digest, &expected);
    let result = missing_certificate[0]
        .get_mut("verificationResult")
        .ok_or("verification result is missing")?;
    result
        .get_mut("signature")
        .ok_or("signature is missing")?
        .as_object_mut()
        .ok_or("signature is not an object")?
        .remove("certificate");
    let missing_certificate_output = serde_json::to_vec(&missing_certificate)?;
    assert!(validate_gh_verified_output(&missing_certificate_output, &digest, &expected).is_err());
    Ok(())
}

#[test]
fn rejects_duplicate_subjects_unconfigured_parameters_and_malformed_json()
-> Result<(), Box<dyn Error>> {
    let expected = expectation();
    let digest = "b".repeat(64);

    let mut duplicate_subject = fixture(&digest, &expected);
    let subjects = duplicate_subject[0]["verificationResult"]["statement"]["subject"]
        .as_array_mut()
        .ok_or("fixture subject is not an array")?;
    subjects.push(subjects[0].clone());
    let duplicate_output = serde_json::to_vec(&duplicate_subject)?;
    assert!(validate_gh_verified_output(&duplicate_output, &digest, &expected).is_err());

    let mut extra_parameter = fixture(&digest, &expected);
    extra_parameter[0]["verificationResult"]["statement"]["predicate"]["buildDefinition"]["externalParameters"]
        .as_object_mut()
        .ok_or("fixture external parameters are not an object")?
        .insert("unreviewed".to_owned(), json!({"value": true}));
    let extra_parameter_output = serde_json::to_vec(&extra_parameter)?;
    assert!(validate_gh_verified_output(&extra_parameter_output, &digest, &expected).is_err());

    assert!(validate_gh_verified_output(b"{not-json", &digest, &expected).is_err());
    Ok(())
}

#[test]
fn rejects_noncanonical_expectations() -> Result<(), Box<dyn Error>> {
    let digest = "b".repeat(64);
    let invalid_values = [
        ("tag", "refs/tags/v0.1.0"),
        ("commit", "AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"),
        ("run_attempt", "02"),
        ("workflow_path", "../release.yml"),
        ("event_name", "pull_request"),
    ];
    for (field, invalid) in invalid_values {
        let mut expected = expectation();
        match field {
            "tag" => expected.tag = invalid.to_owned(),
            "commit" => expected.commit = invalid.to_owned(),
            "run_attempt" => expected.run_attempt = invalid.to_owned(),
            "workflow_path" => expected.workflow_path = invalid.to_owned(),
            _ => expected.event_name = invalid.to_owned(),
        }
        let output = serde_json::to_vec(&fixture(&digest, &expected))?;
        assert!(
            validate_gh_verified_output(&output, &digest, &expected).is_err(),
            "invalid {field} expectation must fail closed"
        );
    }
    Ok(())
}
