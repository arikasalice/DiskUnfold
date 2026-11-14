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
import { interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { scanPriorityText } from "../src/ipc";
import type { BrowseOpenResponse } from "../src/ipc";
import { useScanPriority } from "../src/state/scan-priority";
import type { ScanPriorityContext } from "../src/state/scan-priority";
import {
  gridRow,
  newer,
  priorityClient,
  ref,
  rowAction,
  running,
  setup,
  version,
} from "./scan-priority-fixture";

/*
 * F02 FR8.HD1 follow-up of PR #45: a folder list that shows an earlier result
 * of the running scan keeps its selection action bar, and responses that
 * arrive after the scan stopped running claim only a receipt.
 */

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

const REASON =
  "A newer result is available · Switch to it to analyze a folder first";
/** The scan published a newer result while the list still shows the fixture's. */
const moved: ScanPriorityContext = { ...running, resultVersion: newer };

function bar() {
  return screen.queryByRole("toolbar", { name: "Selection actions" });
}

function barAction(name: string) {
  return within(bar()!).queryByRole("button", { name });
}

function toggle(name: string) {
  const target = gridRow(name);
  act(() => target.focus());
  fireEvent.keyDown(target, { key: " ", code: "Space" });
  fireEvent.keyUp(target, { key: " ", code: "Space" });
}

async function rowMenu(name: string) {
  fireEvent.contextMenu(gridRow(name));
  return screen.findByRole("menu", { name: "Actions for " + name });
}

async function closeMenu(menu: HTMLElement) {
  fireEvent.keyDown(menu, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
}

function analyzeFirstItem(menu: HTMLElement) {
  return within(menu)
    .queryAllByRole("menuitem")
    .find((item) => item.textContent?.startsWith("Analyze first"));
}

test("F02 FR8.HD1: an earlier result of the running scan keeps the bar with the reason first", async () => {
  const open = vi.fn();
  const { answers } = await setup(moved, {
    jobRef: "scan",
    resultVersion: version,
    open,
  });
  expect(bar()).not.toBeNull();
  const summary = screen.getByText(REASON);
  const action = barAction("Analyze first")!;
  expect(action.getAttribute("aria-disabled")).toBe("true");
  expect(action.getAttribute("aria-describedby")).toBe(summary.id);

  // The reason comes before any reason about the selection, even a usable one.
  toggle("Photos");
  await waitFor(() =>
    expect(gridRow("Photos").getAttribute("aria-selected")).toBe("true"),
  );
  expect(screen.getByText(REASON)).toBeDefined();
  expect(screen.queryByText("1 item selected")).toBeNull();
  toggle("Videos");
  expect(screen.getByText(REASON)).toBeDefined();
  toggle("Videos");

  fireEvent.click(action);
  act(() => action.focus());
  fireEvent.keyDown(action, { key: "Enter" });
  fireEvent.keyUp(action, { key: "Enter" });
  expect(answers).toHaveLength(0);
  expect(document.activeElement).toBe(action);
  // Row buttons stay hidden.
  expect(rowAction("Photos")).toBeNull();
  expect(rowAction("Videos")).toBeNull();
  expect(open).not.toHaveBeenCalled();
});

test("F02 FR8.HD1: Show latest result opens the scan's newest result through the host", async () => {
  const open = vi.fn();
  const view = { jobRef: "scan", resultVersion: version, open };
  const { rerender } = await setup(moved, view);
  fireEvent.click(barAction("Show latest result")!);
  expect(open).toHaveBeenCalledTimes(1);
  expect(open).toHaveBeenCalledWith(newer);

  // A paused scan offers the same; a host that cannot switch offers only the reason.
  rerender({ ...moved, scanning: false, paused: true });
  expect(screen.getByText(REASON)).toBeDefined();
  expect(barAction("Show latest result")).not.toBeNull();
  rerender(moved, { jobRef: "scan", resultVersion: version });
  expect(screen.getByText(REASON)).toBeDefined();
  expect(barAction("Show latest result")).toBeNull();

  // The current result offers Analyze first as before and nothing to switch to.
  rerender(running, view);
  await waitFor(() => expect(rowAction("Photos")).not.toBeNull());
  expect(screen.queryByText(REASON)).toBeNull();
  expect(barAction("Show latest result")).toBeNull();
  expect(
    screen.getByText("Select one folder to analyze it first"),
  ).toBeDefined();
});

test("F02 FR8.HD1: the row menu keeps Analyze first unavailable with the reason", async () => {
  const { answers } = await setup(moved, {
    jobRef: "scan",
    resultVersion: version,
  });
  const menu = await rowMenu("Photos");
  const item = analyzeFirstItem(menu)!;
  expect(item.getAttribute("aria-disabled")).toBe("true");
  expect(item.textContent).toContain(REASON);
  fireEvent.click(item);
  fireEvent.keyDown(item, { key: "Enter" });
  fireEvent.keyUp(item, { key: "Enter" });
  expect(answers).toHaveLength(0);
  await closeMenu(menu);

  // Rows that could not be analyzed first in any result offer nothing.
  for (const name of ["memo.txt", "Skipped", "Documents"]) {
    const other = await rowMenu(name);
    expect(analyzeFirstItem(other)).toBeUndefined();
    await closeMenu(other);
  }
});

test("F02 FR8.HD1: another scan's result or a list of unknown origin shows no bar", async () => {
  // Another scan's result, as when the list was opened from an earlier scan.
  const { rerender } = await setup(moved, {
    jobRef: "earlier",
    resultVersion: version,
    open: vi.fn(),
  });
  expect(bar()).toBeNull();
  expect(rowAction("Photos")).toBeNull();
  let menu = await rowMenu("Photos");
  expect(analyzeFirstItem(menu)).toBeUndefined();
  await closeMenu(menu);

  // A sample or a result opened elsewhere names no scan.
  rerender(moved, undefined);
  expect(bar()).toBeNull();
  menu = await rowMenu("Photos");
  expect(analyzeFirstItem(menu)).toBeUndefined();
  await closeMenu(menu);

  // Without a running scan nothing is superseded.
  rerender(undefined, { jobRef: "scan", resultVersion: version });
  expect(bar()).toBeNull();
});

const fallbackText: LocaleText = (_key, fallback, values) =>
  interpolate(fallback, (name) => String(values?.[name] ?? ""));

test("F02 FR8.HD1: a response after the scan stopped running reads only as a receipt", async () => {
  const { client, answers } = priorityClient();
  const hook = renderHook(
    (context: ScanPriorityContext | undefined) =>
      useScanPriority(client, context),
    { initialProps: running as ScanPriorityContext | undefined },
  );
  const ask = async (
    itemNo: number,
    name: string,
    at: ScanPriorityContext | undefined,
  ) => {
    act(() => hook.result.current.request(ref(itemNo), name));
    hook.rerender(at);
    await act(async () =>
      answers.at(-1)!.resolve({
        controlResult: "accepted",
        appliedPriorityArea: ref(itemNo),
      }),
    );
    const notice = hook.result.current.notice;
    hook.rerender(running);
    return notice && scanPriorityText(notice, fallbackText);
  };

  // The scan ended before the answer: it will not analyze the folder first.
  expect(await ask(1, "Photos", undefined)).toBe(
    "Requested to analyze ‘Photos’ first",
  );
  // A scan being cancelled is neither running nor paused.
  const cancelling = { ...running, scanning: false };
  expect(await ask(2, "Videos", cancelling)).toBe(
    "Requested to analyze ‘Videos’ first. This replaces the request for ‘Photos’.",
  );
  // A paused scan still applies the area when it continues.
  expect(
    await ask(3, "Music", { ...running, scanning: false, paused: true }),
  ).toBe(
    "‘Music’ will be analyzed first when the scan continues. ‘Videos’ returns to its normal order.",
  );
  expect(await ask(4, "Games", running)).toBe(
    "‘Games’ is now analyzed first. ‘Music’ returns to its normal order.",
  );
});

function openedAt(
  first: BrowseOpenResponse,
  requestSeq: number,
  folder: number | undefined,
): BrowseOpenResponse {
  if (first.kind !== "opened" || folder === undefined || folder === 0)
    return { ...first, requestSeq };
  const crumb = (itemNo: number, name: string) => ({
    item: ref(itemNo),
    displayName: { state: "present", value: name } as const,
  });
  return {
    ...first,
    requestSeq,
    rootKind: "folder",
    location: ref(folder),
    breadcrumbs: [crumb(0, "Root"), crumb(folder, "Videos")],
  };
}

test("F02 FR8.HD1: opening another folder or going up clears the selection with the grid", async () => {
  const { browsePort } = await setup();
  const open = vi.mocked(browsePort.open);
  const first = (await open.mock.results[0]!.value) as BrowseOpenResponse;
  open.mockImplementation((request) =>
    Promise.resolve(
      openedAt(first, request.requestSeq, request.location?.itemNo),
    ),
  );
  const selected = () => gridRow("Photos").getAttribute("aria-selected");

  toggle("Photos");
  await screen.findByText("1 item selected");
  expect(selected()).toBe("true");
  fireEvent.keyDown(gridRow("Videos"), { key: "Enter", code: "Enter" });
  await waitFor(() => expect(open).toHaveBeenCalledTimes(2));
  expect(open.mock.calls[1]![0].location).toEqual(ref(2));
  await screen.findByText("Select one folder to analyze it first");
  expect(selected()).toBe("false");

  toggle("Photos");
  await screen.findByText("1 item selected");
  const up = screen.getByRole("button", { name: "Up" });
  await waitFor(() => expect(up.hasAttribute("disabled")).toBe(false));
  fireEvent.click(up);
  await waitFor(() => expect(open).toHaveBeenCalledTimes(3));
  expect(open.mock.calls[2]![0].location).toEqual(ref(0));
  await screen.findByText("Select one folder to analyze it first");
  expect(selected()).toBe("false");
});
