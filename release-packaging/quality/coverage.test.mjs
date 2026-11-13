import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdirSync, mkdtempSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

const runner = fileURLToPath(new URL("./coverage.mjs", import.meta.url));
const baseCommit = "1".repeat(40);
const currentCommit = "2".repeat(40);

function fixture(t) {
  const root = mkdtempSync(join(tmpdir(), "diskunfold-coverage-"));
  t.after(() => rmSync(root, { recursive: true, force: true }));

  function write(file, value) {
    const path = join(root, file);
    mkdirSync(dirname(path), { recursive: true });
    writeFileSync(path, value);
  }
  write("rust-toolchain.toml", '[toolchain]\nchannel = "1.99.1"\n');
  write(".node-version", "26.13.0\n");
  write("release-packaging/quality/rust-coverage.mjs", "// fixture\n");
  write("gui-web/vite.config.ts", "// fixture\n");
  write("gui-web/package-lock.json", JSON.stringify({
    packages: {
      "node_modules/vitest": { version: "5.1.0" },
      "node_modules/@vitest/coverage-v8": { version: "5.1.0" },
    },
  }));
  write("quality-output/rust-tests.log", "test result: ok. 1 passed; 0 failed; 0 ignored;\n");
  write("quality-output/gui-tests.json", '{"numTotalTests":1,"numPassedTests":1}');
  const env = {
    ...process.env, GITHUB_SHA: baseCommit, GITHUB_RUN_ID: "123", GITHUB_RUN_ATTEMPT: "2",
    GITHUB_STEP_SUMMARY: join(root, "job-summary.md"), COVERAGE_BASE_COMMIT: "", COVERAGE_BASE_RUN: "",
    RUST_COVERAGE_SUCCEEDED: "true", GUI_COVERAGE_SUCCEEDED: "true",
  };

  function lcov(rust, gui) {
    write("quality-output/rust-lcov.info", `LF:10\nLH:${rust}\nFNF:2\nFNH:1\n`);
    write("gui-web/coverage/lcov.info", `LF:10\nLH:${gui}\nFNF:2\nFNH:1\n`);
  }
  function run(baseline, overrides = {}) {
    const result = spawnSync(process.execPath, [runner, ...(baseline ? [baseline] : [])], {
      cwd: root, env: { ...env, ...overrides }, encoding: "utf8",
    });
    assert.equal(result.status, 0, result.stderr);
    return JSON.parse(readFileSync(join(root, "quality-output/coverage-summary.json"), "utf8"));
  }
  return { lcov, run, write, root };
}

test("an exact base reports an increase and decrease without failing", (t) => {
  const { lcov, run, write, root } = fixture(t);
  lcov(8, 5);
  const base = run();
  assert.equal(base.baseline.status, "unavailable");
  write("base.json", JSON.stringify(base));
  lcov(6, 6);
  const current = run("base.json", {
    GITHUB_SHA: currentCommit, GITHUB_RUN_ID: "456",
    COVERAGE_BASE_COMMIT: baseCommit, COVERAGE_BASE_RUN: "123",
  });
  assert.equal(current.source.commit, currentCommit);
  assert.equal(current.baseline.commit, baseCommit);
  assert.equal(current.baseline.attempt, "2");
  assert.equal(current.results.rust.lineDecreased, true);
  assert.equal(current.results.gui.lineDecreased, false);
  assert.match(readFileSync(join(root, "job-summary.md"), "utf8"), /\| rust \| 60\.00% \| -20\.00 \|/);
});

test("different commit, run, or measurement settings cannot become a baseline", (t) => {
  const { lcov, run, write } = fixture(t);
  lcov(8, 8);
  const base = run();
  for (const overrides of [
    { COVERAGE_BASE_COMMIT: currentCommit },
    { COVERAGE_BASE_RUN: "124" },
    { CARGO_LLVM_COV_VERSION: "0.0.0" },
  ]) {
    write("base.json", JSON.stringify(base));
    const current = run("base.json", {
      COVERAGE_BASE_COMMIT: baseCommit, COVERAGE_BASE_RUN: "123", ...overrides,
    });
    assert.equal(current.baseline.status, "unavailable");
    assert.equal(current.results.rust.lineDelta, null);
  }
});

test("failed producer or inconsistent baseline counts suppress only the affected comparison", (t) => {
  const { lcov, run, write } = fixture(t);
  lcov(8, 8);
  const base = run();
  base.results.gui.counts.linesHit = 10;
  write("base.json", JSON.stringify(base));
  lcov(6, 6);
  const expected = { COVERAGE_BASE_COMMIT: baseCommit, COVERAGE_BASE_RUN: "123" };
  const current = run("base.json", expected);
  assert.equal(current.results.rust.lineDecreased, true);
  assert.equal(current.results.gui.lineDelta, null);
  const failed = run("base.json", { ...expected, RUST_COVERAGE_SUCCEEDED: "false" });
  assert.equal(failed.results.rust.comparisonEligible, false);
  assert.equal(failed.results.rust.lineDelta, null);
});

test("failed tests retain measured coverage and executed test counts", (t) => {
  const { lcov, run, write } = fixture(t);
  lcov(8, 6);
  write("quality-output/rust-tests.log", [
    "test result: FAILED. 0 passed; 1 failed; 1 ignored;",
    "test result: ok. 1 passed; 0 failed; 0 ignored;",
  ].join("\n"));
  write("quality-output/gui-tests.json", '{"numTotalTests":2,"numPassedTests":0,"numPendingTests":1}');
  const { results } = run(undefined, {
    RUST_COVERAGE_SUCCEEDED: "false", GUI_COVERAGE_SUCCEEDED: "false",
  });
  assert.equal(results.rust.counts.tests, 2);
  assert.equal(results.rust.tests, 0.5);
  assert.equal(results.rust.tem, 1.25);
  assert.equal(results.gui.counts.tests, 1);
  assert.equal(results.gui.tem, 0);
  for (const result of Object.values(results)) {
    assert.equal(result.status, "measured");
    assert.equal(result.comparisonEligible, false);
  }
});
