import assert from "node:assert/strict";
import test from "node:test";
import { checkProductInfo, checkVersionOutput } from "./check-build-kind.mjs";

const COMMIT = "0123456789abcdef0123456789abcdef01234567";

test("the version output must show the build kind label and the commit", () => {
  const official = `Version: 0.0.1\nBuild type: Official release build\nSource commit: ${COMMIT}\n`;
  assert.deepEqual(checkVersionOutput(official, "official", COMMIT), []);
  assert.equal(checkVersionOutput(official, "developer", COMMIT).length, 1);
  assert.equal(checkVersionOutput(official, "official", "f".repeat(40)).length, 1);
  assert.equal(checkVersionOutput("", "developer", COMMIT).length, 2);
});

test("an official build needs notices and format versions", () => {
  const info = { build: { buildKind: "official", commit: COMMIT, appDbFormat: 1, snapshotFormat: 1, supportMatrixRevision: "r", builtAtUtc: "t" }, notices: { thirdParty: "x" } };
  assert.deepEqual(checkProductInfo(info, "official", COMMIT), []);
  assert.ok(checkProductInfo({ ...info, notices: {} }, "official", COMMIT).length > 0);
});
