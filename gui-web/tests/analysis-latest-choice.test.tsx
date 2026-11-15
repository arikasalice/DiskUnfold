import {
  act,
  cleanup,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AnalysisError } from "../src/ipc";
import type { AnalysisClient, JobState, SpaceTotal } from "../src/ipc";
import {
  logical,
  passReflectInterval,
  resetReflectOffset,
  startPanel,
} from "./summary-fixture";

/*
 * F02 FR3.HD1.gui-web.DD1: the summary follows its scan by itself, so "Show
 * latest result" is offered only once the scan has stopped and its newest
 * result could not be read automatically. These tests keep, on that path,
 * what analysis.test.tsx asserted about the explicit choice before the
 * summary became automatic (review of PR #69).
 */

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  resetReflectOffset();
});

/** Only the reads under test reach the port: no 500 ms status poll. */
function withoutStatusPoll() {
  const schedule = window.setInterval.bind(window);
  vi.spyOn(window, "setInterval").mockImplementation(((
    handler: TimerHandler,
    timeout?: number,
    ...args: unknown[]
  ) =>
    timeout === 500
      ? 0
      : schedule(handler, timeout, ...args)) as typeof window.setInterval);
}

const stale = () =>
  new AnalysisError("STALE_VERSION", "result.requery_required");

/**
 * The panel on a running scan, with the engine's answers under the test's
 * control: a read may be held back or refused by returning a gate; a read
 * without one, or whose gate opens, is answered by the scan. Calls made
 * before this, the first totals among them, are not recorded.
 */
async function panel(onBrowse?: (version: string) => void) {
  const scan = await startPanel(undefined, onBrowse);
  const answer = scan.client.call;
  const control = {
    /** Holds back or refuses a read of the totals of a version. */
    totals: undefined as
      ((version: string) => Promise<void> | undefined) | undefined,
    /** Holds back a status read, which then answers for another job. */
    strayStatus: undefined as (() => Promise<void> | undefined) | undefined,
    /** Refuses every status read. */
    failStatus: false,
    /** Holds back a read of the volume explanation. */
    volume: undefined as (() => Promise<void> | undefined) | undefined,
    /** Marks the totals of the stopped scan's last result. */
    completeness: undefined as SpaceTotal["completeness"] | undefined,
    /** Every call in order, with the result version it named. */
    asked: [] as { name: string; version: string | undefined }[],
  };
  scan.client.call = ((name: string, body: { resultVersion?: string }) => {
    control.asked.push({ name, version: body.resultVersion });
    const pass = () => answer(name as never, body as never);
    if (name === "scan.failures")
      return Promise.resolve({
        resultVersion: body.resultVersion,
        totalCount: 1,
        rows: [
          {
            kind: "unread_range",
            stage: "enumerate",
            pathRef: { rootIndex: 0, relative: [] },
            reasonKey: "scan.path_denied",
            displayPath: "C:\\" + body.resultVersion,
          },
        ],
      });
    if (name === "job.get") {
      if (control.failStatus) return Promise.reject(new Error("No status."));
      const stray = control.strayStatus?.();
      if (!stray) return pass();
      return stray
        .then(pass)
        .then((status) => ({ ...(status as object), jobRef: "unrelated" }));
    }
    if (name === "space.volume_gap")
      return control.volume?.()?.then(pass) ?? pass();
    if (name !== "space.total") return pass();
    const gate = control.totals?.(body.resultVersion!);
    const read = gate ? gate.then(pass) : pass();
    return (read as Promise<SpaceTotal>).then((total) =>
      control.completeness && total.resultVersion !== "v1"
        ? { ...total, completeness: control.completeness }
        : total,
    );
  }) as AnalysisClient["call"];
  const versions = (name: string) =>
    control.asked
      .filter((call) => call.name === name)
      .map((call) => call.version);
  return { scan, control, versions };
}

type Run = Awaited<ReturnType<typeof panel>>;

/**
 * Stops the scan on "v2", whose automatic read fails, and returns the
 * "Show latest result" action that this leaves.
 */
async function stopUnread(run: Run, state: JobState = "done") {
  run.control.totals = (version) =>
    version === "v2"
      ? Promise.reject(new Error("Could not read newer totals."))
      : undefined;
  passReflectInterval();
  run.scan.publish("v2", state);
  await within(await screen.findByRole("alert")).findByText(
    "Analysis could not be read.",
  );
  run.control.totals = undefined;
  return screen.findByRole<HTMLButtonElement>("button", {
    name: "Show latest result",
  });
}

const reading = () =>
  screen.queryByRole("button", { name: "Reading latest result…" });
const statusOf = (label: string) =>
  screen.getByText(label).closest<HTMLElement>('[role="status"]')!;

function pressEnter(button: HTMLElement, repeat = false) {
  fireEvent.keyDown(button, { key: "Enter", code: "Enter", repeat });
  fireEvent.keyUp(button, { key: "Enter", code: "Enter" });
}

test("F02 FR3.HD1.gui-web.DD1: a final result that could not be read keeps the interim result and offers the choice beside the summary", async () => {
  const browsed = vi.fn();
  const run = await panel(browsed);
  const latest = await stopUnread(run);
  const summary = statusOf("Result version: Interim");
  expect(screen.getByText("Scan finished")).toBeDefined();
  expect(summary.textContent).toContain(
    "You’re still viewing an interim result.",
  );
  // The action is not part of the announced summary and comes before the
  // result actions.
  expect(summary.contains(latest)).toBe(false);
  expect(
    latest.compareDocumentPosition(
      screen.getByRole("button", { name: "Browse folders" }),
    ),
  ).toBe(Node.DOCUMENT_POSITION_FOLLOWING);
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(screen.queryByText("200 bytes")).toBeNull();
  // The unread result is not read again without the user's choice.
  await act(async () => {});
  expect(run.versions("space.total")).toEqual(["v2"]);
  // Browsing opens the result that is shown.
  fireEvent.click(screen.getByRole("button", { name: "Browse folders" }));
  expect(browsed).toHaveBeenCalledTimes(1);
  expect(browsed.mock.calls[0]![0]).toBe("v1");
});

test("F02 FR3.HD1.gui-web.DD1: a choice made during a status read is kept, cannot be repeated while it reads, and moves focus to the summary", async () => {
  withoutStatusPoll();
  const browsed = vi.fn();
  const run = await panel(browsed);
  const latest = await stopUnread(run);
  const summary = statusOf("Result version: Interim");
  // Queue the choice behind a status read that cannot consume it.
  let releaseStatus = () => {};
  let statusHeld = false;
  run.control.strayStatus = () => {
    if (statusHeld) return undefined;
    statusHeld = true;
    return new Promise<void>((resolve) => {
      releaseStatus = resolve;
    });
  };
  let finish = () => {};
  run.control.totals = () =>
    new Promise<void>((resolve) => {
      finish = resolve;
    });
  run.scan.changeState("done");
  await waitFor(() => expect(statusHeld).toBe(true));

  latest.focus();
  pressEnter(latest);
  expect(reading()).toBe(latest);
  expect(latest.getAttribute("aria-disabled")).toBe("true");
  expect(document.activeElement).toBe(latest);
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(
    logical().getByText("Partial measurement · Reported by the file system"),
  ).toBeDefined();
  expect(run.versions("space.total")).toEqual(["v2"]);

  // The status read ends; the choice is still served by the next one.
  await act(async () => releaseStatus());
  await waitFor(() =>
    expect(run.versions("space.total")).toEqual(["v2", "v2"]),
  );
  expect(reading()).toBe(latest);
  const started = run.control.asked.length;
  pressEnter(latest, true);
  fireEvent.click(latest);
  await act(async () => finish());
  await screen.findByText("Result version: Final");
  expect(
    run.control.asked.slice(started).filter(({ name }) => name === "job.get"),
  ).toEqual([]);
  expect(run.versions("space.total")).toEqual(["v2", "v2"]);
  expect(logical().getByText("200 bytes")).toBeDefined();
  expect(screen.queryByText("100 bytes")).toBeNull();
  // The measurement line is the chosen result's, apart from its version.
  expect(
    logical().getByText("Complete measurement · Reported by the file system"),
  ).toBeDefined();
  expect(document.activeElement).toBe(summary);
  expect(summary.textContent).toContain("Now showing the latest result.");
  expect(screen.queryByText("Analysis could not be read.")).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Browse folders" }));
  expect(browsed.mock.calls.at(-1)![0]).toBe("v2");
});

test("F02 FR3.HD1.gui-web.DD1: a chosen read that fails keeps the interim totals, the action and its focus", async () => {
  const run = await panel();
  const latest = await stopUnread(run);
  let fail = () => {};
  run.control.totals = () =>
    new Promise<void>((_, reject) => {
      fail = () => reject(new Error("Could not read newer totals."));
    });
  latest.focus();
  fireEvent.click(latest);
  await waitFor(() =>
    expect(run.versions("space.total")).toEqual(["v2", "v2"]),
  );
  expect(reading()).toBe(latest);
  await act(async () => fail());
  expect(
    await screen.findByRole("button", { name: "Show latest result" }),
  ).toBe(latest);
  expect(document.activeElement).toBe(latest);
  expect(screen.getByText("Analysis could not be read.")).toBeDefined();
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  expect(logical().getByText("100 bytes")).toBeDefined();

  run.control.totals = undefined;
  fireEvent.click(latest);
  await screen.findByText("Result version: Final");
  expect(logical().getByText("200 bytes")).toBeDefined();
});

test.each(["STALE_VERSION", "NOT_FOUND"] as const)(
  "F02 FR3.HD1.gui-web.DD1: a chosen result refused as %s stops showing progress after resynchronizing and can be chosen again",
  async (code) => {
    withoutStatusPoll();
    const run = await panel();
    const latest = await stopUnread(run);
    const summary = statusOf("Result version: Interim");
    run.control.totals = () =>
      Promise.reject(new AnalysisError(code, "result.requery_required"));
    const before = run.control.asked.length;
    fireEvent.click(latest);
    expect(reading()).toBe(latest);
    expect(
      await screen.findByRole("button", { name: "Show latest result" }),
    ).toBe(latest);
    expect(
      run.control.asked.slice(before, before + 3).map(({ name }) => name),
    ).toEqual(["job.get", "space.total", "job.get"]);
    expect(logical().getByText("100 bytes")).toBeDefined();
    expect(screen.getByText("Result version: Interim")).toBeDefined();

    run.control.totals = undefined;
    latest.focus();
    pressEnter(latest);
    await screen.findByText("Now showing the latest result.");
    expect(screen.getByText("Result version: Final")).toBeDefined();
    expect(document.activeElement).toBe(summary);
  },
);

test("F02 FR3.HD1.gui-web.DD1: a choice whose status read fails reads no totals and can be made again", async () => {
  withoutStatusPoll();
  const run = await panel();
  const latest = await stopUnread(run);
  const summary = statusOf("Result version: Interim");
  run.control.failStatus = true;
  latest.focus();
  pressEnter(latest);
  await waitFor(() =>
    expect(run.versions("job.get").length).toBeGreaterThan(0),
  );
  expect(
    await screen.findByRole("button", { name: "Show latest result" }),
  ).toBe(latest);
  await act(async () => {});
  expect(run.versions("space.total")).toEqual(["v2"]);
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(document.activeElement).toBe(latest);

  run.control.failStatus = false;
  pressEnter(latest);
  await screen.findByText("Result version: Final");
  expect(run.versions("space.total")).toEqual(["v2", "v2"]);
  expect(document.activeElement).toBe(summary);
  expect(summary.textContent).toContain("Now showing the latest result.");
});

test("F02 FR3.HD1.gui-web.DD1: a chosen read follows a replacement found while resynchronizing", async () => {
  const run = await panel();
  const latest = await stopUnread(run);
  run.control.totals = (version) => {
    if (version !== "v2") return undefined;
    // The result is replaced while the chosen read is refused.
    run.scan.job.version = "v3";
    run.scan.job.sequence += 1;
    return Promise.reject(stale());
  };
  fireEvent.click(latest);
  await screen.findByText("Now showing the latest result.");
  expect(run.versions("space.total")).toEqual(["v2", "v2", "v3"]);
  expect(logical().getByText("300 bytes")).toBeDefined();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
});

test.each([
  ["partial", "Scan ended with gaps", "missing", "Final with scan gaps"],
  ["cancelled", "Scan cancelled", "cancelled", "Cancelled"],
] as const)(
  "F02 FR3.HD1.gui-web.DD1: a %s scan whose last result could not be read keeps the interim result apart from the job outcome",
  async (outcome, badge, completeness, label) => {
    const run = await panel();
    run.control.completeness = completeness;
    const latest = await stopUnread(run, outcome);
    expect(screen.getByText(badge)).toBeDefined();
    expect(screen.queryByText(/^Scan finished/)).toBeNull();
    expect(statusOf("Result version: Interim").textContent).toContain(
      "Newer result available. Showing the current result until you switch.",
    );
    fireEvent.click(latest);
    await screen.findByText("Result version: " + label);
    expect(screen.getByText(badge)).toBeDefined();
    expect(
      screen.queryByRole("button", { name: "Show latest result" }),
    ).toBeNull();
  },
);

test("F02 FR3.HD1.gui-web.DD1: the issue list stays on the shown result until the unread one is chosen", async () => {
  const run = await panel();
  fireEvent.click(screen.getByRole("button", { name: "View scan issues" }));
  await screen.findByText("C:\\v1");
  const latest = await stopUnread(run);
  await act(async () => {});
  expect(screen.getByText("C:\\v1")).toBeDefined();
  expect(run.versions("scan.failures")).toEqual(["v1"]);

  fireEvent.click(latest);
  await screen.findByText("C:\\v2");
  expect(screen.queryByText("C:\\v1")).toBeNull();
  expect(run.versions("scan.failures")).toEqual(["v1", "v2"]);
});

test("F02 FR3.HD1.gui-web.DD1: a chosen result is still being read until an open volume explanation has read it too", async () => {
  const run = await panel();
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  const part = () => within(screen.getByLabelText("Volume usage summary"));
  await screen.findByLabelText("Volume usage summary");
  const latest = await stopUnread(run);
  const summary = statusOf("Result version: Interim");
  let finish = () => {};
  run.control.volume = () =>
    new Promise<void>((resolve) => {
      finish = resolve;
    });

  latest.focus();
  passReflectInterval();
  fireEvent.click(latest);
  await waitFor(() =>
    expect(run.versions("space.volume_gap")).toEqual(["v1", "v2"]),
  );
  // The scan has read the totals; the summary waits to show them with the
  // explanation, and the action still says so.
  expect(run.versions("space.total")).toEqual(["v2", "v2"]);
  expect(reading()).toBe(latest);
  expect(document.activeElement).toBe(latest);
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(part().getByText(/^100 bytes/)).toBeDefined();

  await act(async () => finish());
  await screen.findByText("Result version: Final");
  expect(logical().getByText("200 bytes")).toBeDefined();
  expect(part().getByText(/^200 bytes/)).toBeDefined();
  expect(document.activeElement).toBe(summary);
  expect(summary.textContent).toContain("Now showing the latest result.");
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
});
