import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";

const target = 0.8;
const [, , baselinePath] = process.argv;
const baseline = baselinePath ? JSON.parse(readFileSync(baselinePath, "utf8")).results : {};
const inputs = {
  rust: "quality-output/rust-lcov.info",
  gui: "gui-web/coverage/lcov.info",
};

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
for (const [name, file] of Object.entries(inputs)) {
  const counts = { ...readLcov(file), ...readTests(name) };
  const line = ratio(counts.linesHit, counts.lines);
  const functions = ratio(counts.functionsHit, counts.functions);
  const tests = ratio(counts.testsPassed, counts.tests);
  const measured = line !== null && functions !== null && tests !== null;
  const previous = baseline?.[name]?.line;
  const delta = line !== null && typeof previous === "number" ? line - previous : null;
  results[name] = {
    status: measured ? "measured" : "unavailable",
    line,
    function: functions,
    tests,
    tem: measured ? 5 * Math.min(line / target, 1) * functions * tests : null,
    lineTargetMet: line === null ? null : line >= target,
    lineDelta: delta,
    lineDecreased: delta === null ? null : delta < 0,
    counts,
  };
}

mkdirSync("quality-output", { recursive: true });
writeFileSync(
  "quality-output/coverage-summary.json",
  JSON.stringify({ target, enforcement: "report-only", results }, null, 2) + "\n",
);
console.log(JSON.stringify(results, null, 2));
