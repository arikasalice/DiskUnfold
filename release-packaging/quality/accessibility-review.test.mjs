import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import { mkdtempSync, readFileSync, realpathSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { fileURLToPath } from "node:url";
import test from "node:test";

import { combinations, evaluate, template, validateChecklist } from "./accessibility-review.mjs";

const script = fileURLToPath(new URL("./accessibility-review.mjs", import.meta.url));
const checklist = JSON.parse(readFileSync(new URL("./accessibility-review.json", import.meta.url), "utf8"));

function passing() {
  const results = template(checklist, "0.1.0", "Windows 11 24H2");
  for (const record of results.records) {
    const task = checklist.tasks.find(({ id }) => id === record.task);
    record.result = task.entryPoint.present ? "pass" : "not_applicable";
    record.evidence = task.entryPoint.present ? "review-record-1" : "no entry point in this release";
  }
  return results;
}

function invoke(t, args, results) {
  const root = realpathSync(mkdtempSync(join(tmpdir(), "diskunfold-a11y-")));
  t.after(() => rmSync(root, { recursive: true, force: true }));
  if (results) writeFileSync(join(root, "results.json"), JSON.stringify(results));
  const run = spawnSync(process.execPath, [script, ...args], { cwd: root, encoding: "utf8" });
  const report = JSON.parse(readFileSync(join(root, "quality-output/accessibility-review-report.json"), "utf8"));
  return { ...run, report };
}

test("the shipped checklist is valid and a complete passing review gives measure 0", (t) => {
  assert.deepEqual(validateChecklist(checklist), []);
  assert.equal(combinations(checklist.axes).length, 36);
  assert.equal(checklist.tasks.length, 8);

  const merge = invoke(t, ["--check"]);
  assert.equal(merge.status, 0, merge.stderr);
  assert.equal(merge.report.status, "validated");
  assert.equal(merge.report.measure.value, null, "no release results means no measurement");

  const review = invoke(t, ["--results", "results.json"], passing());
  assert.equal(review.status, 0, review.stdout);
  assert.equal(review.report.status, "pass");
  assert.equal(review.report.measure.value, 0);
  assert.equal(review.report.missingCells, 0);
  assert.equal(review.report.expectedCells, 8 * 6 * 36);
});

test("a blocking failure is counted once per task and combination and fails the review", (t) => {
  const results = passing();
  const [first, second] = results.records.filter(
    (record) => record.task === "browse" && record.combination.language === "ko" &&
      record.combination.assistiveTechnology === "ubuntu-orca" && record.combination.appScale === 200,
  );
  Object.assign(first, { result: "fail", blocksTask: true, evidence: "orca-note-7" });
  Object.assign(second, { result: "fail", blocksTask: true, evidence: "orca-note-8" });
  const review = invoke(t, ["--results", "results.json"], results);
  assert.equal(review.status, 1);
  assert.equal(review.report.status, "fail");
  assert.equal(review.report.measure.value, 1);
  assert.equal(review.report.blocked[0].checks.length, 2);
});

test("unverified and missing cells are not passes", (t) => {
  const results = passing();
  results.records[0].result = "unverified";
  results.records.pop();
  const review = invoke(t, ["--results", "results.json"], results);
  assert.equal(review.status, 1);
  assert.equal(review.report.status, "incomplete");
  assert.equal(review.report.measure.value, 0);
  assert.equal(review.report.counts.unverified, 1);
  assert.equal(review.report.missingCells, 1);
  assert.equal(review.report.notPassed, 2);
});

test("a results file that breaks the record format is invalid", () => {
  const cases = {
    "unknown task": (results) => { results.records[0].task = "delete-everything"; },
    "unknown result": (results) => { results.records[0].result = "ok"; },
    "evidence missing": (results) => { results.records[0].evidence = " "; },
    "OS version missing": (results) => { results.records[0].osVersion = ""; },
    "failure without blocksTask": (results) => { Object.assign(results.records[0], { result: "fail" }); },
    "duplicate cell": (results) => { results.records.push({ ...results.records[0] }); },
    "mixed product versions": (results) => { results.records[1].productVersion = "0.2.0"; },
    "pass on a task without an entry point": (results) => {
      const record = results.records.find(({ task }) => task === "approve");
      record.result = "pass";
    },
  };
  for (const [name, mutate] of Object.entries(cases)) {
    const results = passing();
    mutate(results);
    const { errors, summary } = evaluate(checklist, results);
    assert.ok(errors.length > 0, name);
    assert.equal(summary?.verdict ?? "invalid", "invalid", name);
  }
});

test("a checklist that drops a check item, an axis value or the zero threshold is rejected", () => {
  const edits = {
    "check item": (copy) => { copy.checks.pop(); },
    "axis value": (copy) => { copy.axes.appScale = [150, 200]; },
    "assistive technology": (copy) => { copy.axes.assistiveTechnology.pop(); },
    "threshold": (copy) => { copy.measure.passThreshold = 1; },
    "unverified as pass": (copy) => { copy.measure.unverifiedCountsAsPass = true; },
    "owner feature": (copy) => { copy.tasks[0].ownerFeature = "scan"; },
    "entry point flag": (copy) => { delete copy.tasks[0].entryPoint.present; },
  };
  for (const [name, edit] of Object.entries(edits)) {
    const copy = structuredClone(checklist);
    edit(copy);
    assert.ok(validateChecklist(copy).length > 0, name);
  }
});
