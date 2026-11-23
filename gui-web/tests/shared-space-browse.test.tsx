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
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import { FileInspection } from "../src/features/analysis/FileInspection";
import { LocaleTextContext, interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { createAnalysisClient } from "../src/ipc";
import type {
  BrowseOpenResponse,
  FolderQueryRow,
  ItemRef,
  SpaceDetail,
} from "../src/ipc";
import { folderDetail } from "./summary-fixture";

afterEach(cleanup);

const version = "v1";
const ref = (itemNo: number): ItemRef => ({ resultVersion: version, itemNo });
const text: LocaleText = (_key, fallback, values) =>
  interpolate(fallback, (name) => String(values?.[name] ?? "Unknown"));

function opened(row: FolderQueryRow, requestSeq: number): BrowseOpenResponse {
  return {
    kind: "opened",
    requestSeq,
    origin: "scan",
    location: null,
    breadcrumbs: [],
    rootKind: "virtual_root",
    currentCorrespondence: false,
    page: {
      rowKind: "folder",
      queryRef: "query",
      resultVersion: version,
      rows: [row],
      total: { state: "exact", count: 1 },
      unknownCount: 0,
      observedAt: null,
      completeness: "complete",
      nextCursor: null,
    },
    sortStrategies: [],
    predicateStrategies: [],
  };
}

function sharedFileScenario() {
  const sourceRow: FolderQueryRow = {
    item: ref(0),
    displayName: { state: "present", value: "clip.mp4" },
    kind: "file",
    logicalSize: { state: "absent" },
    allocatedSize: { state: "absent" },
    modifiedAt: { state: "absent" },
    hasChildFolder: { state: "absent" },
    correspondence: { state: "absent" },
    flags: { unenumerated: { state: "absent" }, excluded: { state: "absent" } },
  };
  const countedItem = ref(4);
  const countedFolder = ref(2);
  const targetResponse = opened(
    {
      ...sourceRow,
      item: ref(5),
      displayName: { state: "present", value: "Target" },
      kind: "directory",
    },
    2,
  );
  if (targetResponse.kind !== "opened")
    throw new Error("Expected opened response");
  const target = {
    ...targetResponse,
    location: countedFolder,
    rootKind: "folder" as const,
  };
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValueOnce(opened(sourceRow, 1))
      .mockResolvedValueOnce(target),
    page: vi.fn(),
  };
  const space: SpaceDetail = {
    item: sourceRow.item,
    logical: { state: "absent" },
    allocated: { state: "absent" },
    uniquePhysical: { state: "absent" },
    exclusive: { state: "absent" },
    shared: { state: "absent" },
    linkGroup: {
      state: "present",
      value: {
        reportedLinkCount: "2",
        inScopePaths: [sourceRow.item, countedItem],
        sharesSpace: true,
        countedItem,
        countedFolder,
        hasOutsideLinks: false,
      },
    },
    auxiliary: { state: "absent" },
    exclusiveDecision: { confirmed: false, reasonKey: null },
    observedAt: { state: "absent" },
    capabilities: { state: "absent" },
  };
  const client = createAnalysisClient(async (_command, { envelope }) => {
    const item = (envelope.body as { item: ItemRef }).item;
    return {
      ...envelope,
      body:
        envelope.name === "space.detail"
          ? space
          : {
              ...folderDetail(version),
              item,
              displayName: { state: "present", value: "clip.mp4" },
              displayPath: {
                state: "present",
                value: "/Users/minseo/clip.mp4",
              },
              kind: { state: "present", value: "file" },
            },
    };
  });
  render(
    <LocaleTextContext.Provider value={text}>
      <BrowsePanel
        port={port}
        resultVersion={version}
        inspectItem={(
          item,
          kind,
          _countedAt,
          _countedAtItem,
          _group,
          navigateItem,
        ) => (
          <FileInspection
            client={client}
            item={item}
            itemKind={kind}
            {...(navigateItem ? { navigateItem } : {})}
          />
        )}
      />
    </LocaleTextContext.Provider>,
  );
  return { port, countedFolder };
}

test("#333: Previous restores shared-file details after opening its counted folder", async () => {
  const { port, countedFolder } = sharedFileScenario();
  const file = await screen.findByRole("button", { name: "clip.mp4" });
  fireEvent.contextMenu(file.closest('[role="row"]')!);
  const menu = await screen.findByRole("menu", {
    name: "Actions for clip.mp4",
  });
  fireEvent.click(within(menu).getByRole("menuitem", { name: "View details" }));
  const details = await screen.findByRole("dialog");
  fireEvent.click(
    await within(details).findByRole("button", {
      name: "Space measurement details",
    }),
  );
  fireEvent.click(
    await within(details).findByRole("button", { name: "Open folder" }),
  );
  expect(await screen.findByRole("button", { name: "Target" })).toBeTruthy();
  expect(port.open).toHaveBeenLastCalledWith(
    expect.objectContaining({
      location: countedFolder,
      resultVersion: version,
    }),
    expect.any(AbortSignal),
  );

  fireEvent.click(
    screen.getByRole("button", { name: "Back to previous folder" }),
  );
  const restored = await screen.findByRole("dialog");
  fireEvent.click(
    await within(restored).findByRole("button", {
      name: "Space measurement details",
    }),
  );
  await waitFor(() =>
    expect(document.body.textContent).toContain(
      "Counted at /Users/minseo/clip.mp4",
    ),
  );
});
