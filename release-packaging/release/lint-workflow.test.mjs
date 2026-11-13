import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { lintFuzzWorkflow, lintStages, lintWorkflow, pinnedActions, verifyRemote } from "./lint-workflow.mjs";

const workflow = readFileSync(
  fileURLToPath(new URL("../../.github/workflows/release.yml", import.meta.url)),
  "utf8",
);
const SHA = "0123456789abcdef0123456789abcdef01234567";

test("the release workflow satisfies the policy", () => {
  assert.deepEqual(lintWorkflow(workflow), []);
  assert.ok(pinnedActions(workflow).length >= 6);
});

test("an action pinned to a tag is rejected", () => {
  const problems = lintWorkflow(workflow.replace(/actions\/checkout@[0-9a-f]{40}/, "actions/checkout@v4"));
  assert.ok(problems.some((p) => p.includes("not pinned")));
});

test("a short or uppercase SHA is rejected", () => {
  assert.ok(lintWorkflow(workflow.replace(/actions\/setup-node@[0-9a-f]{40}/, "actions/setup-node@49933ea")).length > 0);
  assert.ok(lintWorkflow(workflow.replace(/actions\/setup-node@[0-9a-f]{40}/, `actions/setup-node@${SHA.toUpperCase()}`)).length > 0);
});

test("a pinned action without a version comment is rejected", () => {
  const problems = lintWorkflow(workflow.replace(/(actions\/checkout@[0-9a-f]{40}) # v7.0.1/, "$1"));
  assert.ok(problems.some((p) => p.includes("version comment")));
});

test("an unconfirmed SHA marker fails the lint", () => {
  const problems = lintWorkflow(workflow.replace(/actions\/attest-build-provenance@[0-9a-f]{40} # v4.2.2/, "actions/attest-build-provenance@TODO(sha) # unconfirmed"));
  assert.ok(problems.some((p) => p.includes("TODO")));
  assert.ok(problems.some((p) => p.includes("not pinned")));
});

test("extra triggers are rejected", () => {
  assert.ok(lintWorkflow(workflow.replace("  workflow_dispatch:", "  pull_request:\n  workflow_dispatch:")).some((p) => p.includes("pull_request")));
  assert.ok(lintWorkflow(workflow.replace("    tags:", "    branches: [main]\n    tags:")).some((p) => p.includes("branches")));
});

test("a hard-coded official build kind is rejected", () => {
  assert.ok(lintWorkflow(workflow.replace(/DU_BUILD_KIND: .*/, "DU_BUILD_KIND: official")).some((p) => p.includes("DU_BUILD_KIND")));
});

test("publishing without --draft is rejected", () => {
  assert.ok(lintWorkflow(workflow.replace(" --draft", "")).some((p) => p.includes("--draft")));
});

test("remote verification accepts a matching tag and rejects a moved one", () => {
  const action = { repository: "owner/repo", sha: SHA, version: "v1.0.0" };
  assert.deepEqual(verifyRemote([action], () => `${SHA}\trefs/tags/v1.0.0\n`), []);
  assert.deepEqual(verifyRemote([action], () => `ffffffffffffffffffffffffffffffffffffffff\trefs/tags/v1.0.0\n${SHA}\trefs/tags/v1.0.0^{}\n`), []);
  assert.equal(verifyRemote([action], () => "ffffffffffffffffffffffffffffffffffffffff\trefs/tags/v1.0.0\n").length, 1);
  assert.equal(verifyRemote([action], () => "").length, 1);
});

test("the id-token, attestation and content-write permissions stay in their one job", () => {
  const widen = (job, extra) => workflow.replace(new RegExp(`(\\n  ${job}:\\n(?:.*\\n)*?    runs-on: [^\\n]*\\n)`), `$1    permissions:\n${extra}\n`);
  assert.ok(lintWorkflow(widen("inventory", "      id-token: write")).some((p) => p.includes("id-token")));
  assert.ok(lintWorkflow(widen("verify-provenance", "      contents: write")).some((p) => p.includes("must not write contents")));
  assert.ok(lintWorkflow(widen("gate", "      attestations: write")).some((p) => p.includes("attestations")));
  assert.ok(lintWorkflow(widen("build", "      packages: write")).some((p) => p.includes("unexpected write")));
  assert.ok(lintWorkflow(workflow.replace("      id-token: write\n", "")).some((p) => p.includes("attest must hold id-token")));
});

test("a secret in job-level env is rejected", () => {
  const leaked = workflow.replace("    env:\n      DU_BUILD_KIND:", "    env:\n      SIGNPATH_API_TOKEN: ${{ secrets.SIGNPATH_API_TOKEN }}\n      DU_BUILD_KIND:");
  assert.notEqual(leaked, workflow);
  assert.ok(lintWorkflow(leaked).some((p) => p.includes("job-level env")));
});

test("attest, verify-provenance and publish-draft must stay official-only", () => {
  for (const job of ["attest", "verify-provenance", "publish-draft"]) {
    const removed = workflow.replace(new RegExp(`(\\n  ${job}:\\n)    if: needs.gate.outputs.official == 'true'\\n`), "$1");
    assert.notEqual(removed, workflow, job);
    assert.ok(lintWorkflow(removed).some((p) => p.includes(job) && p.includes("official")), job);
  }
});

test("a manual run cannot be turned into an official build", () => {
  const dispatchOfficial = workflow.replace('echo "official=false" >> "$GITHUB_OUTPUT"', 'echo "official=true" >> "$GITHUB_OUTPUT"');
  assert.ok(lintWorkflow(dispatchOfficial).some((p) => p.includes("official=true")));
  const noTagCheck = workflow.replace(/\n\s*target\/debug\/release-gate check-tag[^\n]*\n[^\n]*\n/, "\n");
  assert.notEqual(noTagCheck, workflow);
  assert.ok(lintWorkflow(noTagCheck).some((p) => p.includes("official=true")));
});

test("a run script may embed only matrix values", () => {
  const injected = workflow.replace('echo "::notice::Dry run', 'echo "${{ github.ref_name }} ::notice::Dry run');
  assert.notEqual(injected, workflow);
  assert.ok(lintWorkflow(injected).some((p) => p.includes("run script embeds")));
  assert.deepEqual(lintWorkflow(workflow).filter((p) => p.includes("matrix")), []);
});

test("the stage jobs are wired in and run for dry runs", () => {
  assert.deepEqual(lintStages(workflow), []);
  for (const job of ["artifact-scan", "notices-sbom", "fuzz-gate", "release-gate", "channel-manifests"]) {
    const gated = workflow.replace(new RegExp(`(\\n  ${job}:\\n)`), "$1    if: needs.gate.outputs.official == 'true'\n");
    assert.notEqual(gated, workflow, job);
    assert.ok(lintStages(gated).some((p) => p.includes(job) && p.includes("dry runs")), job);
    const removed = workflow.replace(new RegExp(`\\n  ${job}:\\n`), "\n  renamed-job:\n");
    assert.ok(lintStages(removed).some((p) => p.includes(`${job} is missing`)), job);
  }
});

test("the stage jobs keep running their tools", () => {
  assert.ok(lintStages(workflow.replace("release-gate scan-artifacts", "true")).some((p) => p.includes("scan-artifacts")));
  assert.ok(lintStages(workflow.replaceAll("release-gate check-sboms", "true")).some((p) => p.includes("check-sboms")));
  assert.ok(lintStages(workflow.replaceAll("--label fuzz-crash", "")).some((p) => p.includes("fuzz-crash")));
  assert.ok(lintStages(workflow.replaceAll("build-channel-manifests", "true")).some((p) => p.includes("build-channel-manifests")));
});

test("the build cells run the workspace tests and report release.test_failed", () => {
  assert.ok(lintStages(workflow.replaceAll("cargo test --locked --workspace --all-features", "true")).some((p) => p.includes("release.test_failed")));
  assert.ok(lintStages(workflow.replaceAll("release.test_failed", "release.tests_failed")).length > 0);
});

test("inventory, attest and publish-draft wait for the stage jobs", () => {
  assert.ok(lintStages(workflow.replace("needs: [gate, build, package-dmg, artifact-scan, notices-sbom, fuzz-gate]", "needs: [gate, build, package-dmg]")).some((p) => p.includes("inventory must need")));
  assert.ok(lintStages(workflow.replace("needs: [gate, inventory, release-gate]", "needs: [gate, inventory]")).some((p) => p.includes("attest must need release-gate")));
  assert.ok(lintStages(workflow.replace("needs: [gate, inventory, release-gate, attest, verify-provenance]", "needs: [gate, inventory, attest, verify-provenance]")).some((p) => p.includes("publish-draft must need release-gate")));
});

test("a comment never satisfies a stage check", () => {
  const commented = workflow.replace(/^( +)(target\/debug\/release-gate scan-artifacts)/m, "$1# $2");
  assert.notEqual(commented, workflow);
  assert.ok(lintStages(commented).some((p) => p.includes("artifact-scan must run release-gate scan-artifacts")));
  const noTest = workflow.replaceAll("cargo test --locked --workspace --all-features", "# cargo test --locked --workspace --all-features");
  assert.ok(lintStages(noTest).some((p) => p.includes("build must run cargo test")));
});

test("a stage job may not swallow a failure", () => {
  const lenient = workflow.replace("  artifact-scan:\n", "  artifact-scan:\n    continue-on-error: true\n");
  assert.ok(lintStages(lenient).some((p) => p.includes("artifact-scan must not swallow")));
  const orTrue = workflow.replace("cargo build --locked -p du-release-tools --bin release-gate --bin build-sbom", "cargo build --locked -p du-release-tools --bin release-gate --bin build-sbom || true");
  assert.ok(lintStages(orTrue).some((p) => p.includes("notices-sbom must not swallow")));
});

test("the build record says the tests passed", () => {
  assert.ok(lintStages(workflow.replace("tested: true", "tested: false")).some((p) => p.includes("tested: true")));
});

const fuzzWorkflow = readFileSync(fileURLToPath(new URL("../../.github/workflows/fuzz.yml", import.meta.url)), "utf8");

test("fuzz.yml satisfies the policy", () => {
  assert.deepEqual(lintFuzzWorkflow(fuzzWorkflow), []);
});

test("fuzz.yml keeps its triggers, permissions and pins", () => {
  assert.ok(lintFuzzWorkflow(fuzzWorkflow.replace("on:\n", "on:\n  pull_request:\n")).some((p) => p.includes("pull_request")));
  assert.ok(lintFuzzWorkflow(fuzzWorkflow.replace("  fuzz:\n", "  fuzz:\n    permissions:\n      issues: write\n")).some((p) => p.includes("job fuzz must not write issues")));
  assert.ok(lintFuzzWorkflow(fuzzWorkflow.replace("      issues: write\n", "      issues: write\n      contents: write\n")).some((p) => p.includes("must not write contents")));
  assert.ok(lintFuzzWorkflow(fuzzWorkflow.replace(/actions\/cache@[0-9a-f]{40}/, "actions/cache@v4")).some((p) => p.includes("not pinned")));
  assert.ok(lintFuzzWorkflow(fuzzWorkflow.replace("    steps:\n      - name: File the crash issue", "    steps:\n      - uses: actions/checkout@3d3c42e5aac5ba805825da76410c181273ba90b1 # v7.0.1\n      - name: File the crash issue")).some((p) => p.includes("must not check out")));
  assert.ok(lintFuzzWorkflow(fuzzWorkflow.replace('RUN_URL: ${{ github.server_url }}', 'RUN_URL: x')
    .replace('run: |\n          set -euo pipefail\n          gh label', 'run: |\n          echo "${{ github.head_ref }}"\n          gh label')).some((p) => p.includes("run script embeds")));
});

test("the crash issue is also filed when the fuzz job fails without a crash file", () => {
  assert.ok(fuzzWorkflow.includes("needs.fuzz.result == 'failure'"));
});
