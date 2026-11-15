import {
  act,
  cleanup,
  fireEvent,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import {
  VOLUME_COMPARE_REREAD_MS,
  volumePartPace,
} from "../src/features/analysis/summary-batch";
import { AnalysisError } from "../src/ipc";
import { RESULT_REFLECT_INTERVAL_MS } from "../src/state/result-cadence";
import {
  clearSelection,
  logical,
  passReflectInterval,
  resetReflectOffset,
  select,
  startPanel,
  watchSummary,
} from "./summary-fixture";
import { shownPart, summaryRun, volumeSummary } from "./summary-harness";

/*
 * F02 FR3.HD1.gui-web.DD1 (design review of PR #69, decision on the volume
 * explanation): every value in one summary comes from one result. While the
 * volume explanation or the whole-volume comparison is open, the summary
 * changes together with it, at the pace of the slowest open part, and waits
 * for it no longer than that pace; a late or failed part clears its values
 * and says it is updating.
 */

/** How long the summary waits for the explanation of a scanned folder. */
const EXPLANATION_WAIT_MS = volumePartPace({ root: 0 });

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  resetReflectOffset();
  document.getSelection()?.removeAllRanges();
});

test("F02 FR3.HD1.gui-web.DD1: the open volume explanation changes with the summary in the same render and keeps its expanded part and focus", async () => {
  const watch = watchSummary(() =>
    screen.queryByRole("group", { name: "Logical size" }),
  );
  const scan = await startPanel(watch.onRender);
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  const summary = within(await screen.findByLabelText("Volume usage summary"));
  expect(summary.getByText(/^100 bytes/)).toBeDefined();
  expect(screen.queryByText("Result published")).toBeNull();
  const details = screen.getByRole("button", {
    name: "Observation and difference details",
  });
  fireEvent.click(details);
  details.focus();

  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(summary.getByText(/^200 bytes/)).toBeDefined());
  expect(logical().getByText("200 bytes")).toBeDefined();
  // No commit ever drew the new totals beside the older explanation.
  expect(watch.seen).toContainEqual([200, 200]);
  expect(watch.mixed()).toEqual([]);
  expect(
    screen.getByRole("button", { name: "Observation and difference details" }),
  ).toBe(details);
  expect(details.getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement).toBe(details);
  expect(screen.queryByText("Result published")).toBeNull();
  expect(screen.queryByText("Updated to the latest result")).toBeNull();
  expect(scan.reads("space.volume_gap")).toEqual(["v1", "v2"]);
  expect(scan.reads("browse.detail")).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: selecting text holds the volume explanation with the rest of the summary", async () => {
  const scan = await startPanel();
  fireEvent.click(screen.getByRole("button", { name: "Explain volume usage" }));
  const summary = within(await screen.findByLabelText("Volume usage summary"));
  select(summary.getByText(/^100 bytes/));
  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(scan.reads("space.total")).toEqual(["v1", "v2"]));
  await act(async () => {});
  expect(summary.getByText(/^100 bytes/)).toBeDefined();
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(scan.reads("space.volume_gap")).toEqual(["v1"]);

  clearSelection();
  await waitFor(() => expect(summary.getByText(/^200 bytes/)).toBeDefined());
  expect(logical().getByText("200 bytes")).toBeDefined();
});

test("F02 FR3.HD1.gui-web.DD1: with the explanation open a newer total waits for the explanation of the same result", async () => {
  const run = await summaryRun();
  expect(shownPart().getByText(/^100 bytes/)).toBeDefined();
  run.show("v2");
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  expect(run.reads()).toEqual(["v1", "v2"]);
  // While the explanation of v2 is read, the summary still shows v1.
  expect(run.total()).toBe("100 bytes");
  expect(shownPart().getByText(/^100 bytes/)).toBeDefined();

  await run.answer();
  expect(run.total()).toBe("200 bytes");
  expect(shownPart().getByText(/^200 bytes/)).toBeDefined();
  expect(run.watch.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: with the whole-volume comparison open the summary changes at most every two seconds", async () => {
  const run = await summaryRun();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Compare all scanned folders on this volume",
    }),
  );
  await run.answer();
  expect(screen.getByText("All scanned folders on this volume")).toBeDefined();

  for (const version of ["v2", "v3"]) {
    run.show(version);
    await run.settle(RESULT_REFLECT_INTERVAL_MS);
    expect(run.total()).not.toBe(`${Number(version.slice(1)) * 100} bytes`);
    await run.settle(VOLUME_COMPARE_REREAD_MS - RESULT_REFLECT_INTERVAL_MS);
    await run.answer();
    expect(run.total()).toBe(`${Number(version.slice(1)) * 100} bytes`);
    expect(shownPart().getByText(/All scanned folders/)).toBeDefined();
  }
  expect(run.reads(true)).toEqual(["v1", "v2", "v3"]);
  expect(run.watch.mixed()).toEqual([]);
  const changes = run.changes.slice(1);
  expect(changes).toHaveLength(2);
  expect(changes[1]! - changes[0]!).toBeGreaterThanOrEqual(
    VOLUME_COMPARE_REREAD_MS,
  );
});

test("F02 FR3.HD1.gui-web.DD1: an explanation not read within a second leaves the summary to change alone and says it is updating at the same height", async () => {
  const run = await summaryRun();
  vi.spyOn(HTMLElement.prototype, "getBoundingClientRect").mockReturnValue({
    height: 120,
  } as DOMRect);
  const details = screen.getByRole("button", {
    name: "Observation and difference details",
  });
  fireEvent.click(details);
  details.focus();
  // A re-render records the height of the expanded part.
  run.show("v1");
  run.show("v2");
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  await run.settle(EXPLANATION_WAIT_MS - 1);
  expect(run.total()).toBe("100 bytes");

  await run.settle(1);
  expect(run.total()).toBe("200 bytes");
  const updating = screen.getByText("Updating…");
  expect(volumeSummary()).toBeNull();
  expect(screen.queryByText(/^100 bytes/)).toBeNull();
  expect(updating.closest('[role="status"], [aria-live]')).toBeNull();
  expect(updating.parentElement!.style.minHeight).toBe("120px");
  expect(details.getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement).toBe(details);
  expect(
    screen.getByRole("button", { name: "Hide volume explanation" }),
  ).toBeDefined();

  await run.answer();
  expect(shownPart().getByText(/^200 bytes/)).toBeDefined();
  expect(screen.queryByText("Updating…")).toBeNull();
  expect(run.watch.mixed()).toEqual([]);
});

test.each([
  [
    "a replaced result",
    new AnalysisError("STALE_VERSION", "result.requery_required"),
  ],
  ["an error", new AnalysisError("INTERNAL", "internal.unexpected")],
])(
  "F02 FR3.HD1.gui-web.DD1: an explanation refused for %s leaves the summary to change at once and says it is updating",
  async (_, failure) => {
    const run = await summaryRun();
    run.show("v2");
    await run.settle(RESULT_REFLECT_INTERVAL_MS);
    await run.refuse(failure);
    expect(run.total()).toBe("200 bytes");
    expect(screen.getByText("Updating…")).toBeDefined();
    expect(screen.queryByText(/^100 bytes/)).toBeNull();
    expect(screen.queryByRole("alert")).toBeNull();

    run.show("v3");
    await run.settle(RESULT_REFLECT_INTERVAL_MS);
    expect(run.reads()).toEqual(["v1", "v2", "v3"]);
    await run.answer();
    expect(run.total()).toBe("300 bytes");
    expect(shownPart().getByText(/^300 bytes/)).toBeDefined();
    expect(run.watch.mixed()).toEqual([]);
  },
);

test("F02 FR3.HD1.gui-web.DD1: closing the part lets the summary change at once again", async () => {
  const run = await summaryRun();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Compare all scanned folders on this volume",
    }),
  );
  await run.answer();
  run.show("v2");
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  expect(run.total()).toBe("100 bytes");

  fireEvent.click(
    screen.getByRole("button", { name: "Hide volume explanation" }),
  );
  expect(run.total()).toBe("200 bytes");
  run.show("v3");
  expect(run.total()).toBe("300 bytes");
  expect(run.reads(true)).toEqual(["v1"]);
});

test("F02 FR3.HD1.gui-web.DD1: volume reads never overlap and a batch reads only the newest result", async () => {
  const run = await summaryRun();
  run.show("v2");
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  run.show("v3");
  run.show("v4");
  // The batch for v2 runs on; v3 and v4 wait for the next one, unread.
  await run.settle(EXPLANATION_WAIT_MS / 2);
  expect(run.pending).toHaveLength(1);
  await run.answer();
  expect(run.total()).toBe("200 bytes");
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  expect(run.reads()).toEqual(["v1", "v2", "v4"]);
  expect(run.pending).toHaveLength(1);
  await run.answer();
  expect(run.total()).toBe("400 bytes");
  expect(shownPart().getByText(/^400 bytes/)).toBeDefined();
  expect(run.watch.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: the whole-volume comparison is waited for two seconds before the summary changes alone", async () => {
  const run = await summaryRun();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Compare all scanned folders on this volume",
    }),
  );
  await run.answer();
  run.show("v2");
  await run.settle(VOLUME_COMPARE_REREAD_MS);
  expect(run.reads(true)).toEqual(["v1", "v2"]);

  // The explanation's one second is not the comparison's limit.
  await run.settle(EXPLANATION_WAIT_MS);
  expect(run.total()).toBe("100 bytes");
  expect(screen.queryByText("Updating…")).toBeNull();
  await run.settle(
    volumePartPace({ volumeId: "AQ" }) - EXPLANATION_WAIT_MS - 1,
  );
  expect(run.total()).toBe("100 bytes");
  expect(shownPart().getByText(/^100 bytes/)).toBeDefined();

  await run.settle(1);
  expect(run.total()).toBe("200 bytes");
  expect(screen.getByText("Updating…")).toBeDefined();
  expect(screen.queryByText(/^100 bytes/)).toBeNull();
  await run.answer();
  expect(shownPart().getByText(/^200 bytes/)).toBeDefined();
  expect(shownPart().getByText(/All scanned folders/)).toBeDefined();
  expect(run.watch.mixed()).toEqual([]);
});

test("F02 FR3.HD1.gui-web.DD1: a batch whose part changes to the comparison waits as long as the comparison may take", async () => {
  const run = await summaryRun();
  run.show("v2");
  await run.settle(RESULT_REFLECT_INTERVAL_MS);
  fireEvent.click(
    screen.getByRole("button", {
      name: "Compare all scanned folders on this volume",
    }),
  );
  // The explanation read in flight ends; the comparison of v2 is read next.
  await run.answer();
  expect(run.reads(true)).toEqual(["v2"]);

  await run.settle(EXPLANATION_WAIT_MS);
  expect(run.total()).toBe("100 bytes");
  await run.settle(VOLUME_COMPARE_REREAD_MS - EXPLANATION_WAIT_MS);
  expect(run.total()).toBe("200 bytes");
  await run.answer();
  expect(shownPart().getByText(/^200 bytes/)).toBeDefined();
  expect(run.watch.mixed()).toEqual([]);
});
