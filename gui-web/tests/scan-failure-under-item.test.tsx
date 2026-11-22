import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import { ScanIssues } from "../src/features/analysis/ScanIssues";
import { LocaleTextContext } from "../src/i18n/presentation";
import type {
  AnalysisClient,
  BrowseOpenResponse,
  FolderQueryRow,
  ScanFailures,
  ScanFailuresRequest,
} from "../src/ipc";
import { englishText } from "../preview/result-summary-fixture";

afterEach(cleanup);

const version = "018f0000-0000-7000-8000-0000000000c1";
const read = { state: "present", value: false } as const;
const folder = (
  n: number,
  unreadInside?: FolderQueryRow["flags"]["unreadInside"],
) =>
  ({
    item: { resultVersion: version, itemNo: n },
    displayName: { state: "present", value: "Folder " + n },
    kind: "directory",
    logicalSize: { state: "absent" },
    allocatedSize: { state: "absent" },
    modifiedAt: { state: "absent" },
    hasChildFolder: { state: "absent" },
    flags: {
      unenumerated: read,
      excluded: read,
      ...(unreadInside ? { unreadInside } : {}),
    },
    correspondence: { state: "absent" },
  }) as FolderQueryRow;

function opened(rows: FolderQueryRow[]): BrowseOpenResponse {
  return {
    kind: "opened",
    requestSeq: 1,
    origin: "scan",
    location: null,
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
      completeness: "complete",
      nextCursor: null,
    },
    sortStrategies: [],
    predicateStrategies: [],
  };
}

test("F02 #358: only a folder with an unread part offers its unreadable locations", async () => {
  const viewUnread = vi.fn();
  const port = {
    open: vi.fn((request) =>
      Promise.resolve({
        ...opened([
          folder(1),
          folder(2, { state: "present", value: { count: "2" } }),
        ]),
        requestSeq: request.requestSeq,
      }),
    ),
    page: vi.fn(),
  };
  render(
    <LocaleTextContext.Provider value={englishText}>
      <BrowsePanel
        port={port}
        resultVersion={version}
        viewUnread={viewUnread}
      />
    </LocaleTextContext.Provider>,
  );
  const open = async (name: string) => {
    const triggers = await screen.findAllByRole("button", { name });
    fireEvent.contextMenu(triggers[0]!.closest<HTMLElement>('[role="row"]')!);
    return screen.findByRole("menu", { name });
  };
  const first = await open("Actions for Folder 1");
  expect(
    within(first).queryByRole("menuitem", { name: "View unread locations" }),
  ).toBeNull();
  fireEvent.keyDown(first, { key: "Escape" });
  const menu = await open("Actions for Folder 2");
  fireEvent.click(
    within(menu).getByRole("menuitem", { name: "View unread locations" }),
  );
  expect(viewUnread).toHaveBeenCalledWith(
    { resultVersion: version, itemNo: 2 },
    "Folder 2",
  );
});

test("F02 #358: a folder request restricts the list below it and its chip clears the restriction", async () => {
  const requests: ScanFailuresRequest[] = [];
  const client = {
    call: (async (name: string, body: ScanFailuresRequest) => {
      if (name !== "scan.failures") throw new Error("Unexpected " + name);
      requests.push(body);
      return {
        resultVersion: body.resultVersion,
        rows: [],
        totalCount: 0,
      } satisfies ScanFailures;
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  } satisfies AnalysisClient;
  render(
    <LocaleTextContext.Provider value={englishText}>
      <ScanIssues
        client={client}
        resultVersion={version}
        onReopenLatest={() => undefined}
        laneRequest={{
          requestId: 1,
          storageLabel: "",
          storagePaths: [],
          allStorages: false,
          folder: {
            item: { resultVersion: version, itemNo: 2 },
            label: "Photos",
          },
        }}
      />
    </LocaleTextContext.Provider>,
  );
  await waitFor(() =>
    expect(requests.at(-1)).toMatchObject({
      resultVersion: version,
      failuresOnly: true,
      underItem: { resultVersion: version, itemNo: 2 },
    }),
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "Location: below Photos, clear location filter",
    }),
  );
  await waitFor(() => expect(requests.at(-1)?.underItem).toBeUndefined());
  expect(
    screen.queryByRole("button", { name: /clear location filter/ }),
  ).toBeNull();
});
