import {
  act,
  cleanup,
  fireEvent,
  renderHook,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AnalysisError } from "../src/ipc";
import type { AnalysisClient } from "../src/ipc";
import { useScanResult } from "../src/state/analysis-scan";
import { RESULT_REFLECT_INTERVAL_MS } from "../src/state/result-cadence";
import {
  clearSelection,
  coverage,
  liveScan,
  logical,
  passReflectInterval,
  resetReflectOffset,
  select,
  startPanel,
} from "./summary-fixture";

/*
 * F02 FR3.HD1.gui-web.DD1 (user decision 2026-11-15, design review rule 8 and
 * rule 10): the analysis summary reflects a running scan's newer results in
 * place, at most once a second, and holds still only while text in it is
 * selected. Dialogs keep the result they opened with.
 */

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  resetReflectOffset();
  document.getSelection()?.removeAllRanges();
});

test("F02 FR3.HD1.gui-web.DD1: a running scan's newer result replaces the summary values without a selection", async () => {
  const scan = await startPanel();
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(screen.getByText("Values update as the scan runs")).toBeDefined();
  // Neither the changing values nor the line about them is a live region;
  // the state badge alone announces scan state changes.
  expect(
    logical().getByText("100 bytes").closest('[role="status"], [aria-live]'),
  ).toBeNull();
  expect(
    screen
      .getByText("Values update as the scan runs")
      .closest('[role="status"], [aria-live]'),
  ).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
  expect(screen.queryByText(/Newer result available/)).toBeNull();

  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(logical().getByText("200 bytes")).toBeDefined());
  expect(coverage().getByText("2")).toBeDefined();
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
  expect(scan.reads("space.total")).toEqual(["v1", "v2"]);
});

test("F02 FR3.HD1.gui-web.DD1: the summary line follows pause and the final result replaces the interim one", async () => {
  const scan = await startPanel();
  scan.changeState("paused");
  await screen.findByText("Result so far");
  expect(screen.queryByText("Values update as the scan runs")).toBeNull();
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();

  scan.changeState("running");
  await screen.findByText("Values update as the scan runs");
  passReflectInterval();
  scan.publish("v3", "done");
  await screen.findByText("Result version: Final");
  expect(screen.getByText("Scan finished")).toBeDefined();
  expect(logical().getByText("300 bytes")).toBeDefined();
  expect(screen.queryByText("Values update as the scan runs")).toBeNull();
  expect(screen.queryByText("Result so far")).toBeNull();
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
});

test("F02 FR3.HD1.gui-web.DD1: selecting text holds the whole summary until the selection is released", async () => {
  const scan = await startPanel();
  select(logical().getByText("100 bytes"));
  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(scan.reads("space.total")).toEqual(["v1", "v2"]));
  await act(async () => {});
  // Every value of the summary still comes from the first result.
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(coverage().getByText("1")).toBeDefined();

  clearSelection();
  expect(logical().getByText("200 bytes")).toBeDefined();
  expect(coverage().getByText("2")).toBeDefined();
  // Releasing shows what was already read; nothing is read again.
  expect(scan.reads("space.total")).toEqual(["v1", "v2"]);
});

test("F02 FR3.HD1.gui-web.DD1: a selection outside the summary does not hold it", async () => {
  const scan = await startPanel();
  select(screen.getByRole("heading", { name: "Disk space results" }));
  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(logical().getByText("200 bytes")).toBeDefined());
});

test("F02 FR3.HD1.gui-web.DD1: expanded measurement details stay open and focused while values change", async () => {
  const scan = await startPanel();
  const details = screen.getByRole("button", { name: "Measurement details" });
  fireEvent.click(details);
  details.focus();
  const options = screen.getByRole("button", { name: "Display options" });
  fireEvent.click(options);
  details.focus();
  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(logical().getByText("200 bytes")).toBeDefined());
  const measurements = within(
    screen.getByLabelText("Measurement details", { selector: "dl" }),
  );
  expect(measurements.getByText("200 bytes")).toBeDefined();
  expect(screen.getByRole("button", { name: "Measurement details" })).toBe(
    details,
  );
  expect(details.getAttribute("aria-expanded")).toBe("true");
  expect(options.getAttribute("aria-expanded")).toBe("true");
  expect(document.activeElement).toBe(details);
});

test("F02 FR3.HD1.gui-web.DD1: folder details keep the result they opened with and move to the newest one for a further read", async () => {
  const scan = await startPanel();
  fireEvent.click(screen.getByRole("button", { name: "View folder details" }));
  const dialog = within(await screen.findByRole("dialog"));
  await dialog.findByText("100 bytes");
  expect(dialog.getByText("Result published")).toBeDefined();

  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(scan.reads("space.total")).toEqual(["v1", "v2"]));
  await act(async () => {});
  // The open dialog stays and keeps its values; the engine kept nothing for it.
  expect(screen.getByRole("dialog")).toBeDefined();
  expect(dialog.getByText("100 bytes")).toBeDefined();
  expect(dialog.queryByText("Updated to the latest result")).toBeNull();
  expect(scan.reads("browse.detail")).toEqual(["v1"]);

  // A copy refused because the result was replaced moves the dialog to the
  // newest result and copies there once; the user pressed only once.
  fireEvent.click(dialog.getByRole("button", { name: "Copy path" }));
  await dialog.findByText("Updated to the latest result");
  await dialog.findByText("200 bytes");
  await dialog.findByText("Copied 1 path.");
  expect(dialog.queryByText("100 bytes")).toBeNull();
  expect(scan.reads("browse.detail")).toEqual(["v1", "v2"]);
  expect(scan.reads("browse.copy_path")).toEqual(["v1", "v2"]);

  fireEvent.click(dialog.getByRole("button", { name: "Close details" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  fireEvent.click(screen.getByRole("button", { name: "View folder details" }));
  const reopened = within(await screen.findByRole("dialog"));
  await reopened.findByText("200 bytes");
  expect(reopened.queryByText("Updated to the latest result")).toBeNull();
  expect(scan.reads("browse.copy_path")).toEqual(["v1", "v2"]);
});

/** Opens folder details on v1, then lets the scan publish v2 behind it. */
async function folderDetailsBehindLatest() {
  const scan = await startPanel();
  fireEvent.click(screen.getByRole("button", { name: "View folder details" }));
  const dialog = within(await screen.findByRole("dialog"));
  await dialog.findByText("100 bytes");
  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(scan.reads("space.total")).toEqual(["v1", "v2"]));
  return { scan, dialog };
}

test("F02 FR3.HD1.gui-web.DD1: a continued copy refused again is reported and not continued further", async () => {
  const { scan, dialog } = await folderDetailsBehindLatest();
  const call = scan.client.call;
  scan.client.call = ((name: string, body: { resultVersion?: string }) => {
    // The continued copy finds yet another result published first.
    if (name === "browse.copy_path" && body.resultVersion === "v2")
      scan.job.version = "v3";
    return call(name as never, body as never);
  }) as AnalysisClient["call"];

  fireEvent.click(dialog.getByRole("button", { name: "Copy path" }));
  await dialog.findByText("Could not copy the path.");
  expect(scan.reads("browse.copy_path")).toEqual(["v1", "v2"]);

  // Learning of v3 neither moves the dialog nor copies again.
  scan.publish("v3");
  await act(async () => {});
  expect(dialog.getByText("Could not copy the path.")).toBeDefined();
  expect(dialog.getByText("200 bytes")).toBeDefined();
  expect(scan.reads("browse.detail")).toEqual(["v1", "v2"]);
  expect(scan.reads("browse.copy_path")).toEqual(["v1", "v2"]);
});

test.each(["browse.copy_path", "browse.detail"])(
  "F02 FR3.HD1.gui-web.DD1: a continued copy whose item is not in the latest result says so (%s refuses it)",
  async (refusing) => {
    const { scan, dialog } = await folderDetailsBehindLatest();
    const copies: (string | undefined)[] = [];
    const call = scan.client.call;
    scan.client.call = (async (
      name: string,
      body: { resultVersion?: string },
    ) => {
      if (name === "browse.copy_path") copies.push(body.resultVersion);
      if (name === refusing && body.resultVersion === "v2")
        throw new AnalysisError("NOT_FOUND", "ref.unavailable");
      return call(name as never, body as never);
    }) as AnalysisClient["call"];

    fireEvent.click(dialog.getByRole("button", { name: "Copy path" }));
    await dialog.findByText(
      "Couldn't copy the path. The item is no longer in the latest result.",
    );
    await dialog.findByText("Updated to the latest result");
    expect(copies).toEqual(
      refusing === "browse.copy_path" ? ["v1", "v2"] : ["v1"],
    );
    expect(dialog.queryByText(/^Copied/)).toBeNull();
  },
);

test("F02 NFR5.HD1.gui-web.DD1: newer results are read at most once per reflect interval", async () => {
  vi.useFakeTimers({
    toFake: [
      "setTimeout",
      "clearTimeout",
      "setInterval",
      "clearInterval",
      "Date",
    ],
  });
  const frames: FrameRequestCallback[] = [];
  vi.spyOn(window, "requestAnimationFrame").mockImplementation((callback) =>
    frames.push(callback),
  );
  vi.spyOn(window, "cancelAnimationFrame").mockImplementation(() => {});
  const scan = liveScan();
  const readTimes: number[] = [];
  const call = scan.client.call;
  scan.client.call = ((name: string, body: never) => {
    if (name === "space.total") readTimes.push(Date.now());
    return call(name as never, body);
  }) as AnalysisClient["call"];
  const settle = async (ms = 0) => {
    await act(async () => {
      await vi.advanceTimersByTimeAsync(ms);
      for (const frame of frames.splice(0)) frame(0);
      await vi.advanceTimersByTimeAsync(0);
    });
  };
  const hook = renderHook(() =>
    useScanResult(scan.client, {
      jobRef: "scan",
      existing: false,
      scopeVersion: "scope",
      scopeFingerprint: "verified",
      lineageId: "lineage",
    }),
  );
  await settle();
  await settle();
  expect(hook.result.current.total?.resultVersion).toBe("v1");
  const started = readTimes[0]!;

  // Four publications inside one interval lead to a single read of the newest.
  for (const version of ["v2", "v3", "v4", "v5"]) {
    scan.publish(version);
    await settle(200);
  }
  expect(scan.reads("space.total")).toEqual(["v1"]);
  expect(hook.result.current.total?.resultVersion).toBe("v1");
  await settle(Math.max(0, started + RESULT_REFLECT_INTERVAL_MS - Date.now()));
  await settle();
  expect(scan.reads("space.total")).toEqual(["v1", "v5"]);
  expect(hook.result.current.total?.resultVersion).toBe("v5");

  scan.publish("v6");
  await settle(500);
  expect(scan.reads("space.total")).toEqual(["v1", "v5"]);
  await settle(RESULT_REFLECT_INTERVAL_MS);
  await settle();
  expect(scan.reads("space.total")).toEqual(["v1", "v5", "v6"]);
  for (let index = 1; index < readTimes.length; index += 1)
    expect(readTimes[index]! - readTimes[index - 1]!).toBeGreaterThanOrEqual(
      RESULT_REFLECT_INTERVAL_MS,
    );
  expect(hook.result.current.hasNewResult).toBe(false);
});
