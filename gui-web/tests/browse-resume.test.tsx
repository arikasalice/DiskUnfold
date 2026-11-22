import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import type { BrowseResume } from "../src/features/browse/browse-resume";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { BrowseOpenResponse, FolderQueryRow, ItemRef } from "../src/ipc";
import { rowKey } from "../src/features/browse/browse-display";
import { englishText } from "../preview/result-summary-fixture";

afterEach(cleanup);

const version = "018f0000-0000-7000-8000-0000000000d1";
const read = { state: "present", value: false } as const;
const ref = (itemNo: number): ItemRef => ({ resultVersion: version, itemNo });
const folder = (n: number) =>
  ({
    item: ref(n),
    displayName: { state: "present", value: "Folder " + n },
    kind: "directory",
    logicalSize: { state: "absent" },
    allocatedSize: { state: "absent" },
    modifiedAt: { state: "absent" },
    hasChildFolder: { state: "absent" },
    flags: { unenumerated: read, excluded: read },
    correspondence: { state: "absent" },
  }) as FolderQueryRow;

function opened(location: ItemRef | null, rows: FolderQueryRow[]) {
  return {
    kind: "opened",
    requestSeq: 1,
    origin: "scan",
    location,
    breadcrumbs: [],
    rootKind: location ? "folder" : "scope_root",
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
  } as unknown as BrowseOpenResponse;
}

function panel(
  open: BrowsePort["open"],
  resume: { current: BrowseResume | undefined },
  resultVersion = version,
) {
  render(
    <LocaleTextContext.Provider value={englishText}>
      <BrowsePanel
        port={{ open, page: vi.fn() }}
        resultVersion={resultVersion}
        resume={resume}
      />
    </LocaleTextContext.Provider>,
  );
}

test("F02 #406: a saved folder of the same result opens again where the user left it", async () => {
  const open = vi.fn<BrowsePort["open"]>((request) =>
    Promise.resolve({
      ...opened(request.location ?? null, [folder(5)]),
      requestSeq: request.requestSeq,
    }),
  );
  const resume = {
    current: {
      version,
      location: ref(3),
      selected: [rowKey(ref(5)), "not-drawn"],
      scrollTop: 0,
    } satisfies BrowseResume as BrowseResume | undefined,
  };
  panel(open, resume);
  await screen.findAllByText("Folder 5");
  expect(open).toHaveBeenCalledTimes(1);
  expect(open.mock.calls[0]?.[0]).toMatchObject({ location: ref(3) });
  expect(resume.current?.location).toEqual(ref(3));
  // The saved selection of the restored folder stays, except a row that is not drawn.
  expect(resume.current?.selected).toEqual([rowKey(ref(5))]);
});

test("F02 #406: another result, or none saved, opens the scope roots and records them", async () => {
  const open = vi.fn<BrowsePort["open"]>((request) =>
    Promise.resolve({
      ...opened(null, [folder(5)]),
      requestSeq: request.requestSeq,
    }),
  );
  const resume = {
    current: {
      version: "018f0000-0000-7000-8000-0000000000d2",
      location: ref(3),
      selected: ["x"],
      scrollTop: 40,
    } satisfies BrowseResume as BrowseResume | undefined,
  };
  panel(open, resume);
  await screen.findAllByText("Folder 5");
  expect(open.mock.calls[0]?.[0]).not.toHaveProperty("location");
  expect(resume.current).toEqual({
    version,
    location: undefined,
    selected: [],
    scrollTop: 0,
    focusRestored: false,
  });
});

test("F02 #406: a saved folder that can no longer be opened falls back to the scope roots", async () => {
  const open = vi.fn<BrowsePort["open"]>((request) =>
    request.location
      ? Promise.reject(new Error("gone"))
      : Promise.resolve({
          ...opened(null, [folder(5)]),
          requestSeq: request.requestSeq,
        }),
  );
  const resume = {
    current: {
      version,
      location: ref(3),
      selected: [],
      scrollTop: 0,
    } satisfies BrowseResume as BrowseResume | undefined,
  };
  panel(open, resume);
  await screen.findAllByText("Folder 5");
  await waitFor(() => expect(open).toHaveBeenCalledTimes(2));
  expect(open.mock.calls[1]?.[0]).not.toHaveProperty("location");
  expect(resume.current?.location).toBeUndefined();
  expect(screen.queryByText(/could not/i)).toBeNull();
});

test("F02 #406: a restored selection takes the focus, and without one the first row does", async () => {
  const open = vi.fn<BrowsePort["open"]>((request) =>
    Promise.resolve({
      ...opened(request.location ?? null, [folder(5), folder(6)]),
      requestSeq: request.requestSeq,
    }),
  );
  const resume = {
    current: {
      version,
      location: ref(3),
      selected: [rowKey(ref(6))],
      scrollTop: 0,
    } satisfies BrowseResume as BrowseResume | undefined,
  };
  panel(open, resume);
  await screen.findAllByText("Folder 6");
  await waitFor(() =>
    expect(
      document.activeElement
        ?.closest("[data-row]")
        ?.getAttribute("aria-selected"),
    ).toBe("true"),
  );
  // The focus is asked for once: a later list does not take it again.
  expect(resume.current?.focusRestored).toBe(false);
});
