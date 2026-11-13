use super::{
    github_cli::{prepare_gh, verify_artifact},
    staged_file::StagedFile,
};
use serde_json::Value;
#[cfg(test)]
use sha2::{Digest, Sha256};
use std::{
    error::Error,
    fmt,
    path::{Path, PathBuf},
};
#[cfg(test)]
use std::{fs::File, io::Read};

/// SLSA provenance predicate accepted by this verifier.
pub const SLSA_PROVENANCE_V1: &str = "https://slsa.dev/provenance/v1";
/// The SLSA community build type describing a top-level GitHub Actions workflow.
pub const GITHUB_ACTIONS_WORKFLOW_BUILD_TYPE: &str =
    "https://slsa-framework.github.io/github-actions-buildtypes/workflow/v1";
/// In-toto statement envelope required by the SLSA v1 predicate.
pub const IN_TOTO_STATEMENT_V1: &str = "https://in-toto.io/Statement/v1";

/// Values a release consumer expects the cryptographically verified statement to prove.
///
/// Every field is explicit so that the verifier never adopts the repository, tag, workflow,
/// builder, run, or commit from the untrusted statement it is checking.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ReleaseProvenanceExpectation {
    /// GitHub repository in `owner/name` form.
    pub repository: String,
    /// Release tag without the `refs/tags/` prefix.
    pub tag: String,
    /// Full, lower-case 40-character Git commit object ID named by the tag.
    pub commit: String,
    /// GitHub Actions run ID expected for this release build.
    pub run_id: String,
    /// GitHub Actions run attempt expected for this release build.
    pub run_attempt: String,
    /// Repository-relative workflow path, such as `.github/workflows/release.yml`.
    pub workflow_path: String,
    /// Expected GitHub event name; it must be supported by the SLSA workflow build type.
    pub event_name: String,
    /// Policy-selected SLSA builder identity. It is never learned from the attestation.
    pub builder_id: String,
    /// Absolute path to the GitHub CLI executable selected by trusted caller policy.
    pub gh_executable: PathBuf,
    /// Lower-case SHA-256 of the expected GitHub CLI executable from independent policy.
    pub gh_sha256: String,
    /// When true, ask GitHub CLI to reject provenance made on self-hosted runners.
    pub deny_self_hosted_runners: bool,
}

/// Release identity values extracted only after successful GitHub CLI verification.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct VerifiedReleaseProvenance {
    /// SHA-256 of the private artifact snapshot passed to GitHub CLI.
    pub artifact_sha256: String,
    /// Repository whose tag and workflow are recorded in provenance.
    pub repository: String,
    /// Exact source tag ref recorded in provenance.
    pub source_ref: String,
    /// Git commit recorded as the tag's resolved source dependency.
    pub source_commit: String,
    /// Workflow path recorded in provenance.
    pub workflow_path: String,
    /// Exact invocation ID recorded in provenance.
    pub invocation_id: String,
    /// SLSA builder ID recorded in provenance.
    pub builder_id: String,
}

/// Failure to verify the artifact and its expected release provenance.
#[derive(Debug)]
pub struct ReleaseProvenanceError {
    message: String,
}

impl ReleaseProvenanceError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ReleaseProvenanceError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for ReleaseProvenanceError {}

/// Runs a caller-pinned GitHub CLI and checks its verified SLSA statement against explicit policy.
///
/// GitHub CLI performs signature, attestation, repository, source ref, source commit, and signer
/// workflow verification. This function additionally binds the artifact bytes, run invocation,
/// workflow parameters, source dependency, and SLSA builder to the caller's expectation. GitHub
/// documents that predicate fields can be workflow-controlled; this verifier does not prove that
/// the selected workflow is protected from a compromised caller. Trust in that workflow must come
/// from independent policy and workflow review. It does not create attestations or publish
/// artifacts. A successful result relies on the caller-pinned GitHub CLI executable and GitHub's
/// verification roots; callers must not substitute JSON fixtures for the command's successful
/// output. The caller must source the executable path and its pinned hash from trusted policy.
/// The verifier snapshots artifact bytes before invoking GitHub CLI; a later publisher must use
/// those same bytes or revalidate the source artifact at its publication boundary.
pub fn verify_release_provenance(
    artifact: &Path,
    expected: &ReleaseProvenanceExpectation,
) -> Result<VerifiedReleaseProvenance, ReleaseProvenanceError> {
    let checked = CheckedExpectation::new(expected)?;
    let gh = prepare_gh(&expected.gh_executable, &expected.gh_sha256)?;
    let artifact = StagedFile::from_path(artifact, "artifact", false).map_err(|error| {
        ReleaseProvenanceError::new(format!(
            "failed to stage artifact for verification: {error}"
        ))
    })?;
    let output = verify_artifact(&gh, &artifact, expected, &checked.source_ref)?;
    validate_gh_verified_output(output.as_slice(), artifact.sha256(), expected)
}

/// Applies release policy to JSON returned by a successful `gh attestation verify --format=json`.
///
/// The input is not independently authenticated. The production caller uses this only after a
/// successful GitHub CLI verification with the same repository, source ref, source digest,
/// predicate, and signer workflow policy as [`verify_release_provenance`].
fn validate_gh_verified_output(
    output: &[u8],
    artifact_sha256: &str,
    expected: &ReleaseProvenanceExpectation,
) -> Result<VerifiedReleaseProvenance, ReleaseProvenanceError> {
    let checked = CheckedExpectation::new(expected)?;
    if !is_hex_digest(artifact_sha256, 64) {
        return Err(ReleaseProvenanceError::new(
            "expected artifact SHA-256 must be 64 lower-case hexadecimal characters",
        ));
    }
    let parsed: Value = serde_json::from_slice(output).map_err(|error| {
        ReleaseProvenanceError::new(format!("GitHub CLI returned invalid JSON: {error}"))
    })?;
    let results = parsed.as_array().ok_or_else(|| {
        ReleaseProvenanceError::new("GitHub CLI verification output must be a JSON array")
    })?;
    if results.len() != 1 {
        return Err(ReleaseProvenanceError::new(format!(
            "expected exactly one verified attestation, received {}",
            results.len()
        )));
    }

    let attestation = required_field(&results[0], "attestation")?;
    if !attestation.is_object() {
        return Err(ReleaseProvenanceError::new(
            "verified result is missing the parsed attestation bundle",
        ));
    }
    let verification = required_field(&results[0], "verificationResult")?;
    let signature = required_field(verification, "signature")?;
    let certificate = required_field(signature, "certificate")?;
    if !certificate.is_object() {
        return Err(ReleaseProvenanceError::new(
            "verified result is missing the parsed signer certificate",
        ));
    }
    if !required_field(verification, "verifiedTimestamps")?.is_array() {
        return Err(ReleaseProvenanceError::new(
            "verified result must include the GitHub CLI timestamp verification list",
        ));
    }
    let statement = required_field(verification, "statement")?;
    require_string_eq(statement, "_type", IN_TOTO_STATEMENT_V1)?;
    require_string_eq(statement, "predicateType", SLSA_PROVENANCE_V1)?;
    validate_subject(statement, artifact_sha256)?;

    let predicate = required_field(statement, "predicate")?;
    let build_definition = required_field(predicate, "buildDefinition")?;
    require_string_eq(
        build_definition,
        "buildType",
        GITHUB_ACTIONS_WORKFLOW_BUILD_TYPE,
    )?;
    validate_external_parameters(build_definition, expected, &checked)?;
    validate_source_dependency(build_definition, expected, &checked)?;
    validate_event_type(predicate, &expected.event_name)?;

    let run_details = required_field(predicate, "runDetails")?;
    let builder = required_field(run_details, "builder")?;
    let builder_id = required_string(builder, "id")?;
    if builder_id != expected.builder_id {
        return Err(ReleaseProvenanceError::new(format!(
            "SLSA builder ID mismatch: expected {}, received {builder_id}",
            expected.builder_id
        )));
    }
    let metadata = required_field(run_details, "metadata")?;
    let invocation_id = required_string(metadata, "invocationId")?;
    if invocation_id != checked.invocation_id {
        return Err(ReleaseProvenanceError::new(format!(
            "SLSA invocation ID mismatch: expected {}, received {invocation_id}",
            checked.invocation_id
        )));
    }

    Ok(VerifiedReleaseProvenance {
        artifact_sha256: artifact_sha256.to_owned(),
        repository: expected.repository.clone(),
        source_ref: checked.source_ref,
        source_commit: expected.commit.clone(),
        workflow_path: expected.workflow_path.clone(),
        invocation_id: checked.invocation_id,
        builder_id: expected.builder_id.clone(),
    })
}

struct CheckedExpectation {
    repository_uri: String,
    source_ref: String,
    invocation_id: String,
}

impl CheckedExpectation {
    fn new(expected: &ReleaseProvenanceExpectation) -> Result<Self, ReleaseProvenanceError> {
        validate_repository(&expected.repository)?;
        validate_tag(&expected.tag)?;
        if !is_hex_digest(&expected.commit, 40) {
            return Err(ReleaseProvenanceError::new(
                "expected Git commit must be 40 lower-case hexadecimal characters",
            ));
        }
        validate_positive_integer("run ID", &expected.run_id)?;
        validate_positive_integer("run attempt", &expected.run_attempt)?;
        validate_workflow_path(&expected.workflow_path)?;
        if expected.builder_id.trim().is_empty()
            || expected.builder_id.chars().any(char::is_whitespace)
        {
            return Err(ReleaseProvenanceError::new(
                "expected builder ID must be a nonempty URI without whitespace",
            ));
        }
        let repository_uri = format!("https://github.com/{}", expected.repository);
        let source_ref = format!("refs/tags/{}", expected.tag);
        let invocation_id = format!(
            "{repository_uri}/actions/runs/{}/attempts/{}",
            expected.run_id, expected.run_attempt
        );
        Ok(Self {
            repository_uri,
            source_ref,
            invocation_id,
        })
    }
}

fn validate_subject(
    statement: &Value,
    expected_digest: &str,
) -> Result<(), ReleaseProvenanceError> {
    let subjects = required_field(statement, "subject")?
        .as_array()
        .ok_or_else(|| ReleaseProvenanceError::new("in-toto subject must be an array"))?;
    let mut matches = 0;
    for subject in subjects {
        if subject
            .get("digest")
            .and_then(|digest| digest.get("sha256"))
            .and_then(Value::as_str)
            == Some(expected_digest)
        {
            matches += 1;
        }
    }
    if matches != 1 {
        return Err(ReleaseProvenanceError::new(format!(
            "expected artifact SHA-256 subject exactly once, found {matches} matches"
        )));
    }
    Ok(())
}

fn validate_external_parameters(
    build_definition: &Value,
    expected: &ReleaseProvenanceExpectation,
    checked: &CheckedExpectation,
) -> Result<(), ReleaseProvenanceError> {
    let parameters = required_field(build_definition, "externalParameters")?;
    let object = parameters
        .as_object()
        .ok_or_else(|| ReleaseProvenanceError::new("SLSA externalParameters must be an object"))?;
    if object.len() != 1 || !object.contains_key("workflow") {
        return Err(ReleaseProvenanceError::new(
            "external parameters contain values without an explicit verifier policy",
        ));
    }
    let workflow = required_field(parameters, "workflow")?;
    let workflow_object = workflow.as_object().ok_or_else(|| {
        ReleaseProvenanceError::new("GitHub workflow external parameter must be an object")
    })?;
    if workflow_object.len() != 3
        || !workflow_object.contains_key("repository")
        || !workflow_object.contains_key("ref")
        || !workflow_object.contains_key("path")
    {
        return Err(ReleaseProvenanceError::new(
            "GitHub workflow external parameter has missing or unexpected fields",
        ));
    }
    require_string_eq(workflow, "repository", &checked.repository_uri)?;
    require_string_eq(workflow, "ref", &checked.source_ref)?;
    require_string_eq(workflow, "path", &expected.workflow_path)
}

fn validate_source_dependency(
    build_definition: &Value,
    expected: &ReleaseProvenanceExpectation,
    checked: &CheckedExpectation,
) -> Result<(), ReleaseProvenanceError> {
    let wanted_uri = format!("git+{}@{}", checked.repository_uri, checked.source_ref);
    let dependencies = required_field(build_definition, "resolvedDependencies")?
        .as_array()
        .ok_or_else(|| ReleaseProvenanceError::new("SLSA resolvedDependencies must be an array"))?;
    let matching: Vec<&Value> = dependencies
        .iter()
        .filter(|dependency| dependency.get("uri").and_then(Value::as_str) == Some(&wanted_uri))
        .collect();
    if matching.len() != 1 {
        return Err(ReleaseProvenanceError::new(format!(
            "expected exactly one resolved dependency for {wanted_uri}, found {}",
            matching.len()
        )));
    }
    let digest = required_field(matching[0], "digest")?;
    require_string_eq(digest, "gitCommit", &expected.commit)
}

fn validate_event_type(
    predicate: &Value,
    expected_event: &str,
) -> Result<(), ReleaseProvenanceError> {
    if !["create", "release", "push", "workflow_dispatch"].contains(&expected_event) {
        return Err(ReleaseProvenanceError::new(
            "expected event must be supported by the SLSA GitHub Actions workflow build type",
        ));
    }
    let build_definition = required_field(predicate, "buildDefinition")?;
    let internal_parameters = required_field(build_definition, "internalParameters")?;
    let github = required_field(internal_parameters, "github")?;
    require_string_eq(github, "event_name", expected_event)
}

fn required_field<'a>(object: &'a Value, name: &str) -> Result<&'a Value, ReleaseProvenanceError> {
    object.get(name).ok_or_else(|| {
        ReleaseProvenanceError::new(format!("verified SLSA statement is missing field {name}"))
    })
}

fn required_string<'a>(object: &'a Value, name: &str) -> Result<&'a str, ReleaseProvenanceError> {
    required_field(object, name)?
        .as_str()
        .ok_or_else(|| ReleaseProvenanceError::new(format!("field {name} must be a string")))
}

fn require_string_eq(
    object: &Value,
    name: &str,
    expected: &str,
) -> Result<(), ReleaseProvenanceError> {
    let actual = required_string(object, name)?;
    if actual == expected {
        Ok(())
    } else {
        Err(ReleaseProvenanceError::new(format!(
            "field {name} mismatch: expected {expected}, received {actual}"
        )))
    }
}

fn validate_repository(repository: &str) -> Result<(), ReleaseProvenanceError> {
    let parts: Vec<&str> = repository.split('/').collect();
    if parts.len() != 2
        || parts.iter().any(|part| {
            part.is_empty()
                || !part
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
        })
    {
        return Err(ReleaseProvenanceError::new(
            "repository must be a GitHub owner/name pair",
        ));
    }
    Ok(())
}

fn validate_tag(tag: &str) -> Result<(), ReleaseProvenanceError> {
    let valid = !tag.is_empty()
        && tag.trim() == tag
        && !tag.starts_with("refs/tags/")
        && !tag.contains("//")
        && !tag.contains("..")
        && !tag.contains("@{")
        && !tag.ends_with('/')
        && !tag.chars().any(char::is_whitespace)
        && !tag.chars().any(char::is_control)
        && !tag.contains('\\');
    if !valid {
        return Err(ReleaseProvenanceError::new(
            "tag must be a bare Git tag name without whitespace or a refs/tags/ prefix",
        ));
    }
    Ok(())
}

fn validate_positive_integer(field: &str, value: &str) -> Result<(), ReleaseProvenanceError> {
    let parsed = value.parse::<u64>().ok();
    if !matches!(parsed, Some(number) if number > 0 && number.to_string() == value) {
        return Err(ReleaseProvenanceError::new(format!(
            "expected {field} must be a positive canonical decimal integer"
        )));
    }
    Ok(())
}

fn validate_workflow_path(path: &str) -> Result<(), ReleaseProvenanceError> {
    let valid_suffix = path.ends_with(".yml") || path.ends_with(".yaml");
    if !path.starts_with(".github/workflows/")
        || !valid_suffix
        || path.trim() != path
        || path.contains('\\')
        || path.chars().any(char::is_control)
        || path
            .split('/')
            .any(|component| component.is_empty() || component == "." || component == "..")
    {
        return Err(ReleaseProvenanceError::new(
            "workflow path must be a repository-relative workflow YAML path",
        ));
    }
    Ok(())
}

fn is_hex_digest(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[cfg(test)]
fn sha256_file(path: &Path) -> Result<String, ReleaseProvenanceError> {
    let mut file = File::open(path).map_err(|error| {
        ReleaseProvenanceError::new(format!("failed to open artifact: {error}"))
    })?;
    let mut hasher = Sha256::new();
    let mut buffer = [0; 64 * 1024];
    loop {
        let read = file.read(&mut buffer).map_err(|error| {
            ReleaseProvenanceError::new(format!("failed to read artifact: {error}"))
        })?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(encode_hex(&hasher.finalize()))
}

#[cfg(test)]
fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }
    output
}

#[cfg(test)]
mod tests;
