import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import { AnalysisError } from "../src/ipc";
import type {
  AnalysisClient,
  BrowseOpenResponse,
  FolderQueryRow,
  ScanPriorityRequest,
  ScanPriorityResponse,
} from "../src/ipc";
import { useScanPriority } from "../src/state/scan-priority";
import type { ScanPriorityContext } from "../src/state/scan-priority";

afterEach(cleanup);

const version = "018f0000-0000-7000-8000-0000000000a1";
const newer = "018f0000-0000-7000-8000-0000000000a2";
const ref = (itemNo: number) => ({ resultVersion: version, itemNo });
const unknown = { state: "unknown", reasonKey: "ref.unavailable" } as const;

function row(
  itemNo: number,
  name: string,
  kind: FolderQueryRow["kind"] = "directory",
  excluded = false,
): FolderQueryRow {
  return {
    item: ref(itemNo),
    displayName: { state: "present", value: name },
    kind,
    logicalSize: unknown,
    allocatedSize: { state: "absent" },
    modifiedAt: { state: "absent" },
    hasChildFolder: unknown,
    flags: {
      unenumerated: { state: "absent" },
      excluded: excluded
        ? { state: "present", value: true }
        : { state: "absent" },
    },
    correspondence: unknown,
  };
}

const rows = [
  row(1, "Photos"),
  row(2, "Videos"),
  row(3, "memo.txt", "file"),
  row(4, "Skipped", "directory", true),
];

function port(): BrowsePort {
  const response: BrowseOpenResponse = {
    kind: "opened",
    requestSeq: 1,
    origin: "scan",
    location: { resultVersion: version, itemNo: 0 },
    breadcrumbs: [],
    rootKind: "scope_root",
    currentCorrespondence: false,
    page: {
      rowKind: "folder",
      queryRef: version,
      resultVersion: version,
      rows,
      total: { state: "exact", count: rows.length },
      unknownCount: 0,
      observedAt: null,
      completeness: "partial",
      nextCursor: null,
    },
    sortStrategies: [],
    predicateStrategies: [],
  };
  return {
    open: vi.fn((request) =>
      Promise.resolve({ ...response, requestSeq: request.requestSeq }),
    ),
    page: vi.fn(),
    copyPath: vi.fn(),
  };
}

type Answer = {
  request: ScanPriorityRequest;
  resolve: (response: ScanPriorityResponse) => void;
  reject: (reason: unknown) => void;
};

function priorityClient() {
  const answers: Answer[] = [];
  const client: AnalysisClient = {
    call: ((name: string, body: unknown) => {
      if (name !== "scan.prioritize") throw new Error(`Unexpected ${name}`);
      return new Promise((resolve, reject) =>
        answers.push({ request: body as ScanPriorityRequest, resolve, reject }),
      );
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  };
  return { client, answers };
}

function Harness({
  client,
  context,
  browsePort,
}: {
  client: AnalysisClient;
  context: ScanPriorityContext | undefined;
  browsePort: BrowsePort;
}) {
  const priority = useScanPriority(client, context);
  return (
    <BrowsePanel
      port={browsePort}
      resultVersion={version}
      priority={priority}
      inspectItem={() => "Details"}
    />
  );
}

const running = { jobRef: "scan", resultVersion: version };

async function setup(context: ScanPriorityContext | undefined = running) {
  const { client, answers } = priorityClient();
  const browsePort = port();
  const view = render(
    <Harness client={client} context={context} browsePort={browsePort} />,
  );
  await screen.findByText("Photos");
  const rerender = (next: ScanPriorityContext | undefined) =>
    view.rerender(
      <Harness client={client} context={next} browsePort={browsePort} />,
    );
  return { answers, browsePort, rerender };
}

function gridRow(name: string) {
  return screen.getByText(name).closest<HTMLElement>('[role="row"]')!;
}

function rowAction(name: string) {
  return within(gridRow(name)).queryByRole("button", { name: "Analyze first" });
}

async function requestFirst(name: string) {
  const button = rowAction(name);
  if (!button) throw new Error(`No Analyze first action for ${name}`);
  fireEvent.click(button);
  await waitFor(() =>
    expect(
      within(gridRow(name)).getByText("Analyze first requested"),
    ).toBeDefined(),
  );
}

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
  expect(screen.queryByText("Analyzing ‘Photos’ first")).toBeNull();

  await act(async () =>
    answers[0]?.resolve({
      controlResult: "accepted",
      appliedPriorityArea: ref(1),
    }),
  );
  expect(screen.getAllByText("Analyzing ‘Photos’ first")).toHaveLength(1);
  expect(
    within(gridRow("Photos")).getByText("Analyze first requested"),
  ).toBeDefined();
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
  expect(
    within(gridRow("Photos")).getByText("Analyze first requested"),
  ).toBeDefined();

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
  expect(screen.queryByText("Analyzing ‘Photos’ first")).toBeNull();
  expect(
    within(gridRow("Videos")).getByText("Analyze first requested"),
  ).toBeDefined();

  // A newer partial result replaced the pinned one before the answer arrived.
  rerender({ jobRef: "scan", resultVersion: newer });
  await act(async () => answers[1]?.resolve({ controlResult: "accepted" }));
  expect(screen.queryByText("Analyzing ‘Videos’ first")).toBeNull();
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
