import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { FileInspection } from "../src/features/analysis/FileInspection";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import type { BrowseDetail, BrowseOpenResponse } from "../src/ipc";
import { sampleClient } from "./sample-support";

afterEach(cleanup);
const id = "018f0000-0000-7000-8000-000000000001";
const REASON = "Cannot run on sample data";

const fileRow = {
  item: { resultVersion: id, itemNo: 1 },
  displayName: { state: "present", value: "clip.mp4" },
  kind: "file",
  logicalSize: { state: "unknown", reasonKey: "ref.unavailable" },
  allocatedSize: { state: "absent" },
  modifiedAt: { state: "absent" },
  hasChildFolder: { state: "unknown", reasonKey: "ref.unavailable" },
  flags: {
    unenumerated: { state: "unknown", reasonKey: "ref.unavailable" },
    excluded: { state: "absent" },
  },
  correspondence: { state: "unknown", reasonKey: "ref.unavailable" },
} as const;

const openedPage = {
  kind: "opened",
  requestSeq: 1,
  origin: "scan",
  location: null,
  breadcrumbs: [],
  rootKind: "virtual_root",
  currentCorrespondence: false,
  page: {
    rowKind: "folder",
    queryRef: id,
    resultVersion: id,
    rows: [fileRow],
    total: { state: "exact", count: 1 },
    unknownCount: 0,
    observedAt: null,
    completeness: "complete",
    nextCursor: null,
  },
  sortStrategies: [],
  predicateStrategies: [],
} as unknown as BrowseOpenResponse;

async function rowMenu(reason: string | undefined) {
  const copyPath = vi
    .fn<NonNullable<BrowsePort["copyPath"]>>()
    .mockResolvedValue({ outcome: "copied", count: 1 });
  const port: BrowsePort = {
    open: vi.fn().mockResolvedValue(openedPage),
    page: vi.fn(),
    copyPath,
  };
  render(
    <BrowsePanel
      port={port}
      resultVersion={id}
      {...(reason ? { actionsDisabledReason: reason } : {})}
    />,
  );
  const trigger = await screen.findByRole("button", {
    name: "Actions for clip.mp4",
  });
  fireEvent.contextMenu(trigger.closest<HTMLElement>('[role="row"]')!);
  const menu = await screen.findByRole("menu", {
    name: "Actions for clip.mp4",
  });
  return { menu, copyPath };
}

test("in the sample, copying a path is off with the sample reason and never calls the host", async () => {
  const { menu, copyPath } = await rowMenu(REASON);
  const item = within(menu).getByRole("menuitem", { name: /Copy path/ });
  expect(item.getAttribute("aria-disabled")).toBe("true");
  expect(item.textContent).toContain(REASON);
  fireEvent.click(item);
  expect(copyPath).not.toHaveBeenCalled();
});

test("a real result keeps copying a path available", async () => {
  const { menu, copyPath } = await rowMenu(undefined);
  const item = within(menu).getByRole("menuitem", { name: "Copy path" });
  expect(item.getAttribute("aria-disabled")).not.toBe("true");
  fireEvent.click(item);
  expect(copyPath).toHaveBeenCalledTimes(1);
  expect((await screen.findByRole("status")).textContent).toBe(
    "Copied 1 path for clip.mp4.",
  );
});

function detail(): BrowseDetail {
  return {
    item: { resultVersion: id, itemNo: 1 },
    dataSource: "sample",
    displayPath: { state: "present", value: "Sample data\\clip.mp4" },
    displayName: { state: "present", value: "clip.mp4" },
    kind: { state: "present", value: "file" },
    logicalSize: { state: "unknown", reasonKey: "ref.unavailable" },
    allocatedSize: { state: "unknown", reasonKey: "ref.unavailable" },
    createdAt: { state: "absent" },
    modifiedAt: { state: "absent" },
    observedAt: { state: "absent" },
    origin: "scan",
    completeness: "complete",
    currentCorrespondence: false,
    observationOnly: true,
  };
}

test("the inspection of a sample item disables Copy path beside the sample reason", async () => {
  const fake = sampleClient({ "browse.detail": () => detail() });
  render(
    <FileInspection
      client={fake.client}
      item={{ resultVersion: id, itemNo: 1 }}
      actionsDisabledReason={REASON}
    />,
  );
  const copy = await screen.findByRole("button", { name: "Copy path" });
  expect((copy as HTMLButtonElement).disabled).toBe(true);
  expect(screen.getByText(REASON)).toBeTruthy();
  fireEvent.click(copy);
  expect(fake.count("browse.copy_path")).toBe(0);
});
