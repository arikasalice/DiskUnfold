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
import type { BrowseOpenResponse, FolderQueryRow } from "../src/ipc";
import { useScanPriority } from "../src/state/scan-priority";
import { priorityClient, ref, running, version } from "./scan-priority-fixture";

afterEach(cleanup);

const unknown = { state: "unknown", reasonKey: "ref.unavailable" } as const;

function folder(itemNo: number, name: string): FolderQueryRow {
  return {
    item: ref(itemNo),
    displayName: { state: "present", value: name },
    kind: "directory",
    logicalSize: unknown,
    allocatedSize: { state: "absent" },
    modifiedAt: { state: "absent" },
    hasChildFolder: unknown,
    flags: { unenumerated: { state: "absent" }, excluded: { state: "absent" } },
    correspondence: unknown,
  };
}

const rows = [folder(1, "보고서 "), folder(2, "보고서")];

function port(): BrowsePort {
  const response: BrowseOpenResponse = {
    kind: "opened",
    requestSeq: 1,
    origin: "scan",
    location: { resultVersion: version, itemNo: 0 },
    breadcrumbs: [
      { item: ref(0), displayName: { state: "present", value: "자료 " } },
    ],
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
    copyPath: vi.fn(() =>
      Promise.resolve({ outcome: "copied", count: 1 } as const),
    ),
  };
}

async function setup() {
  const { client, answers } = priorityClient();
  const browsePort = port();
  function Harness() {
    const priority = useScanPriority(client, running);
    return (
      <BrowsePanel
        port={browsePort}
        resultVersion={version}
        priority={priority}
        inspectItem={() => "Details"}
      />
    );
  }
  render(<Harness />);
  const grid = await screen.findByRole("grid");
  await waitFor(() => expect(within(grid).getAllByRole("row").length).toBe(3));
  return { answers, browsePort };
}

function rowOf(name: RegExp) {
  return screen
    .getByRole("button", { name })
    .closest<HTMLElement>('[role="row"]')!;
}

test("F02: two folders that differ only by a trailing space read differently in the folder list", async () => {
  await setup();
  const spaced = rowOf(/^보고서\s*, The name ends with 1 space$/);
  const plain = rowOf(/^보고서$/);
  expect(spaced.querySelectorAll(".du-name__mark")).toHaveLength(1);
  expect(plain.querySelectorAll(".du-name__mark")).toHaveLength(0);
});

test("F02: the path line and the location hierarchy mark the current folder name", async () => {
  await setup();
  const current = document.querySelector('[aria-current="location"]')!;
  expect(current.getAttribute("title")).toBe("자료 ");
  expect(current.querySelectorAll(".du-name__mark")).toHaveLength(1);
  fireEvent.click(screen.getByRole("button", { name: /Observation details/ }));
  const hierarchy = screen
    .getByText("Location hierarchy")
    .closest("div")!
    .querySelector("dd")!;
  expect(hierarchy.querySelectorAll(".du-name__mark")).toHaveLength(1);
});

test("F02: the Analyze first notice writes the marks into its text and sends the item itself", async () => {
  const { answers } = await setup();
  const row = rowOf(/^보고서\s*, The name ends/);
  fireEvent.click(within(row).getByRole("button", { name: "Analyze first" }));
  expect(answers[0]?.request).toEqual({ jobRef: "scan", priorityArea: ref(1) });
  await act(async () =>
    answers[0]?.resolve({
      controlResult: "accepted",
      appliedPriorityArea: ref(1),
    }),
  );
  expect(screen.getAllByText("‘보고서\u2423’ is analyzed first")).toHaveLength(
    1,
  );
});

test("F02: the copy feedback draws the name with its marks and copies the item itself", async () => {
  const { browsePort } = await setup();
  fireEvent.contextMenu(rowOf(/^보고서\s*, The name ends/));
  // An accessible name drops the trailing space; the menu belongs to the spaced row.
  const menu = await screen.findByRole("menu", { name: "Actions for 보고서" });
  fireEvent.click(within(menu).getByRole("menuitem", { name: "Copy path" }));
  await waitFor(() => expect(browsePort.copyPath).toHaveBeenCalledWith(ref(1)));
  const name = await waitFor(() => {
    const element = document.querySelector(".du-feedback-name");
    expect(element).not.toBeNull();
    return element!;
  });
  await waitFor(() =>
    expect(name.querySelectorAll(".du-name__mark")).toHaveLength(1),
  );
  expect(name.textContent).toContain("보고서 ");
});
