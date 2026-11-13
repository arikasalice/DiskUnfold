import assert from "node:assert/strict";
import { mkdtempSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import test from "node:test";
import { classifyAttestation, expectedDigest, sha256File } from "./verify-download.mjs";

const DIGEST = "a".repeat(64);

test("expectedDigest reads the sha256sum format by exact file name", () => {
  const sums = `${DIGEST}  DiskUnfold-0.1.0-x64.exe\n${"b".repeat(64)} *other.deb\n`;
  assert.equal(expectedDigest(sums, "DiskUnfold-0.1.0-x64.exe"), DIGEST);
  assert.equal(expectedDigest(sums, "other.deb"), "b".repeat(64));
  assert.equal(expectedDigest(sums, "DiskUnfold-0.1.0-x64"), undefined);
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
