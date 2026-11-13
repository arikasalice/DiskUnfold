# Release provenance verification

`verify-release-provenance` verifies one downloaded file against an explicit SLSA v1 release identity. It snapshots the artifact and the GitHub CLI executable into a newly created temporary directory, checks the CLI executable against a caller-pinned SHA-256, requires GitHub CLI 2.102.0 or newer, invokes `gh attestation verify`, then checks the verified statement for the artifact SHA-256, repository, `refs/tags/<tag>`, resolved Git commit, top-level GitHub Actions workflow path, push event, run ID and attempt, and caller-selected builder ID.

```text
cargo run --locked -p du-release-tools --bin verify-release-provenance -- <artifact> <owner/repo> <tag> <commit-sha> <run-id> <run-attempt> <.github/workflows/file.yml> <event-name> <builder-id-uri> <absolute-gh-path> <gh-sha256> [--deny-self-hosted-runners]
```

Provide expected values, including the absolute GitHub CLI path and its lower-case SHA-256, from an independent release policy or trusted CI context. A path or hash derived from the untrusted artifact or workflow does not establish tool trust. The verifier does not resolve `gh` through `PATH` and does not adopt policy from the statement. It asks GitHub CLI to verify the signature and certificate identity for the repository and tag-specific signer workflow, plus the expected source ref, commit, SLSA predicate and GitHub OIDC issuer. It pins the verification host to `github.com`. Add `--deny-self-hosted-runners` when the release policy requires a GitHub-hosted runner; the flag is optional so deployments can choose their own trusted builder policy. GitHub CLI obtains attestations through the GitHub API; the verifier does not create, sign, upload, or publish anything.

The checks implement a strict GitHub Actions Workflow build type over SLSA provenance v1. The GitHub Actions Workflow build type is community maintained by SLSA. The caller supplies the expected event (`create`, `release`, `push`, or `workflow_dispatch`); the verifier requires an exact match. It rejects extra external parameters such as nonempty workflow-dispatch inputs or non-default release fields because this policy has no independent expected values for them.

## Trust boundary

A successful run means that the staged executable matching the caller-supplied SHA-256 reported successful GitHub CLI verification and the returned statement matched the supplied artifact and release policy. This command cannot establish that the supplied executable hash or other policy values came from a trusted source; the caller must protect them independently and run the verifier in a trusted environment. The verifier checks the staged artifact snapshot; callers must publish those same bytes or revalidate the original file at the publication boundary. It hashes the staged copy after opening the handle used during verification. On Windows that handle denies later write/delete opens while verification runs, but the temporary directory inherits the configured temp ACL, which the verifier does not audit. On Unix the temporary directory is owner-only, but another process running as the same user can still alter it. These measures do not defend against a compromised verifier host, a same-user attacker, or tampering before the executable is copied into the private directory. The command also does not prove that the signer workflow is safe from a compromised caller or malicious workflow code. GitHub documents that statement predicate fields can be controlled by the workflow. The workflow and builder policy still need independent review and protection; a future release design may choose a caller-independent reusable trusted workflow.

The minimum GitHub CLI version is 2.102.0 because earlier versions could accept a signer workflow or Git reference that only matched a prefix. The verifier also compares the CLI file bytes with the caller-pinned digest before running that exact staged copy. Obtain that digest from trusted release policy, not by trusting an executable found through an attacker-controlled `PATH`.

The unit-test JSON is synthetic, unsigned fixture data. It tests only the parser's fail-closed policy and is never used by the production CLI as verification evidence.

`.github/workflows/release.yml` creates the attestations (`actions/attest-build-provenance` over `SHA256SUMS`) and its `verify-provenance` job runs this verifier for every package before the draft release is created. The architecture DLD for release packaging is still unpublished, so the job names and reason keys in `release-packaging/release/README.md` need design registration. In that job the `gh` path and digest come from the runner image, the builder ID is `https://github.com/<repo>/.github/workflows/release.yml@refs/tags/<tag>`, and no real tag run has exercised the verifier yet. How a user runs it on a download is in `release-packaging/release/VERIFY.md`.

References:

- [SLSA v1 provenance](https://slsa.dev/spec/v1.0/provenance)
- [SLSA GitHub Actions Workflow build type](https://github.com/slsa-framework/github-actions-buildtypes/tree/main/workflow/v1)
- [GitHub CLI `gh attestation verify`](https://cli.github.com/manual/gh_attestation_verify)
