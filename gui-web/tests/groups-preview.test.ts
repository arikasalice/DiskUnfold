import { afterEach, beforeEach, expect, test, vi } from "vitest";
import { GroupRuleEditor } from "../src/state/groups-editor";
import { GroupPreview } from "../src/state/groups-preview";
import {
  ARCHIVE,
  MOVIES,
  RESULT,
  deferred,
  fakeClient,
  ruleset,
  standardHandlers,
  summary,
  total,
  totals,
} from "./groups-fixtures";
import type { Body } from "./groups-fixtures";

const DRAFT = "018f0000-0000-7000-8000-0000000000d1";
const DELAY = 400;

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  vi.useRealTimers();
});

/** A classification of a draft: the stored rule set version is zero (F36 DLD). */
function draftSummary(movies: number) {
  const base = summary({ rulesetVersion: 0, rulesetFingerprint: "ee" });
  return {
    ...base,
    classificationRef: DRAFT,
    groups: base.groups.map((row) =>
      row.groupId === MOVIES ? { ...row, memberCount: movies } : row,
    ),
  };
}

function setup(extra: Record<string, (body: Body) => unknown> = {}) {
  const fake = fakeClient(
    standardHandlers({
      "group.classify": (body: Body) => ({
        kind: "summary",
        requestSeq: body.requestSeq,
        summary: draftSummary(9),
      }),
      "group.totals": () =>
        totals({
          classificationRef: DRAFT,
          groups: [
            { groupId: MOVIES, total: total("9437184") },
            { groupId: ARCHIVE, total: total("5242880") },
          ],
        }),
      ...extra,
    }),
  );
  const editor = new GroupRuleEditor(fake.client, "iec", () => {});
  editor.open(ruleset());
  const preview = new GroupPreview(
    fake.client,
    "iec",
    editor,
    () => ({ resultVersion: RESULT, scope: undefined }),
    DELAY,
    "view-preview",
  );
  const keys = () => editor.getSnapshot().drafts.map((rule) => rule.key);
  const classifies = () =>
    fake.calls.filter((call) => call.name === "group.classify");
  return { fake, editor, preview, keys, classifies };
}

test("F36 FR3: a changed draft is classified on its own view channel after typing pauses, and nothing is saved", async () => {
  const { fake, editor, preview, keys, classifies } = setup();
  expect(preview.getSnapshot().phase).toBe("applied");
  editor.rename(keys()[0]!, "Films");
  expect(preview.getSnapshot().phase).toBe("waiting");
  await vi.advanceTimersByTimeAsync(DELAY - 1);
  expect(classifies()).toHaveLength(0);
  await vi.advanceTimersByTimeAsync(1);
  expect(classifies()).toHaveLength(1);
  const body = classifies()[0]!.body;
  expect(body.viewRef).toBe("view-preview");
  expect(body.resultVersion).toBe(RESULT);
  expect(body.rules.kind).toBe("draft");
  expect(body.rules.rules).toHaveLength(2);
  expect(body.rules.rules[0]).toMatchObject({
    groupId: MOVIES,
    displayName: "Films",
  });
  const state = preview.getSnapshot();
  expect(state.phase).toBe("ready");
  expect(state.result?.keys).toEqual(keys());
  expect(state.result?.summary.groups[0]?.memberCount).toBe(9);
  expect(state.result?.totals?.groups).toHaveLength(2);
  // The preview never stores anything and leaves the editor as it was.
  expect(fake.calls.some((call) => call.name === "group.rules.save")).toBe(
    false,
  );
  expect(editor.getSnapshot().phase).toBe("editing");
});

test("F36 FR3: typing keeps postponing the request; one request follows the last change", async () => {
  const { editor, preview, keys, classifies } = setup();
  const key = keys()[0]!;
  for (const name of ["F", "Fi", "Fil", "Film"]) {
    editor.rename(key, name);
    await vi.advanceTimersByTimeAsync(DELAY - 100);
  }
  expect(classifies()).toHaveLength(0);
  await vi.advanceTimersByTimeAsync(100);
  expect(classifies()).toHaveLength(1);
  expect(classifies()[0]!.body.rules.rules[0].displayName).toBe("Film");
  expect(preview.getSnapshot().phase).toBe("ready");
});

test("F36 FR3: a late answer to an earlier draft is dropped", async () => {
  const first = deferred<unknown>();
  let calls = 0;
  const { editor, preview, keys, classifies } = setup({
    "group.classify": (body: Body) => {
      calls += 1;
      if (calls === 1) return first.promise;
      return {
        kind: "summary",
        requestSeq: body.requestSeq,
        summary: draftSummary(5),
      };
    },
  });
  const key = keys()[0]!;
  editor.rename(key, "Films");
  await vi.advanceTimersByTimeAsync(DELAY);
  expect(classifies()).toHaveLength(1);
  expect(preview.getSnapshot().phase).toBe("computing");
  // The draft changes while the first request is still running.
  editor.rename(key, "Movies 2");
  await vi.advanceTimersByTimeAsync(DELAY);
  expect(classifies()).toHaveLength(2);
  expect(preview.getSnapshot().result?.summary.groups[0]?.memberCount).toBe(5);
  // The first answer arrives last and must not replace the newer one.
  first.resolve({
    kind: "summary",
    requestSeq: classifies()[0]!.body.requestSeq,
    summary: draftSummary(99),
  });
  await vi.advanceTimersByTimeAsync(0);
  expect(preview.getSnapshot().phase).toBe("ready");
  expect(preview.getSnapshot().result?.summary.groups[0]?.memberCount).toBe(5);
});

test("F36 FR3: an input problem blocks the request; a draft equal to the stored rules needs none", async () => {
  const { editor, preview, keys, classifies } = setup();
  const key = keys()[0]!;
  editor.addRow(key, "name");
  expect(editor.getSnapshot().blocker).toBe("empty_value");
  expect(preview.getSnapshot().phase).toBe("blocked");
  await vi.advanceTimersByTimeAsync(DELAY * 3);
  expect(classifies()).toHaveLength(0);
  editor.removeRow(key, 1);
  expect(editor.getSnapshot().dirty).toBe(false);
  expect(preview.getSnapshot().phase).toBe("applied");
  await vi.advanceTimersByTimeAsync(DELAY * 3);
  expect(classifies()).toHaveLength(0);
});

test("F36 FR3: closing the editor drops a waiting request and forgets the answer", async () => {
  const { editor, preview, keys, classifies } = setup();
  editor.rename(keys()[0]!, "Films");
  expect(preview.getSnapshot().phase).toBe("waiting");
  editor.requestClose();
  editor.discard();
  await vi.advanceTimersByTimeAsync(DELAY * 3);
  expect(classifies()).toHaveLength(0);
  expect(preview.getSnapshot()).toEqual({
    phase: "applied",
    result: undefined,
    reasonKey: undefined,
  });
});

test("F36 FR3: a failed preview reads as failed with its reason and can be tried again", async () => {
  let fail = true;
  const { editor, preview, keys, classifies } = setup({
    "group.classify": (body: Body) => {
      if (fail) throw new Error("boom");
      return {
        kind: "summary",
        requestSeq: body.requestSeq,
        summary: draftSummary(7),
      };
    },
  });
  editor.rename(keys()[0]!, "Films");
  await vi.advanceTimersByTimeAsync(DELAY);
  expect(preview.getSnapshot().phase).toBe("failed");
  expect(preview.getSnapshot().result).toBeUndefined();
  fail = false;
  preview.retry();
  await vi.advanceTimersByTimeAsync(0);
  expect(classifies()).toHaveLength(2);
  expect(preview.getSnapshot().phase).toBe("ready");
  expect(preview.getSnapshot().result?.summary.groups[0]?.memberCount).toBe(7);
});

test("F36 FR3: a draft that needs a job waits for it and then shows its summary", async () => {
  let done = false;
  const { editor, preview, keys, fake } = setup({
    "group.classify": (body: Body) => ({
      kind: "job",
      requestSeq: body.requestSeq,
      jobRef: "job-p",
      classificationRef: DRAFT,
      resultVersion: RESULT,
    }),
    "job.get": () => ({
      jobRef: "job-p",
      kind: "group",
      state: done ? "done" : "running",
      sequence: done ? 2 : 1,
      resultVersion: null,
      reasonKey: null,
      terminal: done ? { sequence: 2 } : undefined,
    }),
    "group.summary": () => draftSummary(4),
  });
  editor.rename(keys()[0]!, "Films");
  await vi.advanceTimersByTimeAsync(DELAY);
  expect(preview.getSnapshot().phase).toBe("computing");
  done = true;
  fake.emit("job.finished", {
    jobRef: "job-p",
    body: { sequence: 2 },
  });
  await vi.advanceTimersByTimeAsync(0);
  await vi.advanceTimersByTimeAsync(0);
  expect(preview.getSnapshot().phase).toBe("ready");
  expect(preview.getSnapshot().result?.summary.groups[0]?.memberCount).toBe(4);
  preview.dispose();
});
