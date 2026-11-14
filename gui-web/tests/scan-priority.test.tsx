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
import { AnalysisError, scanPriorityText } from "../src/ipc";
import {
  SCAN_PRIORITY_NOTICE_MS,
  useScanPriority,
} from "../src/state/scan-priority";
import type { ScanPriorityContext } from "../src/state/scan-priority";
import {
  gridRow,
  newer,
  priorityClient,
  ref,
  requestFirst,
  rowAction,
  running,
  setup,
  version,
} from "./scan-priority-fixture";
import type { Answer } from "./scan-priority-fixture";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test("F02 FR8.HD1: Analyze first appears only on in-scope folders of the running scan's current result", async () => {
  const { rerender } = await setup();
  expect(rowAction("Photos")).not.toBeNull();
  expect(rowAction("Videos")).not.toBeNull();
  expect(rowAction("memo.txt")).toBeNull();
  expect(rowAction("Skipped")).toBeNull();

  // Another job's or a replaced result, and no partial result yet.
  rerender({ jobRef: "scan", resultVersion: newer });
  await waitFor(() => expect(rowAction("Photos")).toBeNull());
  rerender(undefined);
  expect(rowAction("Photos")).toBeNull();
  fireEvent.contextMenu(gridRow("Photos"));
  const menu = await screen.findByRole("menu", { name: "Actions for Photos" });
  expect(
    within(menu).queryByRole("menuitem", { name: "Analyze first" }),
  ).toBeNull();
});

test("F02 FR8.HD1: a request marks the row before the response, sends once and announces acceptance once", async () => {
  const { answers } = await setup();
  const button = rowAction("Photos")!;
  fireEvent.click(button);
  fireEvent.click(button);
  expect(
    within(gridRow("Photos")).getByText("Analyze first requested"),
  ).toBeDefined();
  expect(rowAction("Photos")).toBeNull();
  expect(answers).toHaveLength(1);
  expect(answers[0]?.request).toEqual({ jobRef: "scan", priorityArea: ref(1) });
  expect(screen.queryByText("‘Photos’ is analyzed first")).toBeNull();

  await act(async () =>
    answers[0]?.resolve({
      controlResult: "accepted",
      appliedPriorityArea: ref(1),
    }),
  );
  expect(screen.getAllByText("‘Photos’ is analyzed first")).toHaveLength(1);
  expect(within(gridRow("Photos")).getByText("Analyzing first")).toBeDefined();
});

test("F02 FR8.HD1: the row reads analyzing first only for the applied area of a running scan", async () => {
  const { answers, rerender } = await setup();
  await requestFirst("Photos");
  await act(async () => answers[0]?.resolve({ controlResult: "accepted" }));
  // Receipt alone does not name the applied area.
  expect(
    within(gridRow("Photos")).getByText("Analyze first requested"),
  ).toBeDefined();

  await requestFirst("Videos");
  await act(async () =>
    answers[1]?.resolve({
      controlResult: "accepted",
      appliedPriorityArea: ref(2),
    }),
  );
  expect(within(gridRow("Videos")).getByText("Analyzing first")).toBeDefined();

  // A paused scan is not working on the folder.
  rerender({ ...running, scanning: false });
  await waitFor(() =>
    expect(
      within(gridRow("Videos")).getByText("Analyze first requested"),
    ).toBeDefined(),
  );
});

test("F02 FR8.HD1: a folder with an exact size is collected and offers no Analyze first", async () => {
  await setup();
  expect(rowAction("Documents")).toBeNull();
  expect(rowAction("Downloads")).not.toBeNull();
  fireEvent.contextMenu(gridRow("Documents"));
  const menu = await screen.findByRole("menu", {
    name: "Actions for Documents",
  });
  expect(
    within(menu).queryByRole("menuitem", { name: "Analyze first" }),
  ).toBeNull();
});

function toggle(name: string) {
  const target = gridRow(name);
  act(() => target.focus());
  fireEvent.keyDown(target, { key: " ", code: "Space" });
  fireEvent.keyUp(target, { key: " ", code: "Space" });
}

function selectionAction() {
  const bar = screen.getByRole("toolbar", { name: "Selection actions" });
  return within(bar).getByRole("button", { name: "Analyze first" });
}

function unavailable() {
  return selectionAction().getAttribute("aria-disabled") === "true";
}

test("F02 FR8.HD1: the selection action bar analyzes one selected folder first and says why it cannot", async () => {
  const { answers, rerender } = await setup();
  expect(
    screen.getByText("Select one folder to analyze it first"),
  ).toBeDefined();
  expect(unavailable()).toBe(true);

  toggle("memo.txt");
  await screen.findByText(
    "1 item selected · Only folders can be analyzed first",
  );
  expect(unavailable()).toBe(true);
  toggle("memo.txt");

  toggle("Documents");
  await screen.findByText(
    "1 item selected · This folder is already fully analyzed",
  );
  expect(unavailable()).toBe(true);
  toggle("Documents");

  toggle("Skipped");
  await screen.findByText(
    "1 item selected · Excluded folders cannot be analyzed first",
  );
  expect(unavailable()).toBe(true);
  toggle("Skipped");

  toggle("Photos");
  toggle("Videos");
  const many = await screen.findByText(
    "2 items selected · Select one folder to analyze it first",
  );
  expect(unavailable()).toBe(true);
  // The unavailable action refers to the summary for its reason.
  expect(selectionAction().getAttribute("aria-describedby")).toBe(many.id);
  toggle("Videos");
  await screen.findByText("1 item selected");
  expect(selectionAction().hasAttribute("aria-disabled")).toBe(false);
  expect(selectionAction().hasAttribute("aria-describedby")).toBe(false);

  fireEvent.click(selectionAction());
  await waitFor(() => expect(answers).toHaveLength(1));
  expect(answers[0]?.request.priorityArea).toEqual(ref(1));
  expect(
    within(gridRow("Photos")).getByText("Analyze first requested"),
  ).toBeDefined();
  // The requested folder is not offered again.
  expect(screen.getByText("1 item selected · Already requested")).toBeDefined();
  expect(unavailable()).toBe(true);
  fireEvent.click(selectionAction());
  expect(answers).toHaveLength(1);

  // Outside the running scan's current result there is no bar.
  rerender(undefined);
  await waitFor(() =>
    expect(
      screen.queryByRole("toolbar", { name: "Selection actions" }),
    ).toBeNull(),
  );
});

test("F02 FR8.HD1: pressing Analyze first by keyboard keeps focus in the bar", async () => {
  const { answers } = await setup();
  toggle("Photos");
  await screen.findByText("1 item selected");
  const button = selectionAction();
  act(() => button.focus());
  fireEvent.keyDown(button, { key: "Enter" });
  fireEvent.keyUp(button, { key: "Enter" });
  await waitFor(() => expect(answers).toHaveLength(1));
  expect(unavailable()).toBe(true);
  expect(document.activeElement).toBe(button);
  expect(
    screen
      .getByRole("toolbar", { name: "Selection actions" })
      .contains(document.activeElement),
  ).toBe(true);
  await act(async () =>
    answers[0]?.resolve({
      controlResult: "accepted",
      appliedPriorityArea: ref(1),
    }),
  );
  // The outcome replaces the summary without moving focus.
  expect(within(selectionBar()!).getByRole("status").textContent).toBe(
    "‘Photos’ is analyzed first",
  );
  expect(document.activeElement).toBe(button);
});

test("F02 FR8.HD1: focus moves to the panel when the bar goes away while holding it", async () => {
  const { rerender } = await setup();
  act(() => selectionAction().focus());
  rerender(undefined);
  await waitFor(() => expect(selectionBar()).toBeNull());
  expect(document.activeElement).toBe(
    screen.getByRole("region", { name: "Folder navigation" }),
  );
});

test("F02 FR8.HD1: no effect keeps the mark only when the folder remains the applied area", async () => {
  const { answers } = await setup();
  await requestFirst("Photos");
  await act(async () =>
    answers[0]?.resolve({
      controlResult: "no_effect",
      appliedPriorityArea: ref(1),
    }),
  );
  expect(
    screen.getByText(
      "This folder is already analyzed or is already being analyzed first.",
    ),
  ).toBeDefined();
  expect(within(gridRow("Photos")).getByText("Analyzing first")).toBeDefined();

  await requestFirst("Videos");
  await act(async () => answers[1]?.resolve({ controlResult: "no_effect" }));
  await waitFor(() => expect(rowAction("Videos")).not.toBeNull());
  expect(
    within(gridRow("Videos")).queryByText("Analyze first requested"),
  ).toBeNull();
});

test("F02 FR8.HD1: an ended scan, a replaced result and other failures clear the request", async () => {
  const { answers, browsePort } = await setup();
  await requestFirst("Photos");
  await act(async () =>
    answers[0]?.resolve({ controlResult: "already_terminal" }),
  );
  expect(screen.getByText("The analysis has finished.")).toBeDefined();
  expect(rowAction("Photos")).not.toBeNull();

  await requestFirst("Photos");
  await act(async () =>
    answers[1]?.reject(
      new AnalysisError("STALE_VERSION", "result.version_stale"),
    ),
  );
  expect(
    screen.getByText(
      "The results changed. Select the folder again in the new results.",
    ),
  ).toBeDefined();
  expect(rowAction("Photos")).not.toBeNull();
  // The view keeps the result it shows; the user selects again in a new one.
  expect(browsePort.open).toHaveBeenCalledTimes(1);

  await requestFirst("Videos");
  await act(async () =>
    answers[2]?.reject(new AnalysisError("NOT_FOUND", "job.not_found")),
  );
  expect(
    screen.getByText("This folder could not be analyzed first."),
  ).toBeDefined();
  expect(
    within(gridRow("Videos")).queryByText("Analyze first requested"),
  ).toBeNull();
});

test("F02 FR8.HD1: superseded and late responses are discarded", async () => {
  const { answers, rerender } = await setup();
  await requestFirst("Photos");
  await requestFirst("Videos");
  expect(
    within(gridRow("Photos")).queryByText("Analyze first requested"),
  ).toBeNull();
  await act(async () => answers[0]?.resolve({ controlResult: "accepted" }));
  expect(screen.queryByText(/‘Photos’/)).toBeNull();
  expect(
    within(gridRow("Videos")).getByText("Analyze first requested"),
  ).toBeDefined();

  // A newer partial result replaced the pinned one before the answer arrived.
  rerender({ jobRef: "scan", resultVersion: newer });
  await act(async () => answers[1]?.resolve({ controlResult: "accepted" }));
  expect(screen.queryByText(/‘Videos’/)).toBeNull();
});

const fallbackText: LocaleText = (_key, fallback, values) =>
  interpolate(fallback, (name) => String(values?.[name] ?? ""));

function selectionBar() {
  return screen.queryByRole("toolbar", { name: "Selection actions" });
}

test("F02 FR8.HD1: the bar shows the outcome in place of its summary with a mark for its kind", async () => {
  const { answers } = await setup();
  const bar = selectionBar()!;
  const status = within(bar).getByRole("status");
  expect(status.textContent).toBe("");
  await requestFirst("Photos");
  await act(async () =>
    answers[0]?.resolve({
      controlResult: "accepted",
      appliedPriorityArea: ref(1),
    }),
  );
  expect(status.textContent).toBe("‘Photos’ is analyzed first");
  expect(status.querySelector('[data-tone="success"] svg')).not.toBeNull();
  expect(
    within(bar).getByText("Select one folder to analyze it first").hidden,
  ).toBe(true);

  await requestFirst("Videos");
  await act(async () => answers[1]?.reject(new Error("closed")));
  expect(status.textContent).toBe("This folder could not be analyzed first.");
  expect(status.querySelector('[data-tone="error"] svg')).not.toBeNull();
  // Nothing floats over the list any more.
  expect(document.querySelector(".du-toast-region")).toBeNull();
});

test("F02 FR8.HD1: the bar stays while an outcome shows after the scan ended, then the summary returns or the bar goes", async () => {
  const { answers, rerender } = await setup();
  await requestFirst("Photos");
  vi.useFakeTimers();
  await act(async () => answers[0]?.resolve({ controlResult: "accepted" }));
  const bar = selectionBar()!;
  act(() => {
    vi.advanceTimersByTime(SCAN_PRIORITY_NOTICE_MS);
  });
  expect(within(bar).getByRole("status").textContent).toBe("");
  expect(
    within(bar).getByText("Select one folder to analyze it first").hidden,
  ).toBe(false);

  fireEvent.click(rowAction("Videos")!);
  rerender(undefined);
  await act(async () =>
    answers[1]?.resolve({ controlResult: "already_terminal" }),
  );
  const ended = selectionBar()!;
  expect(within(ended).getByRole("status").textContent).toBe(
    "The analysis has finished.",
  );
  expect(
    within(ended).getByRole("status").querySelector('[data-tone="info"] svg'),
  ).not.toBeNull();
  act(() => {
    vi.advanceTimersByTime(SCAN_PRIORITY_NOTICE_MS);
  });
  expect(selectionBar()).toBeNull();
});

test("F02 FR8.HD1: a receipt without the applied area can be sent again and returns to neutral", async () => {
  vi.useFakeTimers();
  const { client, answers } = priorityClient();
  const hook = renderHook(
    (context: ScanPriorityContext | undefined) =>
      useScanPriority(client, context),
    { initialProps: running as ScanPriorityContext | undefined },
  );
  const photos = ref(1);
  const ask = () => act(() => hook.result.current.request(photos, "Photos"));
  const receipt = () =>
    act(async () => answers.at(-1)?.resolve({ controlResult: "accepted" }));
  const status = () => hook.result.current.status(photos);

  ask();
  ask();
  // An unanswered request is not sent again.
  expect(answers).toHaveLength(1);
  expect(hook.result.current.requested(photos)).toBe(true);
  await receipt();
  expect(status()).toBe("requested");
  expect(hook.result.current.requested(photos)).toBe(false);

  // The answer allows a new request at once; the mark then lasts the notice time.
  ask();
  expect(answers).toHaveLength(2);
  await receipt();
  act(() => {
    vi.advanceTimersByTime(SCAN_PRIORITY_NOTICE_MS - 1);
  });
  expect(status()).toBe("requested");
  act(() => {
    vi.advanceTimersByTime(1);
  });
  expect(status()).toBeUndefined();

  // A replaced result ends the mark before the time is up.
  ask();
  await receipt();
  hook.rerender({ ...running, resultVersion: newer });
  expect(status()).toBeUndefined();
  hook.rerender(running);

  // An applied area keeps its mark and is not sent again.
  ask();
  await act(async () =>
    answers.at(-1)?.resolve({
      controlResult: "accepted",
      appliedPriorityArea: photos,
    }),
  );
  act(() => {
    vi.advanceTimersByTime(SCAN_PRIORITY_NOTICE_MS * 2);
  });
  expect(status()).toBe("analyzing");
  ask();
  expect(answers).toHaveLength(4);
});

test("F02 FR8.HD1: a notice says only what the response confirmed", async () => {
  const { client, answers } = priorityClient();
  const hook = renderHook(
    (context: ScanPriorityContext | undefined) =>
      useScanPriority(client, context),
    { initialProps: running as ScanPriorityContext | undefined },
  );
  const ask = async (
    itemNo: number,
    name: string,
    answer: (reply: Answer) => void,
  ) => {
    act(() => hook.result.current.request(ref(itemNo), name));
    await act(async () => answer(answers.at(-1)!));
    const notice = hook.result.current.notice;
    return notice && scanPriorityText(notice, fallbackText);
  };
  const receipt = (reply: Answer) =>
    reply.resolve({ controlResult: "accepted" });
  const applied = (itemNo: number) => (reply: Answer) =>
    reply.resolve({
      controlResult: "accepted",
      appliedPriorityArea: ref(itemNo),
    });

  expect(await ask(1, "Photos", receipt)).toBe(
    "Requested to analyze ‘Photos’ first",
  );
  // Receipt alone does not say the replaced folder changed its order.
  expect(await ask(2, "Videos", receipt)).toBe(
    "Requested to analyze ‘Videos’ first. This replaces the request for ‘Photos’.",
  );
  expect(await ask(1, "Photos", applied(1))).toBe(
    "‘Photos’ is now analyzed first. ‘Videos’ returns to its normal order.",
  );

  // A paused scan applies the area when it continues.
  hook.rerender({ ...running, scanning: false });
  expect(await ask(2, "Videos", applied(2))).toBe(
    "‘Videos’ will be analyzed first when the scan continues. ‘Photos’ returns to its normal order.",
  );
  // A failed request leaves no requested folder for the next one to replace.
  expect(
    await ask(3, "Music", (reply) => reply.reject(new Error("closed"))),
  ).toBe("This folder could not be analyzed first.");
  expect(await ask(4, "Games", applied(4))).toBe(
    "‘Games’ will be analyzed first when the scan continues",
  );
  hook.rerender(running);
  expect(await ask(5, "Books", applied(5))).toBe(
    "‘Books’ is now analyzed first. ‘Games’ returns to its normal order.",
  );
});

test("F02 FR8.HD1: the row menu offers Analyze first by right click and by keyboard", async () => {
  const { answers } = await setup();
  fireEvent.contextMenu(gridRow("memo.txt"));
  const fileMenu = await screen.findByRole("menu", {
    name: "Actions for memo.txt",
  });
  expect(
    within(fileMenu).queryByRole("menuitem", { name: "Analyze first" }),
  ).toBeNull();
  fireEvent.keyDown(fileMenu, { key: "Escape" });
  await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());

  const folder = gridRow("Photos");
  folder.focus();
  fireEvent.keyDown(folder, { key: "F10", shiftKey: true });
  const menu = await screen.findByRole("menu", { name: "Actions for Photos" });
  const item = within(menu).getByRole("menuitem", { name: "Analyze first" });
  fireEvent.keyDown(item, { key: "Enter" });
  fireEvent.keyUp(item, { key: "Enter" });
  await waitFor(() => expect(answers).toHaveLength(1));
  expect(answers[0]?.request.priorityArea).toEqual(ref(1));
  expect(
    within(gridRow("Photos")).getByText("Analyze first requested"),
  ).toBeDefined();
});

test("F02 FR8.HD1: an outcome is offered for a short time and only to its own scan", async () => {
  vi.useFakeTimers();
  const { client, answers } = priorityClient();
  const hook = renderHook(
    (context: ScanPriorityContext | undefined) =>
      useScanPriority(client, context),
    { initialProps: running as ScanPriorityContext | undefined },
  );
  const ask = async (itemNo: number, name: string) => {
    act(() => hook.result.current.request(ref(itemNo), name));
    await act(async () =>
      answers.at(-1)?.resolve({
        controlResult: "accepted",
        appliedPriorityArea: ref(itemNo),
      }),
    );
  };

  await ask(1, "Photos");
  expect(hook.result.current.notice?.name).toBe("Photos");
  // A view that mounts after the display time finds nothing to announce.
  act(() => {
    vi.advanceTimersByTime(SCAN_PRIORITY_NOTICE_MS);
  });
  expect(hook.result.current.notice).toBeUndefined();

  await ask(2, "Videos");
  expect(hook.result.current.notice?.jobRef).toBe("scan");
  // A later scan does not announce the outcome of an earlier one.
  hook.rerender({ jobRef: "later", resultVersion: version, scanning: true });
  expect(hook.result.current.notice).toBeUndefined();
  // Without a current scan the outcome of the ended one is still announced.
  hook.rerender(undefined);
  expect(hook.result.current.notice?.name).toBe("Videos");
});
