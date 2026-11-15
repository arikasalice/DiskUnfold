import { cleanup, fireEvent, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import {
  VOLUME_COMPARE_REREAD_MS,
  VOLUME_READ_FAILURE_LIMIT,
} from "../src/features/analysis/summary-batch";
import { AnalysisError } from "../src/ipc";
import type { AnalysisClient } from "../src/ipc";
import { RESULT_REFLECT_INTERVAL_MS } from "../src/state/result-cadence";
import {
  logical,
  passReflectInterval,
  resetReflectOffset,
  startPanel,
} from "./summary-fixture";
import { shownPart, summaryRun } from "./summary-harness";

/*
 * F02 FR3.HD1.gui-web.DD1 (design review of PR #69, third check): the volume
 * part says "Updating…" only while a read is in flight or due. A paused scan
 * publishes nothing further, so a failed read shows the problem with "Retry"
 * at once; while the scan runs, the third failure in a row does. A refusal
 * because the result was replaced is not such a failure.
 */

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  resetReflectOffset();
});

const error = () => new AnalysisError("INTERNAL", "internal.unexpected");
const replaced = () =>
  new AnalysisError("STALE_VERSION", "result.requery_required");
const problem = () => screen.queryByRole("alert");
const retry = () => screen.queryByRole("button", { name: "Retry" });
const updating = () => screen.queryByText("Updating…");

type Run = Awaited<ReturnType<typeof summaryRun>>;

/** Offers the totals of `version` and refuses the read the part starts for it. */
async function fail(run: Run, version: string, failure: unknown, live = true) {
  run.show(version, live);
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  expect(run.reads().at(-1)).toBe(version);
  await run.refuse(failure);
}

test("F02 FR3.HD1.gui-web.DD1: a failed read while the scan is paused shows the problem and Retry at once, and Retry reads again", async () => {
  const run = await summaryRun();
  const details = screen.getByRole("button", {
    name: "Observation and difference details",
  });
  fireEvent.click(details);
  await fail(run, "v2", error(), false);
  expect(run.total()).toBe("200 bytes");
  expect(problem()!.textContent).toBe("The volume explanation is unavailable.");
  expect(retry()).not.toBeNull();
  expect(updating()).toBeNull();
  expect(screen.queryByText(/^100 bytes/)).toBeNull();
  expect(run.pending).toHaveLength(0);
  // The part stays open around the problem.
  expect(details.getAttribute("aria-expanded")).toBe("true");
  const hide = screen.getByRole("button", { name: "Hide volume explanation" });

  fireEvent.click(retry()!);
  expect(run.reads()).toEqual(["v1", "v2", "v2"]);
  expect(updating()).not.toBeNull();
  expect(problem()).toBeNull();
  expect(document.activeElement).toBe(hide);
  await run.answer();
  expect(shownPart().getByText(/^200 bytes/)).toBeDefined();
  expect(retry()).toBeNull();
  expect(run.watch.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: a part opened on a paused scan shows a failed read with Retry", async () => {
  const run = await summaryRun();
  fireEvent.click(
    screen.getByRole("button", { name: "Hide volume explanation" }),
  );
  run.show("v1", false);
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  await run.refuse(error());
  expect(problem()).not.toBeNull();
  expect(updating()).toBeNull();

  fireEvent.click(retry()!);
  expect(run.reads()).toEqual(["v1", "v1", "v1"]);
  expect(
    screen.getByRole("progressbar", { name: "Reading volume observations" }),
  ).toBeDefined();
  await run.answer();
  expect(shownPart().getByText(/^100 bytes/)).toBeDefined();
  expect(problem()).toBeNull();
});

test.each([
  ["an error", error],
  ["a replaced result", replaced],
])(
  "F02 FR3.HD1.gui-web.DD1: a part updating after %s shows the problem and Retry when the scan pauses",
  async (_, failure) => {
    const run = await summaryRun();
    await fail(run, "v2", failure());
    expect(updating()).not.toBeNull();
    expect(problem()).toBeNull();

    // The scan pauses on the result the summary already shows.
    run.show("v2", false);
    expect(problem()).not.toBeNull();
    expect(retry()).not.toBeNull();
    expect(updating()).toBeNull();
    expect(run.total()).toBe("200 bytes");
    expect(run.reads()).toEqual(["v1", "v2"]);
    expect(run.pending).toHaveLength(0);
  },
);

test("F02 FR3.HD1.gui-web.DD1: the third failed read in a row of a running scan shows the problem and Retry, and the next result reads again", async () => {
  expect(VOLUME_READ_FAILURE_LIMIT).toBe(3);
  const run = await summaryRun();
  await fail(run, "v2", error());
  // A refusal because the result was replaced is not counted.
  await fail(run, "v3", replaced());
  await fail(run, "v4", error());
  expect(updating()).not.toBeNull();
  expect(problem()).toBeNull();
  expect(retry()).toBeNull();

  await fail(run, "v5", error());
  expect(run.total()).toBe("500 bytes");
  expect(problem()).not.toBeNull();
  expect(retry()).not.toBeNull();
  expect(updating()).toBeNull();
  expect(run.pending).toHaveLength(0);

  // The next result is read without Retry; the problem stays meanwhile.
  run.show("v6");
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  expect(run.reads().at(-1)).toBe("v6");
  expect(run.total()).toBe("500 bytes");
  expect(problem()).not.toBeNull();
  await run.answer();
  expect(run.total()).toBe("600 bytes");
  expect(shownPart().getByText(/^600 bytes/)).toBeDefined();
  expect(problem()).toBeNull();
  expect(retry()).toBeNull();

  // The successful read started the count again.
  await fail(run, "v7", error());
  await fail(run, "v8", error());
  expect(updating()).not.toBeNull();
  expect(problem()).toBeNull();
  expect(run.watch.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: Retry reads the newest result and starts the failure count again", async () => {
  const run = await summaryRun();
  for (const version of ["v2", "v3", "v4"]) await fail(run, version, error());
  expect(problem()).not.toBeNull();
  // A newer result arrives before the user retries.
  run.show("v5");

  fireEvent.click(retry()!);
  expect(updating()).not.toBeNull();
  expect(problem()).toBeNull();
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  expect(run.reads()).toEqual(["v1", "v2", "v3", "v4", "v5"]);
  // One more failure is the first of a new count, so the part keeps waiting.
  await run.refuse(error());
  expect(run.total()).toBe("500 bytes");
  expect(updating()).not.toBeNull();
  expect(problem()).toBeNull();

  run.show("v6");
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  await run.answer();
  expect(run.total()).toBe("600 bytes");
  expect(shownPart().getByText(/^600 bytes/)).toBeDefined();
  expect(run.watch.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: closing and opening the part starts the failure count again", async () => {
  const run = await summaryRun();
  await fail(run, "v2", error());
  await fail(run, "v3", error());
  fireEvent.click(
    screen.getByRole("button", { name: "Hide volume explanation" }),
  );
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  await run.answer();
  expect(shownPart().getByText(/^300 bytes/)).toBeDefined();

  await fail(run, "v4", error());
  await fail(run, "v5", error());
  expect(updating()).not.toBeNull();
  expect(problem()).toBeNull();
});

test("F02 FR3.HD1.gui-web.DD1: comparing all scanned folders while the part is updating reads the newest result", async () => {
  const run = await summaryRun();
  run.show("v2");
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  expect(run.total()).toBe("200 bytes");
  expect(updating()).not.toBeNull();
  // A newer result arrives while the explanation of v2 is still read.
  run.show("v3");

  fireEvent.click(
    screen.getByRole("button", {
      name: "Compare all scanned folders on this volume",
    }),
  );
  expect(updating()).toBeNull();
  // The late explanation belongs to the part that was replaced.
  await run.answer();
  expect(run.total()).toBe("200 bytes");
  expect(run.reads(true)).toEqual([]);

  await run.settle(VOLUME_COMPARE_REREAD_MS);
  expect(run.reads(true)).toEqual(["v3"]);
  await run.answer();
  expect(run.total()).toBe("300 bytes");
  expect(shownPart().getByText(/^300 bytes/)).toBeDefined();
  expect(shownPart().getByText(/All scanned folders/)).toBeDefined();
  expect(run.watch.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: the analysis panel tells the summary that a paused scan publishes nothing further", async () => {
  const scan = await startPanel();
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  const summary = () => screen.findByLabelText("Volume usage summary");
  expect(within(await summary()).getByText(/^100 bytes/)).toBeDefined();
  scan.changeState("paused");
  await screen.findByText("Result so far");

  const call = scan.client.call;
  let failing = true;
  const refused: (string | undefined)[] = [];
  scan.client.call = ((name: string, body: { resultVersion?: string }) => {
    if (name !== "space.volume_gap" || !failing)
      return call(name as never, body as never);
    refused.push(body.resultVersion);
    return Promise.reject(error());
  }) as AnalysisClient["call"];
  // The result published just before the pause still reaches the summary.
  passReflectInterval();
  scan.publish("v2");
  await screen.findByText("The volume explanation is unavailable.");
  expect(logical().getByText("200 bytes")).toBeDefined();
  expect(retry()).not.toBeNull();
  expect(updating()).toBeNull();

  failing = false;
  fireEvent.click(retry()!);
  expect(within(await summary()).getByText(/^200 bytes/)).toBeDefined();
  expect(problem()).toBeNull();
  // One refused read of the new result, then the retried one.
  expect(refused).toEqual(["v2"]);
  expect(scan.reads("space.volume_gap")).toEqual(["v1", "v2"]);
});
