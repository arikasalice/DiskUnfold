import { cleanup, render, screen, within } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { Moment } from "../src/components";
import { BrowsePanel } from "../src/features/browse/BrowsePanel";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import type { RegionalFormat } from "../src/i18n/regional-format";
import type { BrowseOpenResponse } from "../src/ipc";

/*
 * Design review of PR #60 (follow-up 2): a size with its unit ("At least
 * 390.6 KiB") and a date with its time stay whole. In the folder list a line
 * may break only between the date and time and the time zone.
 */

afterEach(cleanup);

const id = "018f0000-0000-7000-8000-0000000000e1";
const regional: RegionalFormat = {
  decimalSeparator: ".",
  groupingSeparator: ",",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "lang_default",
  groupNumbers: true,
  timeZonePolicy: "utc",
  localTimeZoneStatus: "identified",
};

const opened: BrowseOpenResponse = {
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
    rows: [
      {
        item: { resultVersion: id, itemNo: 1 },
        displayName: { state: "present", value: "Photos" },
        kind: "directory",
        logicalSize: {
          state: "present",
          value: {
            basis: "logical",
            evidence: "os_reported",
            state: "lower_bound",
            bytes: "400000",
          },
        },
        allocatedSize: { state: "absent" },
        modifiedAt: { state: "present", value: "2026-11-14T22:45:57Z" },
        hasChildFolder: { state: "absent" },
        flags: {
          unenumerated: { state: "absent" },
          excluded: { state: "absent" },
        },
        correspondence: { state: "absent" },
      },
    ],
    total: { state: "exact", count: 1 },
    unknownCount: 0,
    observedAt: null,
    completeness: "complete",
    nextCursor: null,
  },
  sortStrategies: [],
  predicateStrategies: [],
};

test("the folder list keeps sizes whole and breaks a time only before its zone", async () => {
  const open = vi
    .fn()
    .mockImplementation((request) =>
      Promise.resolve({ ...opened, requestSeq: request.requestSeq }),
    );
  render(
    <RegionalFormatContext.Provider value={regional}>
      <BrowsePanel port={{ open, page: vi.fn() }} resultVersion={id} />
    </RegionalFormatContext.Provider>,
  );
  await screen.findByText("Photos");
  const row = screen.getByText("Photos").closest<HTMLElement>('[role="row"]')!;
  const pieces = [...row.querySelectorAll(".du-unbroken")].map(
    (piece) => piece.textContent,
  );
  expect(pieces).toEqual([
    "At least 390.6 KiB",
    "Not recorded",
    "2026.11.14 22:45:57",
    "UTC",
  ]);
  const time = within(row).getByText("2026.11.14 22:45:57").parentElement!;
  expect(time.textContent).toBe("2026.11.14 22:45:57 UTC");
});

test("an ISO 8601 time keeps its zone in the same piece", () => {
  const { container } = render(<Moment moment="2026-11-15T07:45:57+09:00" />);
  expect(
    [...container.querySelectorAll(".du-unbroken")].map((p) => p.textContent),
  ).toEqual(["2026-11-15T07:45:57+09:00"]);
});

test("a note follows the pieces as prose", () => {
  const { container } = render(
    <Moment
      moment="2026.11.15 07:45:57"
      zone="UTC"
      note="Local time zone unavailable"
    />,
  );
  expect(container.textContent).toBe(
    "2026.11.15 07:45:57 UTC · Local time zone unavailable",
  );
  expect(container.querySelectorAll(".du-unbroken")).toHaveLength(2);
});
