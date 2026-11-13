import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import { fileURLToPath } from "node:url";
import { lintWorkflow, pinnedActions, verifyRemote } from "./lint-workflow.mjs";

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
  const problems = lintWorkflow(workflow.replace(/(actions\/checkout@[0-9a-f]{40}) # v4.2.2/, "$1"));
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
