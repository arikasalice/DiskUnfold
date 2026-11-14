import {
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { expect, vi } from "vitest";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import type {
  AnalysisClient,
  BrowseOpenResponse,
  FolderQueryRow,
  ScanPriorityRequest,
  ScanPriorityResponse,
} from "../src/ipc";
import { useScanPriority } from "../src/state/scan-priority";
import type { ScanPriorityContext } from "../src/state/scan-priority";

/*
 * A folder list of the running scan's current result and a scan.prioritize
 * client whose answers the test resolves, shared by the Analyze first tests.
 */

export const version = "018f0000-0000-7000-8000-0000000000a1";
export const newer = "018f0000-0000-7000-8000-0000000000a2";
export const ref = (itemNo: number) => ({ resultVersion: version, itemNo });
const unknown = { state: "unknown", reasonKey: "ref.unavailable" } as const;

function row(
  itemNo: number,
  name: string,
  kind: FolderQueryRow["kind"] = "directory",
  excluded = false,
  logicalSize: FolderQueryRow["logicalSize"] = unknown,
): FolderQueryRow {
  return {
    item: ref(itemNo),
    displayName: { state: "present", value: name },
    kind,
    logicalSize,
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
  row(5, "Documents", "directory", false, {
    state: "present",
    value: {
      basis: "logical",
      evidence: "os_reported",
      state: "exact",
      bytes: "4096",
    },
  }),
  row(6, "Downloads", "directory", false, {
    state: "present",
    value: {
      basis: "logical",
      evidence: "os_reported",
      state: "lower_bound",
      bytes: "4096",
    },
  }),
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

export type Answer = {
  request: ScanPriorityRequest;
  resolve: (response: ScanPriorityResponse) => void;
  reject: (reason: unknown) => void;
};

export function priorityClient() {
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

export const running = {
  jobRef: "scan",
  resultVersion: version,
  scanning: true,
};

export async function setup(
  context: ScanPriorityContext | undefined = running,
) {
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

export function gridRow(name: string) {
  return screen.getByText(name).closest<HTMLElement>('[role="row"]')!;
}

export function rowAction(name: string) {
  return within(gridRow(name)).queryByRole("button", { name: "Analyze first" });
}

export async function requestFirst(name: string) {
  const button = rowAction(name);
  if (!button) throw new Error(`No Analyze first action for ${name}`);
  fireEvent.click(button);
  await waitFor(() =>
    expect(
      within(gridRow(name)).getByText("Analyze first requested"),
    ).toBeDefined(),
  );
}
