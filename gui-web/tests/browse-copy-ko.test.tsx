import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { displayMeasurement } from "../src/features/analysis/space-format";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import type { BrowsePort } from "../src/features/browse/BrowsePanel";
import { LocaleTextContext, interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import type { BrowseOpenResponse, FolderQueryRow } from "../src/ipc";

/*
 * Copy the design review asked for, read from the Korean bundle the product
 * ships, so a later wording change there is caught here.
 */

afterEach(cleanup);

type Entry = [key: string, text: string, parameters: unknown[]];

const bundle = JSON.parse(
  readFileSync(resolve(process.cwd(), "../user-env/i18n/ko.json"), "utf8"),
) as unknown[];
const messages = new Map(
  (bundle.find(Array.isArray) as Entry[]).map(([key, value]) => [key, value]),
);
const korean: LocaleText = (key, fallback, values) =>
  interpolate(messages.get(key) ?? fallback, (name) =>
    String(values?.[name] ?? ""),
  );

const version = "018f0000-0000-7000-8000-0000000000b1";
const unknown = { state: "unknown", reasonKey: "ref.unavailable" } as const;
const folder: FolderQueryRow = {
  item: { resultVersion: version, itemNo: 1 },
  displayName: { state: "present", value: "Photos" },
  kind: "directory",
  logicalSize: unknown,
  allocatedSize: { state: "absent" },
  modifiedAt: { state: "absent" },
  hasChildFolder: unknown,
  flags: { unenumerated: unknown, excluded: { state: "absent" } },
  correspondence: unknown,
};

function opened(origin: string): BrowseOpenResponse {
  return {
    kind: "opened",
    requestSeq: 1,
    origin,
    location: { resultVersion: version, itemNo: 0 },
    breadcrumbs: [],
    rootKind: "scope_root",
    currentCorrespondence: false,
    page: {
      rowKind: "folder",
      queryRef: version,
      resultVersion: version,
      rows: [folder],
      total: { state: "lower_bound", count: 1 },
      unknownCount: 0,
      observedAt: "2026-11-14T10:00:00Z",
      completeness: "partial",
      nextCursor: null,
    },
    sortStrategies: [],
    predicateStrategies: [],
  };
}

async function browse(origin: string) {
  const port: BrowsePort = {
    open: vi.fn((request) =>
      Promise.resolve({ ...opened(origin), requestSeq: request.requestSeq }),
    ),
    page: vi.fn(),
  };
  render(
    <LocaleTextContext.Provider value={korean}>
      <BrowsePanel port={port} resultVersion={version} />
    </LocaleTextContext.Provider>,
  );
  await screen.findByText("Photos");
}

test("the browse header names the scan origin and a lower-bound count in Korean", async () => {
  await browse("scan");
  expect(
    screen.getByText("현재 분석 · 2026-11-14 · 항목 최소 1개"),
  ).toBeDefined();
});

test("an origin without copy is shown as reported", async () => {
  await browse("imported");
  expect(screen.getByText(/^imported · /)).toBeDefined();
});

test("an availability that could not be read says only that it is unknown", async () => {
  await browse("scan");
  const row = screen.getByText("Photos").closest<HTMLElement>('[role="row"]')!;
  expect(within(row).getByText("알 수 없음")).toBeDefined();
});

test("a partial measurement reported by the file system reads in Korean", () => {
  expect(
    displayMeasurement(
      {
        basis: "logical",
        evidence: "os_reported",
        state: "lower_bound",
        bytes: "42",
      },
      "partial",
      korean,
    ),
  ).toBe("일부만 측정됨 · 파일 시스템이 알려 준 값");
});
