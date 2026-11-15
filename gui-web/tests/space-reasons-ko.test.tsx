import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { FileInspection } from "../src/features/analysis/FileInspection";
import { PinnedResult } from "../src/features/analysis/ResultSummary";
import {
  displayMeasurementLimit,
  displaySpaceReason,
} from "../src/features/analysis/space-format";
import { useSummaryBatch } from "../src/features/analysis/summary-batch";
import { LocaleTextContext, interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import type { AnalysisClient, BrowseDetail, SpaceTotal } from "../src/ipc";
import { folderDetail, liveScan, path } from "./summary-fixture";

/*
 * Space reasons are shown through their message keys, so a Korean screen
 * reads them in Korean. The texts come from the bundles the product ships.
 */

afterEach(cleanup);

type Entry = [key: string, text: string, parameters: unknown[]];

function bundle(language: "en" | "ko") {
  const data = JSON.parse(
    readFileSync(
      resolve(process.cwd(), "../user-env/i18n/" + language + ".json"),
      "utf8",
    ),
  ) as unknown[];
  return new Map(
    (data.find(Array.isArray) as Entry[]).map(([key, value]) => [key, value]),
  );
}

const koreanMessages = bundle("ko");
const englishMessages = bundle("en");
const korean: LocaleText = (key, fallback, values) =>
  interpolate(koreanMessages.get(key) ?? fallback, (name) =>
    String(values?.[name] ?? ""),
  );
/** What a screen shows when no bundle is loaded. */
const fallbackOnly: LocaleText = (_key, fallback) => fallback;

const itemReasons = [
  ["space.aux_not_included", "ui.analysis.space_aux_not_included"],
  ["space.dedup_capacity", "ui.analysis.space_dedup_capacity"],
  ["space.sharing_not_collected", "ui.analysis.space_sharing_not_collected"],
  ["space.sharing_unknown", "ui.analysis.space_sharing_unknown"],
  ["space.identity_unsupported", "ui.analysis.space_identity_unsupported"],
  ["space.out_of_range_links", "ui.analysis.space_out_of_range_links"],
  ["space.shared_reported", "ui.analysis.space_shared_reported"],
] as const;
const limitReasons = [
  ["space.data_insufficient", "ui.analysis.space_data_insufficient"],
  ["space.aggregation_overflow", "ui.analysis.space_aggregation_overflow"],
  [
    "space.allocated_not_collected",
    "ui.analysis.space_allocated_not_collected",
  ],
  [
    "space.observation_detail_insufficient",
    "ui.analysis.space_observation_detail_insufficient",
  ],
] as const;
const unrecognizedLimit = "ui.analysis.space_measurement_limit_reported";

test.each([...itemReasons, ...limitReasons])(
  "the measurement limit %s reads from its message key in Korean",
  (reason, key) => {
    const expected = koreanMessages.get(key);
    expect(expected).toMatch(/[가-힣]/);
    expect(displayMeasurementLimit(reason, korean)).toBe(expected);
    // The fallback is the English bundle text, so both languages have the key.
    expect(displayMeasurementLimit(reason, fallbackOnly)).toBe(
      englishMessages.get(key),
    );
  },
);

test.each(itemReasons)(
  "the item-space reason %s reads from its message key in Korean",
  (reason, key) => {
    expect(displaySpaceReason(reason, korean)).toBe(koreanMessages.get(key));
    expect(displaySpaceReason(reason, fallbackOnly)).toBe(
      englishMessages.get(key),
    );
  },
);

test("an unrecognized reason key falls back the same way in every language", () => {
  // An item-space reason gives no text; the caller shows its own wording.
  expect(displaySpaceReason("space.future", korean)).toBeUndefined();
  expect(displaySpaceReason("space.data_insufficient", korean)).toBeUndefined();
  // A measurement limit still says that a limitation was reported.
  expect(displayMeasurementLimit("space.future", korean)).toBe(
    koreanMessages.get(unrecognizedLimit),
  );
  expect(displayMeasurementLimit("space.future", fallbackOnly)).toBe(
    "A measurement limitation was reported",
  );
  expect(englishMessages.get(unrecognizedLimit)).toBe(
    "A measurement limitation was reported",
  );
});

const total: SpaceTotal = {
  resultVersion: "v1",
  requestSeq: 1,
  completeness: "missing",
  sizes: [
    {
      size: {
        basis: "allocated",
        evidence: "os_reported",
        state: "lower_bound",
        bytes: "4096",
      },
      completeness: "missing",
      measurementLimits: [
        "space.data_insufficient",
        "space.sharing_unknown",
        "space.future",
      ],
    },
  ],
  missingItems: "0",
  inaccessibleRanges: "1",
  untraversedLinks: "0",
};

function Summary({ client }: { client: AnalysisClient }) {
  const summary = useSummaryBatch(client, total, false, false);
  return (
    <PinnedResult
      client={client}
      total={total}
      latestVersion={total.resultVersion}
      volume={summary.volume}
      scan={undefined}
      wait={undefined}
      roots={[path]}
      preferences={{
        display: { system: "iec", unit: "auto" },
        basis: "allocated",
        warning: undefined,
      }}
      displayChoice={undefined}
      onDisplayChoice={() => {}}
      onReopenLatest={() => {}}
    />
  );
}

test("the summary's measurement details name each limit in Korean", () => {
  render(
    <LocaleTextContext.Provider value={korean}>
      <Summary client={liveScan().client} />
    </LocaleTextContext.Provider>,
  );
  fireEvent.click(screen.getByRole("button", { name: /측정 상세/ }));
  const details = within(screen.getByLabelText("측정 상세"));
  for (const key of [
    "ui.analysis.space_data_insufficient",
    "ui.analysis.space_sharing_unknown",
    unrecognizedLimit,
  ])
    expect(details.getByText(koreanMessages.get(key)!)).toBeDefined();
  for (const key of [
    "ui.analysis.space_data_insufficient",
    "ui.analysis.space_sharing_unknown",
    unrecognizedLimit,
  ])
    expect(details.queryByText(englishMessages.get(key)!)).toBeNull();
});

test("item details name a reason and the evidence of a size in Korean", async () => {
  const scan = liveScan();
  const detail: BrowseDetail = {
    ...folderDetail("v1"),
    kind: { state: "unknown", reasonKey: "space.identity_unsupported" },
    allocatedSize: { state: "unknown", reasonKey: "space.future" },
  };
  const client: AnalysisClient = {
    ...scan.client,
    call: ((name: string, body: never) =>
      name === "browse.detail"
        ? Promise.resolve(detail)
        : scan.client.call(name as never, body)) as AnalysisClient["call"],
  };
  render(
    <LocaleTextContext.Provider value={korean}>
      <FileInspection
        client={client}
        item={{ resultVersion: "v1", itemNo: 0 }}
        itemKind="folder"
      />
    </LocaleTextContext.Provider>,
  );
  expect(
    await screen.findByText(
      koreanMessages.get("ui.analysis.space_identity_unsupported")!,
    ),
  ).toBeDefined();
  // The evidence of a present size was English before this was localized.
  expect(
    screen.getByText(koreanMessages.get("ui.analysis.evidence_reported")!),
  ).toBeDefined();
  expect(screen.queryByText("Reported by the file system")).toBeNull();
  // An unrecognized reason shows the general wording of an unknown value.
  expect(
    screen.getByText(koreanMessages.get("ui.inspection.unavailable_in_scan")!),
  ).toBeDefined();
});
