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
import { LocaleTextContext } from "../src/i18n/presentation";
import { interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { createAnalysisClient, unixPath } from "../src/ipc";
import type { BrowseOpenResponse, FolderQueryRow, ItemRef } from "../src/ipc";
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

test.each(["data", "future.role"])(
  "shows data-volume information only for the data role (%s)",
  async (groupVolumeRole) => {
    const dataRow: FolderQueryRow = {
      item: ref(0),
      displayName: { state: "present", value: "Data" },
      kind: "directory",
      logicalSize: { state: "absent" },
      allocatedSize: { state: "absent" },
      modifiedAt: { state: "absent" },
      hasChildFolder: { state: "absent" },
      correspondence: { state: "absent" },
      flags: {
        unenumerated: { state: "absent" },
        excluded: { state: "absent" },
        boundary: { state: "present", value: "already_counted" },
        countedAt: unixPath("/Users"),
        countedAtItem: ref(2),
        groupVolumeRole,
      },
    };
    const targetRow: FolderQueryRow = {
      ...dataRow,
      item: ref(3),
      displayName: { state: "present", value: "Home" } as const,
      flags: {
        unenumerated: { state: "absent" } as const,
        excluded: { state: "absent" } as const,
      },
    };
    const port: BrowsePort = {
      open: vi
        .fn()
        .mockResolvedValueOnce(opened(dataRow, 1))
        .mockResolvedValueOnce(opened(targetRow, 2)),
      page: vi.fn(),
    };
    const client = createAnalysisClient(async (_command, { envelope }) => ({
      ...envelope,
      body: envelope.name === "browse.detail" ? folderDetail(version) : {},
    }));

    render(
      <LocaleTextContext.Provider value={text}>
        <BrowsePanel
          port={port}
          resultVersion={version}
          inspectItem={(
            item,
            kind,
            countedAt,
            countedAtItem,
            groupVolumeRole,
            navigateItem,
            focusCountedAtItem,
            alreadyCounted,
          ) => (
            <FileInspection
              client={client}
              item={item}
              itemKind={kind}
              {...(countedAt ? { countedAt } : {})}
              {...(countedAtItem ? { countedAtItem } : {})}
              {...(groupVolumeRole ? { groupVolumeRole } : {})}
              {...(navigateItem ? { navigateItem } : {})}
              {...(focusCountedAtItem !== undefined
                ? { focusCountedAtItem }
                : {})}
              {...(alreadyCounted ? { alreadyCounted } : {})}
            />
          )}
        />
      </LocaleTextContext.Provider>,
    );

    const name = await screen.findByRole("button", { name: "Data" });
    const explanation =
      groupVolumeRole === "data"
        ? "This is the data volume of Macintosh HD. User folders and apps are counted under /."
        : undefined;
    expect(name.closest<HTMLElement>("[title]")?.title || undefined).toBe(
      explanation,
    );
    fireEvent.contextMenu(name.closest('[role="row"]')!);
    const menu = await screen.findByRole("menu", { name: "Actions for Data" });
    fireEvent.click(
      within(menu).getByRole("menuitem", { name: "View details" }),
    );
    const dialog = await screen.findByRole("dialog", {
      name: "Folder details",
    });
    if (explanation) expect(within(dialog).getByText(explanation)).toBeTruthy();
    else
      expect(
        within(dialog).queryByText(/data volume of Macintosh HD/),
      ).toBeNull();
    expect(within(dialog).getAllByText("Not counted")).toHaveLength(2);
    expect(within(dialog).getByText("Counted at /Users")).toBeTruthy();
    fireEvent.click(
      within(dialog).getByRole("button", { name: "Open folder" }),
    );
    await waitFor(() => expect(port.open).toHaveBeenCalledTimes(2));
    expect(port.open).toHaveBeenLastCalledWith(
      expect.objectContaining({ location: ref(2), resultVersion: version }),
      expect.any(AbortSignal),
    );
    expect(await screen.findByRole("button", { name: "Home" })).toBeTruthy();
    fireEvent.click(
      screen.getByRole("button", { name: "Back to previous folder" }),
    );
    const restoredDialog = await screen.findByRole("dialog", {
      name: "Folder details",
    });
    expect(
      within(restoredDialog).getByRole("button", { name: "Open folder" }),
    ).toBeTruthy();
    fireEvent.click(
      within(restoredDialog).getByRole("button", { name: "Close details" }),
    );
    expect(await screen.findByRole("button", { name: "Data" })).toBeTruthy();
    expect(screen.queryByRole("button", { name: "Home" })).toBeNull();
  },
);

function alreadyCountedScenario() {
  const sourceRow: FolderQueryRow = {
    item: ref(0),
    displayName: { state: "present", value: "Users" },
    kind: "directory",
    logicalSize: { state: "absent" },
    allocatedSize: { state: "absent" },
    modifiedAt: { state: "absent" },
    hasChildFolder: { state: "absent" },
    correspondence: { state: "absent" },
    flags: {
      unenumerated: { state: "absent" },
      excluded: { state: "absent" },
      boundary: { state: "present", value: "already_counted" },
      countedAt: unixPath("/Users"),
      countedAtItem: ref(2),
    },
  };
  const targetRow: FolderQueryRow = {
    ...sourceRow,
    item: ref(2),
    displayName: { state: "present", value: "Home" },
    flags: { unenumerated: { state: "absent" }, excluded: { state: "absent" } },
  };
  const childRow: FolderQueryRow = {
    ...targetRow,
    item: ref(3),
    displayName: { state: "present", value: "Child" },
  };
  const targetResponse = opened(targetRow, 2);
  if (targetResponse.kind !== "opened")
    throw new Error("Expected opened response");
  const target = {
    ...targetResponse,
    location: ref(2),
    rootKind: "folder" as const,
    breadcrumbs: [
      {
        item: ref(1),
        displayName: { state: "present" as const, value: "Parent" },
      },
      {
        item: ref(2),
        displayName: { state: "present" as const, value: "Home" },
      },
    ],
  };
  const child = opened(childRow, 3);
  const port: BrowsePort = {
    open: vi
      .fn()
      .mockResolvedValueOnce(
        (() => {
          const response = opened(sourceRow, 1);
          if (response.kind !== "opened") return response;
          const system = ref(10);
          const volumes = ref(11);
          const data = ref(12);
          return {
            ...response,
            location: data,
            rootKind: "folder",
            breadcrumbs: [
              {
                item: system,
                displayName: { state: "present", value: "System" },
              },
              {
                item: volumes,
                displayName: { state: "present", value: "Volumes" },
              },
              { item: data, displayName: { state: "present", value: "Data" } },
            ],
          };
        })(),
      )
      .mockResolvedValueOnce(target)
      .mockResolvedValueOnce(child)
      .mockResolvedValueOnce(child),
    page: vi.fn(),
  };
  const client = createAnalysisClient(async (_command, { envelope }) => ({
    ...envelope,
    body: envelope.name === "browse.detail" ? folderDetail(version) : {},
  }));
  render(
    <LocaleTextContext.Provider value={text}>
      <BrowsePanel
        port={port}
        resultVersion={version}
        inspectItem={(
          item,
          kind,
          countedAt,
          countedAtItem,
          _groupVolumeRole,
          navigateItem,
          focusCountedAtItem,
          alreadyCounted,
        ) => (
          <FileInspection
            client={client}
            item={item}
            itemKind={kind}
            {...(countedAt ? { countedAt } : {})}
            {...(countedAtItem ? { countedAtItem } : {})}
            {...(navigateItem ? { navigateItem } : {})}
            {...(focusCountedAtItem !== undefined
              ? { focusCountedAtItem }
              : {})}
            {...(alreadyCounted ? { alreadyCounted } : {})}
          />
        )}
      />
    </LocaleTextContext.Provider>,
  );
  return { port };
}

async function openAlreadyCountedDetails() {
  const { port } = alreadyCountedScenario();
  const sourceName = await screen.findByRole("button", { name: "Users" });
  expect(screen.getByLabelText("Folder path").textContent).toContain(
    "System/Volumes/Data",
  );
  fireEvent.contextMenu(sourceName.closest('[role="row"]')!);
  const menu = await screen.findByRole("menu", { name: "Actions for Users" });
  fireEvent.click(within(menu).getByRole("menuitem", { name: "View details" }));
  const sourceDetails = await screen.findByRole("dialog", {
    name: "Folder details",
  });
  fireEvent.click(
    within(sourceDetails).getByRole("button", { name: "Open folder" }),
  );
  expect(await screen.findByRole("button", { name: "Home" })).toBeTruthy();
  return { port, sourceDetails };
}

test("Previous restores the already-counted Users folder, details, and focus", async () => {
  await openAlreadyCountedDetails();
  const previous = screen.getByRole("button", {
    name: "Back to previous folder",
  });
  expect(previous.className).toContain("du-action--quiet");
  expect(previous.querySelector(".lucide-arrow-left")).toBeTruthy();
  expect(previous.parentElement?.firstElementChild).toBe(previous);
  await waitFor(() =>
    expect(previous.title).toBe("/System/Volumes/Data/Users"),
  );
  fireEvent.click(previous);
  const restoredDetails = await screen.findByRole("dialog", {
    name: "Folder details",
  });
  expect(within(restoredDetails).getByText("Counted at /Users")).toBeTruthy();
  const openFolder = within(restoredDetails).getByRole("button", {
    name: "Open folder",
  });
  await waitFor(() => expect(document.activeElement).toBe(openFolder));
  fireEvent.click(
    within(restoredDetails).getByRole("button", { name: "Close details" }),
  );
  expect(await screen.findByRole("button", { name: "Users" })).toBeTruthy();
  expect(screen.queryByRole("button", { name: "Home" })).toBeNull();
});

test.each(["up", "row", "breadcrumb"] as const)(
  "clears Previous after a later %s navigation",
  async (navigation) => {
    const { port } = await openAlreadyCountedDetails();
    expect(
      screen.getByRole("button", { name: "Back to previous folder" }),
    ).toBeTruthy();
    if (navigation === "up") {
      fireEvent.click(screen.getByRole("button", { name: "Up" }));
    } else if (navigation === "row") {
      fireEvent.click(screen.getByRole("button", { name: "Home" }));
    } else {
      fireEvent.click(
        within(
          screen.getByRole("navigation", { name: "Folder path" }),
        ).getByRole("button", { name: "Parent" }),
      );
    }
    await waitFor(() => expect(port.open).toHaveBeenCalledTimes(3));
    await waitFor(() =>
      expect(
        screen.queryByRole("button", { name: "Back to previous folder" }),
      ).toBeNull(),
    );
  },
);
