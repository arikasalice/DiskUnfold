//! Validate artifact provenance through GitHub's cryptographic attestation verifier.
use du_release_tools::release_provenance::{
    ReleaseProvenanceExpectation, verify_release_provenance,
};
use std::{env, error::Error, io, path::PathBuf, process};

fn main() {
    if let Err(error) = run() {
        eprintln!("verify-release-provenance: {error}");
        process::exit(1);
    }
}

fn run() -> Result<(), Box<dyn Error>> {
    let mut args = env::args_os().skip(1);
    let Some(artifact) = args.next().map(PathBuf::from) else {
        return Err(usage().into());
    };
    let mut policy = Vec::new();
    for value in args {
        let value = value.to_str().ok_or_else(usage)?;
        policy.push(value.to_owned());
    }
    if policy.len() != 8 {
        return Err(usage().into());
    }
    let expected = ReleaseProvenanceExpectation {
        repository: policy[0].clone(),
        tag: policy[1].clone(),
        commit: policy[2].clone(),
        run_id: policy[3].clone(),
        run_attempt: policy[4].clone(),
        workflow_path: policy[5].clone(),
        event_name: policy[6].clone(),
        builder_id: policy[7].clone(),
    };
    let verified = verify_release_provenance(&artifact, &expected)?;
    println!("Cryptographically verified SLSA v1 provenance");
    println!("artifact_sha256={}", verified.artifact_sha256);
    println!("repository={}", verified.repository);
    println!("source_ref={}", verified.source_ref);
    println!("source_commit={}", verified.source_commit);
    println!("workflow_path={}", verified.workflow_path);
    println!("invocation_id={}", verified.invocation_id);
    println!("builder_id={}", verified.builder_id);
    Ok(())
}

fn usage() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "usage: verify-release-provenance <artifact> <owner/repo> <tag> <commit-sha> <run-id> <run-attempt> <.github/workflows/file.yml> <event-name> <builder-id-uri>",
    )
}
