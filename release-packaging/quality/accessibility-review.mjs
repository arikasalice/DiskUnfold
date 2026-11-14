import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { join, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

// NFR3.HD1.gui-web.DD1: the product accessibility review list and the check of its release results.
// The measure is the number of (task, combination) pairs that an input means kept from completing.
// The pass threshold is 0 and an "unverified" cell never counts as a pass.

const CHECKLIST_PATH = fileURLToPath(new URL("./accessibility-review.json", import.meta.url));
const RESULT_VALUES = ["pass", "fail", "unverified", "not_applicable"];
const REQUIRED_CHECKS = [
  "name-role-state", "focus-visible-order", "color-independent",
  "composition-no-execute", "truncated-text-full", "primary-action-reachable",
];
const REQUIRED_AXES = {
  language: ["ko", "en"],
  keyboardLayout: ["ko-2set", "en-us"],
  appScale: [150, 175, 200],
  window: ["1024x700"],
};
const REQUIRED_SYSTEMS = ["windows", "macos", "linux"];
const AXIS_NAMES = ["assistiveTechnology", "language", "keyboardLayout", "appScale", "window"];

const isText = (value) => typeof value === "string" && value.trim() !== "";
const sameSet = (actual, expected) =>
  Array.isArray(actual) && actual.length === expected.length && expected.every((value) => actual.includes(value));

function duplicates(values) {
  return values.filter((value, index) => values.indexOf(value) !== index);
}

/** Returns every combination of the five axes in a stable order. */
export function combinations(axes) {
  const rows = [];
  for (const assistiveTechnology of axes.assistiveTechnology.map(({ id }) => id)) {
    for (const language of axes.language) {
      for (const keyboardLayout of axes.keyboardLayout) {
        for (const appScale of axes.appScale) {
          for (const window of axes.window) {
            rows.push({ assistiveTechnology, language, keyboardLayout, appScale, window });
          }
        }
      }
    }
  }
  return rows;
}

const combinationKey = (value) => AXIS_NAMES.map((axis) => String(value?.[axis])).join("|");

/** Returns a list of problems with the checklist definition; an empty list means it is valid. */
export function validateChecklist(list) {
  const errors = [];
  if (list?.schema !== "diskunfold.accessibility-review" || list.schemaVersion !== 1) {
    errors.push("Unsupported checklist schema");
    return errors;
  }
  if (list.measure?.passThreshold !== 0 || list.measure?.unverifiedCountsAsPass !== false) {
    errors.push("The measure must have pass threshold 0 and must not count unverified as a pass");
  }
  const tasks = Array.isArray(list.tasks) ? list.tasks : [];
  if (tasks.length === 0) errors.push("The checklist needs at least one task row");
  for (const task of tasks) {
    if (!/^[a-z][a-z0-9-]*$/.test(task?.id ?? "") || !isText(task.label) ||
        !/^F\d{2}$/.test(task.ownerFeature ?? "") ||
        typeof task.entryPoint?.present !== "boolean" || !isText(task.entryPoint?.evidence)) {
      errors.push(`Invalid task row: ${JSON.stringify(task?.id)}`);
    }
  }
  for (const id of duplicates(tasks.map((task) => task?.id))) errors.push(`Duplicate task: ${id}`);

  const axes = list.axes ?? {};
  const systems = Array.isArray(axes.assistiveTechnology) ? axes.assistiveTechnology : [];
  if (!sameSet(systems.map((row) => row?.os), REQUIRED_SYSTEMS) ||
      systems.some((row) => !isText(row?.id) || !isText(row?.name))) {
    errors.push("The assistive technology axis must cover windows, macos and linux once each");
  }
  for (const id of duplicates(systems.map((row) => row?.id))) errors.push(`Duplicate assistive technology: ${id}`);
  for (const [axis, expected] of Object.entries(REQUIRED_AXES)) {
    if (!sameSet(axes[axis], expected)) errors.push(`The ${axis} axis must be exactly: ${expected.join(", ")}`);
  }

  const checks = Array.isArray(list.checks) ? list.checks : [];
  if (!sameSet(checks.map((check) => check?.id), REQUIRED_CHECKS) || checks.some((check) => !isText(check?.label))) {
    errors.push(`The check items must be exactly: ${REQUIRED_CHECKS.join(", ")}`);
  }
  return errors;
}

/** Evaluates a results file against a valid checklist. */
export function evaluate(list, results) {
  const errors = [];
  const tasks = new Map(list.tasks.map((task) => [task.id, task]));
  const checks = new Set(list.checks.map(({ id }) => id));
  const allCombinations = combinations(list.axes);
  const validCombinations = new Set(allCombinations.map(combinationKey));
  const expectedCells = tasks.size * checks.size * allCombinations.length;

  if (results?.schema !== list.resultFormat.schema || results.schemaVersion !== list.resultFormat.schemaVersion ||
      !Array.isArray(results.records)) {
    return { errors: ["Unsupported results schema"], summary: null };
  }
  const seen = new Set();
  const counts = Object.fromEntries(RESULT_VALUES.map((value) => [value, 0]));
  const blocked = new Map();
  const productVersions = new Set();
  results.records.forEach((record, index) => {
    const where = `record ${index}`;
    const task = tasks.get(record?.task);
    const cell = `${record?.task}|${combinationKey(record?.combination)}|${record?.check}`;
    let valid = true;
    const fail = (message) => { valid = false; errors.push(`${where}: ${message}`); };
    if (!task) fail(`unknown task ${JSON.stringify(record?.task)}`);
    if (!checks.has(record?.check)) fail(`unknown check ${JSON.stringify(record?.check)}`);
    if (!validCombinations.has(combinationKey(record?.combination))) fail("unknown combination");
    if (!RESULT_VALUES.includes(record?.result)) fail(`unknown result ${JSON.stringify(record?.result)}`);
    if (!isText(record?.productVersion) || !isText(record?.osVersion)) fail("product and OS versions are required");
    if (record?.result !== "unverified" && !isText(record?.evidence)) fail("evidence is required");
    if (record?.result === "fail" && typeof record.blocksTask !== "boolean") fail("a failure must say whether it blocks the task");
    if (record?.result !== "fail" && record?.blocksTask !== undefined) fail("blocksTask is only allowed on a failure");
    if (task && !task.entryPoint.present && !["not_applicable", "unverified", "fail"].includes(record?.result)) {
      fail(`task ${task.id} has no entry point in the checklist, so it cannot pass`);
    }
    if (valid && seen.has(cell)) fail("duplicate task, combination and check");
    if (!valid) return;
    seen.add(cell);
    productVersions.add(record.productVersion);
    counts[record.result] += 1;
    if (record.result === "fail" && record.blocksTask) {
      const key = `${record.task}|${combinationKey(record.combination)}`;
      const entry = blocked.get(key) ?? { task: record.task, combination: record.combination, checks: [] };
      entry.checks.push(record.check);
      blocked.set(key, entry);
    }
  });
  if (productVersions.size > 1) errors.push("A results file covers one product version");

  const recorded = seen.size;
  const missing = expectedCells - recorded;
  const summary = {
    productVersion: productVersions.size === 1 ? [...productVersions][0] : null,
    expectedCells,
    recordedCells: recorded,
    missingCells: missing,
    counts,
    measure: { id: list.measure.id, value: blocked.size, threshold: list.measure.passThreshold },
    blocked: [...blocked.values()],
    // Unverified and missing cells are not passes.
    notPassed: counts.unverified + missing,
  };
  summary.verdict = errors.length > 0 ? "invalid"
    : blocked.size > list.measure.passThreshold || counts.fail > 0 ? "fail"
    : summary.notPassed > 0 ? "incomplete"
    : "pass";
  return { errors, summary };
}

/** Builds a results file with every cell present and unverified. */
export function template(list, productVersion, osVersion) {
  const records = [];
  for (const task of list.tasks) {
    for (const combination of combinations(list.axes)) {
      for (const { id } of list.checks) {
        records.push({ task: task.id, combination, check: id, result: "unverified", productVersion, osVersion, evidence: "" });
      }
    }
  }
  return { schema: list.resultFormat.schema, schemaVersion: list.resultFormat.schemaVersion, records };
}

function main(argv) {
  const root = process.cwd();
  const report = { status: "invalid", errors: [] };
  const output = (value) => {
    const directory = join(root, "quality-output");
    mkdirSync(directory, { recursive: true });
    writeFileSync(join(directory, "accessibility-review-report.json"), JSON.stringify(value, null, 2) + "\n");
    console.log(JSON.stringify(value, null, 2));
  };
  try {
    const list = JSON.parse(readFileSync(CHECKLIST_PATH, "utf8"));
    const problems = validateChecklist(list);
    if (argv[0] === "--template") {
      if (problems.length > 0) throw new Error(problems.join("; "));
      const [, productVersion, osVersion] = argv;
      if (!isText(productVersion) || !isText(osVersion)) throw new Error("Usage: --template <productVersion> <osVersion>");
      console.log(JSON.stringify(template(list, productVersion, osVersion), null, 2));
      return;
    }
    if (problems.length > 0) {
      report.errors = problems;
    } else if (argv[0] === "--results") {
      if (!argv[1]) throw new Error("Usage: --results <results.json>");
      const { errors, summary } = evaluate(list, JSON.parse(readFileSync(resolve(root, argv[1]), "utf8")));
      Object.assign(report, summary ?? {}, { status: summary?.verdict ?? "invalid", errors });
    } else {
      // Merge gate: the definition is valid; no release results exist yet, so the measure is not available.
      Object.assign(report, {
        status: "validated", errors: [], measure: { id: list.measure.id, value: null, threshold: 0 },
        tasks: list.tasks.length, checks: list.checks.length, combinations: combinations(list.axes).length,
        tasksWithoutEntryPoint: list.tasks.filter((task) => !task.entryPoint.present).map(({ id }) => id),
      });
    }
  } catch (error) {
    report.errors = [error.message];
  }
  output(report);
  if (report.status !== "validated" && report.status !== "pass") process.exitCode = 1;
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) main(process.argv.slice(2));
