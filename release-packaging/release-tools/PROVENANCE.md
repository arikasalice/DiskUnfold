# Release provenance verification

`verify-release-provenance` verifies one downloaded file against an explicit SLSA v1 release identity. It invokes `gh attestation verify` itself, then checks the verified statement for the artifact SHA-256, repository, `refs/tags/<tag>`, resolved Git commit, top-level GitHub Actions workflow path, push event, run ID and attempt, and caller-selected builder ID.

```text
cargo run --locked -p du-release-tools --bin verify-release-provenance -- <artifact> <owner/repo> <tag> <commit-sha> <run-id> <run-attempt> <.github/workflows/file.yml> <event-name> <builder-id-uri>
```

Provide expected values from an independent release policy or trusted CI context. The verifier does not adopt policy from the statement. It asks GitHub CLI to verify the signature and certificate identity for the repository and tag-specific signer workflow, plus the expected source ref, commit, SLSA predicate and GitHub OIDC issuer. GitHub CLI obtains attestations through the GitHub API; the verifier does not create, sign, upload, or publish anything.

The checks implement a strict GitHub Actions Workflow build type over SLSA provenance v1. The GitHub Actions Workflow build type is community maintained by SLSA. The caller supplies the expected event (`create`, `release`, `push`, or `workflow_dispatch`); the verifier requires an exact match. It rejects extra external parameters such as nonempty workflow-dispatch inputs or non-default release fields because this policy has no independent expected values for them.

## Trust boundary

A successful run proves that the installed GitHub CLI verified an attestation matching the supplied signer and artifact policy, and that the statement fields match the explicit expectations. It does not prove that the signer workflow is safe from a compromised caller or malicious workflow code. GitHub documents that statement predicate fields can be controlled by the workflow. The workflow and builder policy still need independent review and protection; a future release design may choose a caller-independent reusable trusted workflow.

The unit-test JSON is synthetic, unsigned fixture data. It tests only the parser's fail-closed policy and is never used by the production CLI as verification evidence.

This repository currently has no release workflow, and the architecture DLD for release packaging is still a draft. The command is therefore not wired into a release or publication gate. A future workflow must select and protect its signer workflow and builder, pass independently sourced tag, commit, run ID, and attempt values, invoke this verifier for each artifact, and stop before publication on any failure.

References:

- [SLSA v1 provenance](https://slsa.dev/spec/v1.0/provenance)
- [SLSA GitHub Actions Workflow build type](https://github.com/slsa-framework/github-actions-buildtypes/tree/main/workflow/v1)
- [GitHub CLI `gh attestation verify`](https://cli.github.com/manual/gh_attestation_verify)
