import assert from "node:assert/strict";
import { mkdtempSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { classifyAttestation, expectedDigest, expectedPolicy, sha256File } from "./verify-download.mjs";

const DIGEST = "a".repeat(64);

test("expectedDigest reads the sha256sum format by exact file name", () => {
  const sums = `${DIGEST}  DiskUnfold-0.0.1-x64.exe\n${"b".repeat(64)} *other.deb\n`;
  assert.equal(expectedDigest(sums, "DiskUnfold-0.0.1-x64.exe"), DIGEST);
  assert.equal(expectedDigest(sums, "other.deb"), "b".repeat(64));
  assert.equal(expectedDigest(sums, "DiskUnfold-0.0.1-x64"), undefined);
  assert.equal(expectedDigest("not a checksum line", "x"), undefined);
});

test("gh results map to the three published outcomes", () => {
  assert.equal(classifyAttestation(0, ""), "verified");
  assert.equal(classifyAttestation(1, "no attestations found for subject"), "mismatch");
  assert.equal(classifyAttestation(1, "verification failed: signer workflow mismatch"), "mismatch");
  assert.equal(classifyAttestation(1, "dial tcp: lookup api.github.com: no such host"), "unverifiable");
  assert.equal(classifyAttestation(null, ""), "unverifiable");
});

test("sha256File hashes bytes", () => {
  const dir = mkdtempSync(path.join(tmpdir(), "verify-download-"));
  const file = path.join(dir, "f");
  writeFileSync(file, "abc");
  assert.equal(sha256File(file), "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad");
});

test("the expected builder id is the signer workflow ref the release workflow verifies", () => {
  const policy = expectedPolicy("owner/repo", "v1.2.3");
  assert.equal(policy.builderId, "https://github.com/owner/repo/.github/workflows/release.yml@refs/tags/v1.2.3");
  assert.equal(policy.workflowPath, ".github/workflows/release.yml");
  assert.equal(policy.eventName, "push");
  const workflow = readFileSync(new URL("../../.github/workflows/release.yml", import.meta.url), "utf8");
  assert.ok(workflow.includes('"https://github.com/$REPOSITORY/.github/workflows/release.yml@refs/tags/$TAG"'));
  assert.ok(workflow.includes(".github/workflows/release.yml push"));
});
