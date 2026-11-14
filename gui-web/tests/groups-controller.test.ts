import { expect, test } from "vitest";
import { AnalysisError } from "../src/ipc";
import {
  GroupsController,
  showsPreviousClassification,
} from "../src/state/groups-controller";
import {
  CLASSIFICATION,
  NEWER,
  RESULT,
  SCOPE,
  deferred,
  fakeClient,
  flush,
  ruleset,
  standardHandlers,
  summary,
} from "./groups-fixtures";
import type { Body } from "./groups-fixtures";

function setup(
  extra: Parameters<typeof standardHandlers>[0] = {},
  scope = true,
) {
  const fake = fakeClient(standardHandlers(extra));
  const controller = new GroupsController(
    fake.client,
    { resultVersion: RESULT, scope: scope ? SCOPE : undefined },
    "view-1",
  );
  controller.start();
  return { fake, controller };
}

const classifyBodies = (fake: ReturnType<typeof fakeClient>) =>
  fake.calls
    .filter((call) => call.name === "group.classify")
    .map((call) => call.body);

test("F36 FR2: classifying sends the saved rules on one view channel with rising sequences", async () => {
  const { fake, controller } = setup();
  await flush();
  expect(controller.getSnapshot().phase).toBe("idle");
  expect(controller.getSnapshot().ruleset?.rulesetVersion).toBe(3);
  await controller.classify();
  expect(controller.getSnapshot().phase).toBe("showing");
  expect(controller.getSnapshot().summary?.classificationRef).toBe(
    CLASSIFICATION,
  );
  await controller.classify();
  expect(classifyBodies(fake)).toEqual([
    {
      viewRef: "view-1",
      requestSeq: 1,
      resultVersion: RESULT,
      scope: SCOPE,
      rules: { kind: "saved", rulesetVersion: 3 },
    },
    {
      viewRef: "view-1",
      requestSeq: 2,
      resultVersion: RESULT,
      scope: SCOPE,
      rules: { kind: "saved", rulesetVersion: 3 },
    },
  ]);
});

test("F36 FR2: nothing is classified until the user asks", async () => {
  const { fake } = setup();
  await flush();
  expect(fake.count("group.classify")).toBe(0);
});

test("F36 FR2: a job moves Working to Showing only after it finished as done and its summary was read", async () => {
  let state = "running";
  const { fake, controller } = setup({
    "group.classify": (body: Body) => ({
      kind: "job",
      requestSeq: body.requestSeq,
      jobRef: "job-1",
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
    }),
    "job.get": () => ({
      jobRef: "job-1",
      kind: "group.classify",
      state,
      sequence: state === "done" ? 2 : 1,
      resultVersion: null,
      reasonKey: null,
    }),
  });
  await controller.classify();
  expect(controller.getSnapshot().phase).toBe("working");
  expect(controller.getSnapshot().jobRef).toBe("job-1");
  await flush();
  expect(fake.count("group.summary")).toBe(0);
  state = "done";
  fake.emit("job.finished", { jobRef: "job-1", body: { sequence: 2 } });
  await flush();
  await flush();
  expect(fake.count("group.summary")).toBe(1);
  expect(controller.getSnapshot().phase).toBe("showing");
  expect(controller.getSnapshot().summary?.classificationRef).toBe(
    CLASSIFICATION,
  );
});

test.each([
  ["cancelled", { kind: "cancelled" }],
  ["failed", { kind: "failed", reasonKey: "db.busy", retryable: true }],
  ["partial", { kind: "failed", reasonKey: "db.busy", retryable: true }],
] as const)(
  "F36 FR2: a %s job keeps the previous result and shows how it ended",
  async (state, outcome) => {
    let jobState = "running";
    let first = true;
    const { fake, controller } = setup({
      "group.classify": (body: Body) => {
        if (first) {
          first = false;
          return {
            kind: "summary",
            requestSeq: body.requestSeq,
            summary: summary(),
          };
        }
        return {
          kind: "job",
          requestSeq: body.requestSeq,
          jobRef: "job-2",
          classificationRef: "other-classification",
          resultVersion: RESULT,
        };
      },
      "job.get": () => ({
        jobRef: "job-2",
        kind: "group.classify",
        state: jobState,
        sequence: 3,
        resultVersion: null,
        reasonKey: jobState === "running" ? null : "db.busy",
      }),
    });
    await controller.classify();
    const shown = controller.getSnapshot().summary;
    await controller.classify();
    expect(controller.getSnapshot().phase).toBe("working");
    expect(showsPreviousClassification(controller.getSnapshot())).toBe(true);
    jobState = state;
    fake.emit("job.finished", { jobRef: "job-2", body: { sequence: 3 } });
    await flush();
    await flush();
    const snapshot = controller.getSnapshot();
    expect(snapshot.phase).toBe("showing");
    expect(snapshot.summary).toBe(shown);
    expect(snapshot.outcome).toMatchObject(outcome);
    expect(showsPreviousClassification(snapshot)).toBe(true);
  },
);

test("F36 FR2: a cancelled job without an earlier result still ends in Showing with its end state", async () => {
  const { fake, controller } = setup({
    "group.classify": (body: Body) => ({
      kind: "job",
      requestSeq: body.requestSeq,
      jobRef: "job-3",
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
    }),
    "job.get": () => ({
      jobRef: "job-3",
      kind: "group.classify",
      state: "cancelled",
      sequence: 2,
      resultVersion: null,
      reasonKey: null,
    }),
  });
  await controller.classify();
  fake.emit("job.finished", { jobRef: "job-3", body: { sequence: 2 } });
  await flush();
  await flush();
  expect(controller.getSnapshot().phase).toBe("showing");
  expect(controller.getSnapshot().summary).toBeUndefined();
  expect(controller.getSnapshot().outcome).toEqual({ kind: "cancelled" });
});

test("F36 FR2: cancelling asks the host to cancel the job, not the view", async () => {
  const { fake, controller } = setup({
    "group.classify": (body: Body) => ({
      kind: "job",
      requestSeq: body.requestSeq,
      jobRef: "job-4",
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
    }),
    "job.get": () => ({
      jobRef: "job-4",
      kind: "x",
      state: "running",
      sequence: 1,
      resultVersion: null,
      reasonKey: null,
    }),
  });
  await controller.classify();
  await controller.cancel();
  expect(fake.calls.find((call) => call.name === "group.cancel")?.body).toEqual(
    {
      kind: "job",
      jobRef: "job-4",
    },
  );
  expect(controller.getSnapshot().phase).toBe("working");
});

test("F36 FR2: results.version_changed offers an update and never classifies by itself", async () => {
  const { fake, controller } = setup({
    "group.classify": (body: Body) => ({
      kind: "summary",
      requestSeq: body.requestSeq,
      summary: summary({ resultVersion: body.resultVersion ?? NEWER }),
    }),
  });
  await controller.classify();
  expect(controller.getSnapshot().phase).toBe("showing");
  // The event for the version that is already shown changes nothing.
  fake.emit("results.version_changed", { resultVersion: RESULT });
  expect(controller.getSnapshot().phase).toBe("showing");
  fake.emit("results.version_changed", { resultVersion: NEWER });
  await flush();
  expect(controller.getSnapshot().phase).toBe("updatable");
  expect(controller.getSnapshot().newerResult).toBe(NEWER);
  expect(fake.count("group.classify")).toBe(1);
  expect(controller.getSnapshot().summary?.resultVersion).toBe(RESULT);
  // The user's update classifies the whole newer result.
  await controller.classify();
  expect(classifyBodies(fake)[1]).toEqual({
    viewRef: "view-1",
    requestSeq: 2,
    resultVersion: NEWER,
    rules: { kind: "saved", rulesetVersion: 3 },
  });
  expect(controller.getSnapshot().phase).toBe("showing");
  expect(controller.getSnapshot().newerResult).toBeUndefined();
});

test("F36 FR2: a version event that names no version still only offers an update", async () => {
  const { fake, controller } = setup();
  await controller.classify();
  fake.emit("results.version_changed", {
    body: { lineageId: "x", sequence: 4 },
  });
  expect(controller.getSnapshot().phase).toBe("updatable");
  expect(controller.getSnapshot().newerResult).toBe("unknown");
  expect(fake.count("group.classify")).toBe(1);
});

test.each([
  ["result.version_stale", "STALE_VERSION", "version"],
  ["result.requery_required", "STALE_VERSION", "version"],
  ["group.result_expired", "NOT_FOUND", "expired"],
  ["group.rules_modified", "STALE_VERSION", "rules"],
] as const)(
  "F36 FR2: %s sends the view to RequeryNeeded",
  async (reason, code, staleReason) => {
    let fail = false;
    const { controller } = setup({
      "group.classify": (body: Body) => {
        if (fail) throw new AnalysisError(code, reason);
        return {
          kind: "summary",
          requestSeq: body.requestSeq,
          summary: summary(),
        };
      },
    });
    await controller.classify();
    fail = true;
    await controller.classify();
    const snapshot = controller.getSnapshot();
    expect(snapshot.phase).toBe("requery_needed");
    expect(snapshot.staleReason).toBe(staleReason);
    expect(snapshot.summary?.classificationRef).toBe(CLASSIFICATION);
    expect(showsPreviousClassification(snapshot)).toBe(true);
  },
);

test("F36 FR2: a read that finds the shown classification unreadable asks for a new request", async () => {
  const { controller } = setup();
  await controller.classify();
  expect(
    controller.reportUnreadable(
      new AnalysisError("NOT_FOUND", "group.result_expired"),
    ),
  ).toBe(true);
  expect(controller.getSnapshot().phase).toBe("requery_needed");
  expect(
    controller.reportUnreadable(new AnalysisError("BUSY", "db.busy", true)),
  ).toBe(false);
});

test("F36 FR2: a version outside a stale answer re-requests the latest result without the old folder", async () => {
  let fail = true;
  const { fake, controller } = setup({
    "group.classify": (body: Body) => {
      if (fail)
        throw new AnalysisError("STALE_VERSION", "result.version_stale");
      return {
        kind: "summary",
        requestSeq: body.requestSeq,
        summary: summary({ resultVersion: NEWER }),
      };
    },
  });
  await controller.classify();
  expect(controller.getSnapshot().phase).toBe("requery_needed");
  fail = false;
  await controller.classify();
  const second = classifyBodies(fake)[1];
  expect(second).not.toHaveProperty("resultVersion");
  expect(second).not.toHaveProperty("scope");
  expect(controller.getSnapshot().summary?.resultVersion).toBe(NEWER);
});

test("F36 FR2: an older answer that arrives after a newer request is dropped", async () => {
  const first = deferred<unknown>();
  const answers: unknown[] = [];
  const { controller } = setup({
    "group.classify": (body: Body) => {
      if (body.requestSeq === 1) return first.promise;
      return {
        kind: "summary",
        requestSeq: 2,
        summary: summary({ classificationRef: "second" }),
      };
    },
  });
  const pending = controller.classify();
  await flush();
  expect(controller.getSnapshot().phase).toBe("requesting");
  // Something else shows the shown classification is unreadable; the user asks again.
  controller.reportUnreadable(
    new AnalysisError("NOT_FOUND", "group.result_expired"),
  );
  expect(controller.getSnapshot().phase).toBe("requery_needed");
  await controller.classify();
  expect(controller.getSnapshot().summary?.classificationRef).toBe("second");
  first.resolve({
    kind: "summary",
    requestSeq: 1,
    summary: summary({ classificationRef: "first" }),
  });
  await pending;
  answers.push(controller.getSnapshot().summary?.classificationRef);
  expect(answers).toEqual(["second"]);
  expect(controller.getSnapshot().phase).toBe("showing");
});

test("F36 FR2: an answer for the latest sequence but another result version is dropped", async () => {
  let other = false;
  const { controller } = setup({
    "group.classify": (body: Body) => ({
      kind: "summary",
      requestSeq: body.requestSeq,
      summary: summary({
        resultVersion: other ? NEWER : RESULT,
        classificationRef: other ? "wrong" : CLASSIFICATION,
      }),
    }),
  });
  await controller.classify();
  other = true;
  await controller.classify();
  const snapshot = controller.getSnapshot();
  expect(snapshot.summary?.classificationRef).toBe(CLASSIFICATION);
  expect(snapshot.phase).toBe("requery_needed");
  expect(snapshot.staleReason).toBe("version");
});

test("F36 FR2: an answer made with other rules is dropped, whether it names them or the rules changed meanwhile", async () => {
  const wrongRules = setup({
    "group.classify": (body: Body) => ({
      kind: "summary",
      requestSeq: body.requestSeq,
      summary: summary({
        rulesetVersion: 4,
        rulesetFingerprint: "bb",
        classificationRef: "wrong",
      }),
    }),
  });
  await wrongRules.controller.classify();
  expect(wrongRules.controller.getSnapshot().summary).toBeUndefined();
  expect(wrongRules.controller.getSnapshot().phase).toBe("requery_needed");
  expect(wrongRules.controller.getSnapshot().staleReason).toBe("rules");

  const gate = deferred<unknown>();
  const meanwhile = setup({ "group.classify": () => gate.promise });
  const pending = meanwhile.controller.classify();
  await flush();
  meanwhile.controller.applySavedRuleset(
    ruleset({ rulesetVersion: 4, revision: 8, fingerprint: "bb" }),
  );
  gate.resolve({ kind: "summary", requestSeq: 1, summary: summary() });
  await pending;
  expect(meanwhile.controller.getSnapshot().summary).toBeUndefined();
  expect(meanwhile.controller.getSnapshot().phase).toBe("requery_needed");
});

test("F36 FR3: saved rules that differ from the shown classification ask for a new classification", async () => {
  const { fake, controller } = setup();
  await controller.classify();
  controller.applySavedRuleset(
    ruleset({ rulesetVersion: 4, revision: 8, fingerprint: "bb" }),
  );
  const snapshot = controller.getSnapshot();
  expect(snapshot.phase).toBe("requery_needed");
  expect(snapshot.rulesChanged).toBe(true);
  expect(snapshot.summary?.rulesetVersion).toBe(3);
  expect(fake.count("group.classify")).toBe(1);
});

test("F36 FR2: an inapplicable source ends in NotApplicable with its reason and can be asked again", async () => {
  let applicable = false;
  const { controller } = setup({
    "group.classify": (body: Body) =>
      applicable
        ? { kind: "summary", requestSeq: body.requestSeq, summary: summary() }
        : {
            kind: "not_applicable",
            requestSeq: body.requestSeq,
            reasonKey: "group.source_unsupported",
          },
  });
  await controller.classify();
  expect(controller.getSnapshot().phase).toBe("not_applicable");
  expect(controller.getSnapshot().reasonKey).toBe("group.source_unsupported");
  applicable = true;
  await controller.classify();
  expect(controller.getSnapshot().phase).toBe("showing");
});

test("F36 FR2: a failed request keeps what was shown and offers a retry", async () => {
  let fail = false;
  const { controller } = setup({
    "group.classify": (body: Body) => {
      if (fail) throw new AnalysisError("BUSY", "db.busy", true);
      return {
        kind: "summary",
        requestSeq: body.requestSeq,
        summary: summary(),
      };
    },
  });
  await controller.classify();
  fail = true;
  await controller.classify();
  const snapshot = controller.getSnapshot();
  expect(snapshot.phase).toBe("showing");
  expect(snapshot.outcome).toEqual({
    kind: "failed",
    reasonKey: "db.busy",
    retryable: true,
  });
  expect(snapshot.summary?.classificationRef).toBe(CLASSIFICATION);
  fail = false;
  await controller.classify();
  expect(controller.getSnapshot().outcome).toBeUndefined();
});

test("F36 FR2: closing the screen cancels a running classification and stops listening", async () => {
  const { fake, controller } = setup({
    "group.classify": (body: Body) => ({
      kind: "job",
      requestSeq: body.requestSeq,
      jobRef: "job-5",
      classificationRef: CLASSIFICATION,
      resultVersion: RESULT,
    }),
    "job.get": () => ({
      jobRef: "job-5",
      kind: "x",
      state: "running",
      sequence: 1,
      resultVersion: null,
      reasonKey: null,
    }),
  });
  await controller.classify();
  expect(fake.listenerCount()).toBeGreaterThan(0);
  controller.dispose();
  await flush();
  expect(fake.calls.find((call) => call.name === "group.cancel")?.body).toEqual(
    {
      kind: "view",
      viewRef: "view-1",
      requestSeq: 1,
    },
  );
  expect(fake.listenerCount()).toBe(0);
});
