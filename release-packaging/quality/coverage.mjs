import { appendFileSync, existsSync, mkdirSync, readFileSync, statSync, writeFileSync } from "node:fs";
import { createHash } from "node:crypto";

const target = 0.8;
const [, , baselinePath] = process.argv;
const commitPattern = /^[a-f0-9]{40}$/;
const expectedBase = process.env.COVERAGE_BASE_COMMIT || null;
const expectedRun = process.env.COVERAGE_BASE_RUN || null;
const guiLock = JSON.parse(readFileSync("gui-web/package-lock.json", "utf8")).packages;
const method = createHash("sha256");
for (const path of [
  "rust-toolchain.toml", ".node-version", "release-packaging/quality/rust-coverage.mjs",
  "gui-web/vite.config.ts",
]) method.update(readFileSync(path)).update("\0");
for (const tool of ["vitest", "@vitest/coverage-v8"]) {
  method.update(guiLock[`node_modules/${tool}`].version).update("\0");
}
method.update(process.env.CARGO_LLVM_COV_VERSION ?? "0.9.1");
const source = {
  commit: commitPattern.test(process.env.GITHUB_SHA ?? "") ? process.env.GITHUB_SHA : null,
  run: process.env.GITHUB_RUN_ID ?? null,
  attempt: process.env.GITHUB_RUN_ATTEMPT ?? null,
  platform: process.platform,
  arch: process.arch,
  method: method.digest("hex"),
};
const inputs = {
  rust: "quality-output/rust-lcov.info",
  gui: "gui-web/coverage/lcov.info",
};

function readBaseline() {
  if (!baselinePath || !existsSync(baselinePath)) return { status: "unavailable", reason: "no artifact" };
  try {
    if (!statSync(baselinePath).isFile() || statSync(baselinePath).size > 64 * 1024) {
      return { status: "unavailable", reason: "invalid artifact" };
    }
    const report = JSON.parse(readFileSync(baselinePath, "utf8"));
    if (!report?.results || Array.isArray(report.results) || typeof report.results !== "object") {
      return { status: "unavailable", reason: "invalid artifact" };
    }
    if (expectedBase && (!commitPattern.test(expectedBase) ||
        report.schemaVersion !== 1 || report.source?.commit !== expectedBase ||
        (expectedRun && report.source.run !== expectedRun) ||
        report.source.platform !== source.platform || report.source.arch !== source.arch ||
        report.source.method !== source.method)) {
      return { status: "unavailable", reason: "different commit, run, platform or measurement settings" };
    }
    return {
      status: "available", commit: report.source?.commit ?? null, run: report.source?.run ?? null,
      attempt: report.source?.attempt ?? null, results: report.results,
    };
  } catch {
    return { status: "unavailable", reason: "invalid artifact" };
  }
}

function readLcov(file) {
  if (!existsSync(file)) return { lines: 0, linesHit: 0, functions: 0, functionsHit: 0 };
  const data = readFileSync(file, "utf8");
  const sum = (key) =>
    [...data.matchAll(new RegExp(`^${key}:(\\d+)\\r?$`, "gm"))].reduce(
      (total, match) => total + Number(match[1]),
      0,
    );
  return { lines: sum("LF"), linesHit: sum("LH"), functions: sum("FNF"), functionsHit: sum("FNH") };
}

function readTests(name) {
  const file = name === "rust" ? "quality-output/rust-tests.log" : "quality-output/gui-tests.json";
  if (!existsSync(file)) return { tests: 0, testsPassed: 0 };
  const data = readFileSync(file, "utf8");
  if (name === "gui") {
    const t = JSON.parse(data);
    return {
      tests: t.numTotalTests - (t.numPendingTests || 0) - (t.numTodoTests || 0),
      testsPassed: t.numPassedTests,
    };
  }
  let tests = 0;
  let testsPassed = 0;
  for (const match of data.matchAll(/test result: (?:ok|FAILED)\. (\d+) passed; (\d+) failed;/g)) {
    testsPassed += Number(match[1]);
    tests += Number(match[1]) + Number(match[2]);
  }
  return { tests, testsPassed };
}

function ratio(hit, total) {
  return Number.isInteger(hit) && Number.isInteger(total) && total > 0 && hit >= 0 && hit <= total
    ? hit / total
    : null;
}

const results = {};
const baseline = readBaseline();
for (const [name, file] of Object.entries(inputs)) {
  const counts = { ...readLcov(file), ...readTests(name) };
  const line = ratio(counts.linesHit, counts.lines);
  const functions = ratio(counts.functionsHit, counts.functions);
  const tests = ratio(counts.testsPassed, counts.tests);
  const measured = line !== null && functions !== null && tests !== null;
  const producer = process.env[name === "rust" ? "RUST_COVERAGE_SUCCEEDED" : "GUI_COVERAGE_SUCCEEDED"];
  const comparisonEligible = measured && tests === 1 && producer !== "false";
  const previous = baseline.results?.[name];
  const previousEligible = (expectedBase ? previous?.comparisonEligible === true :
    previous?.comparisonEligible !== false) && previous?.status === "measured" && previous?.tests === 1 &&
    Number.isFinite(previous.line) && previous.line >= 0 && previous.line <= 1 &&
    (!expectedBase || ratio(previous.counts?.linesHit, previous.counts?.lines) === previous.line);
  const delta = comparisonEligible && previousEligible ? line - previous.line : null;
  results[name] = {
    status: measured ? "measured" : "unavailable",
    line,
    function: functions,
    tests,
    tem: measured ? 5 * Math.min(line / target, 1) * functions * tests : null,
    lineTargetMet: line === null ? null : line >= target,
    comparisonEligible,
    lineDelta: delta,
    lineDecreased: delta === null ? null : delta < 0,
    counts,
  };
}

mkdirSync("quality-output", { recursive: true });
const { results: _baselineResults, ...baselineSource } = baseline;
writeFileSync(
  "quality-output/coverage-summary.json",
  JSON.stringify({ schemaVersion: 1, source, baseline: baselineSource, target, enforcement: "report-only", results }, null, 2) + "\n",
);
console.log(JSON.stringify(results, null, 2));
if (process.env.GITHUB_STEP_SUMMARY) {
  const lines = [
    "### Line coverage",
    "",
    "| Target | Current | Change (percentage points) |",
    "| --- | ---: | ---: |",
    ...Object.entries(results).map(([name, result]) => {
      const current = result.line === null ? "N/A" : `${(result.line * 100).toFixed(2)}%`;
      const change = result.lineDelta === null ? "N/A" : (result.lineDelta * 100).toFixed(2);
      return `| ${name} | ${current} | ${change} |`;
    }),
  ];
  if (baseline.status === "unavailable") lines.push("", `Baseline unavailable (${baseline.reason}).`);
  appendFileSync(process.env.GITHUB_STEP_SUMMARY, lines.join("\n") + "\n");
}
