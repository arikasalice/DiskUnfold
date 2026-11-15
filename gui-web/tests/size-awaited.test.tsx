import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AnalysisPanel } from "../src/features/analysis/AnalysisPanel";
import { PinnedResult } from "../src/features/analysis/ResultSummary";
import {
  AWAITED_RESULT_LIMIT,
  lacksReadSizes,
} from "../src/features/analysis/awaited-sizes";
import { awaitedSize } from "../src/features/analysis/space-format";
import type { SizeWait } from "../src/features/analysis/space-format";
import { useSummaryBatch } from "../src/features/analysis/summary-batch";
import { LocaleTextContext, interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import type { AnalysisClient, SizeValue, SpaceTotal } from "../src/ipc";
import {
  liveScan,
  passReflectInterval,
  path,
  resetReflectOffset,
} from "./summary-fixture";

/*
 * A logical or allocated size that has no value while its scan still reads
 * says that the value is to come, not that it is unavailable. The results
 * decide for how long: a value on one of the two sizes, or three results in
 * a row without either, end the wait. The reason lists below are the ones
 * the engine attaches to the first result of a scan whose first batch holds
 * folders only, and to the next result.
 */

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  resetReflectOffset();
});

type Basis = SizeValue["basis"];
const firstLimits: Record<Basis, string[]> = {
  logical: [
    "space.aux_not_included",
    "space.observation_detail_insufficient",
    "space.data_insufficient",
  ],
  allocated: [
    "space.allocated_not_collected",
    "space.aux_not_included",
    "space.observation_detail_insufficient",
    "space.data_insufficient",
  ],
  unique_physical: [
    "space.allocated_not_collected",
    "space.sharing_not_collected",
    "space.aux_not_included",
    "space.observation_detail_insufficient",
    "space.data_insufficient",
  ],
};
const readLimits = [
  "space.aux_not_included",
  "space.observation_detail_insufficient",
];
const unknown = (basis: Basis): SizeValue => ({
  basis,
  evidence: "unknown",
  state: "unknown",
});
const atLeast = (basis: Basis): SizeValue => ({
  basis,
  evidence: "os_reported",
  state: "lower_bound",
  bytes: "64",
});

/** A result for which no file with a size was read: every size is unknown. */
function firstResult(version = "v1", requestSeq = 1): SpaceTotal {
  return {
    resultVersion: version,
    requestSeq,
    completeness: "partial",
    sizes: (["logical", "allocated", "unique_physical"] as const).map(
      (basis) => ({
        size: unknown(basis),
        completeness: "partial",
        measurementLimits: firstLimits[basis],
      }),
    ),
    missingItems: "0",
    inaccessibleRanges: null,
    untraversedLinks: "0",
  };
}

/** A later result: files were read, the unique physical size is still unknown. */
function laterResult(version: string, requestSeq = 1): SpaceTotal {
  const base = firstResult(version, requestSeq);
  return {
    ...base,
    sizes: [
      {
        size: atLeast("logical"),
        completeness: "partial",
        measurementLimits: readLimits,
      },
      {
        size: atLeast("allocated"),
        completeness: "partial",
        measurementLimits: readLimits,
      },
      base.sizes[2]!,
    ],
  };
}

/** A result with a logical size and no allocated one. */
function logicalOnly(version: string): SpaceTotal {
  const base = firstResult(version);
  return {
    ...base,
    sizes: [laterResult(version).sizes[0]!, base.sizes[1]!, base.sizes[2]!],
  };
}

test.each(["logical", "allocated"] as const)(
  "an unknown %s size is awaited while its scan runs or is paused, and not once it ended",
  (basis) => {
    const limits = firstLimits[basis];
    expect(awaitedSize(unknown(basis), limits, "running")).toBe("running");
    expect(awaitedSize(unknown(basis), limits, "paused")).toBe("paused");
    expect(awaitedSize(unknown(basis), limits, undefined)).toBeUndefined();
    expect(awaitedSize(unknown(basis), [], "running")).toBe("running");
    expect(awaitedSize(atLeast(basis), limits, "running")).toBeUndefined();
  },
);

test("a size that cannot arrive is not awaited", () => {
  // The unique physical size can stay unknown for a whole scan.
  expect(
    awaitedSize(
      unknown("unique_physical"),
      firstLimits.unique_physical,
      "running",
    ),
  ).toBeUndefined();
  // These reasons leave the size unknown for good.
  for (const lasting of ["space.dedup_capacity", "space.aggregation_overflow"])
    expect(
      awaitedSize(
        unknown("allocated"),
        [...firstLimits.allocated, lasting],
        "running",
      ),
    ).toBeUndefined();
});

test("a result lacks its read sizes only while neither of the two has a value", () => {
  expect(lacksReadSizes(firstResult())).toBe(true);
  expect(lacksReadSizes(logicalOnly("v1"))).toBe(false);
  expect(lacksReadSizes(laterResult("v1"))).toBe(false);
});

const preferences = {
  display: { system: "iec", unit: "auto" },
  basis: "allocated",
  warning: undefined,
} as const;

function Summary({
  client,
  total,
  wait,
  scan = "scan-1",
}: {
  client: AnalysisClient;
  total: SpaceTotal;
  wait: SizeWait | undefined;
  scan?: string;
}) {
  const summary = useSummaryBatch(client, total, false, wait !== undefined);
  return (
    <PinnedResult
      client={client}
      total={total}
      latestVersion={total.resultVersion}
      volume={summary.volume}
      scan={scan}
      wait={wait}
      roots={[path]}
      exclusions={[]}
      preferences={preferences}
      displayChoice={undefined}
      onDisplayChoice={() => {}}
      onReopenLatest={() => {}}
    />
  );
}

const allocated = () => screen.getByRole("group", { name: "Allocated size" });
const awaited = () => within(allocated()).queryByText("Calculating…") !== null;

test("the lead size reads as still to come by how the scan stands", () => {
  const client = liveScan().client;
  const total = firstResult();
  const { rerender } = render(
    <Summary client={client} total={total} wait="running" />,
  );
  const metric = allocated();
  expect(within(metric).getByText("Calculating…")).toBeDefined();
  expect(within(metric).getByText("Checking file sizes")).toBeDefined();
  expect(within(metric).queryByText("Unavailable")).toBeNull();
  expect(within(metric).queryByText("Partial measurement")).toBeNull();
  // The stand-in keeps the place of a value and is marked for the theme.
  expect(metric.getAttribute("data-pending")).toBe("true");
  expect(
    within(metric)
      .getByText("Calculating…")
      .classList.contains("du-metric__value"),
  ).toBe(true);
  // Values are not announced, and neither is the change to a value.
  expect(metric.closest('[role="status"], [aria-live]')).toBeNull();

  rerender(<Summary client={client} total={total} wait="paused" />);
  expect(allocated()).toBe(metric);
  expect(within(metric).getByText("No value yet")).toBeDefined();
  expect(
    within(metric).getByText("It appears when the scan continues"),
  ).toBeDefined();

  rerender(<Summary client={client} total={total} wait={undefined} />);
  expect(within(metric).getByText("Unavailable")).toBeDefined();
  expect(within(metric).getByText("Partial measurement")).toBeDefined();
  expect(metric.hasAttribute("data-pending")).toBe(false);
});

test("an unknown size with a lasting reason stays unavailable while the scan runs", () => {
  const total = firstResult();
  total.sizes[1]!.measurementLimits = [
    ...firstLimits.allocated,
    "space.dedup_capacity",
  ];
  render(<Summary client={liveScan().client} total={total} wait="running" />);
  expect(within(allocated()).getByText("Unavailable")).toBeDefined();
  expect(awaited()).toBe(false);
  expect(allocated().hasAttribute("data-pending")).toBe(false);
});

test("a size beside a value of the other is unavailable, with its reasons", () => {
  render(
    <Summary
      client={liveScan().client}
      total={logicalOnly("v1")}
      wait="running"
    />,
  );
  expect(within(allocated()).getByText("Unavailable")).toBeDefined();
  expect(within(allocated()).getByText("Partial measurement")).toBeDefined();
  expect(awaited()).toBe(false);
  fireEvent.click(screen.getByRole("button", { name: /Measurement details/ }));
  const rows = within(screen.getByLabelText("Measurement details"));
  const row = within(
    rows.getByText("Allocated size").closest("div") as HTMLElement,
  );
  expect(row.getByText("Unavailable")).toBeDefined();
  expect(row.getByText("Allocation data was not collected")).toBeDefined();
  expect(row.getByText("Some space data could not be verified")).toBeDefined();
});

test("three results in a row without a size end the wait", () => {
  expect(AWAITED_RESULT_LIMIT).toBe(3);
  const client = liveScan().client;
  const show = (version: string) => (
    <Summary client={client} total={firstResult(version)} wait="running" />
  );
  const { rerender } = render(show("v1"));
  expect(awaited()).toBe(true);
  // Showing the same result again does not count it again.
  rerender(show("v1"));
  rerender(show("v1"));
  rerender(show("v2"));
  expect(awaited()).toBe(true);
  rerender(show("v3"));
  expect(awaited()).toBe(false);
  expect(within(allocated()).getByText("Unavailable")).toBeDefined();
  expect(within(allocated()).getByText("Partial measurement")).toBeDefined();
  rerender(show("v4"));
  expect(awaited()).toBe(false);
});

test("a result with a size ends the run of results without one", () => {
  const client = liveScan().client;
  const show = (total: SpaceTotal) => (
    <Summary client={client} total={total} wait="running" />
  );
  const { rerender } = render(show(firstResult("v1")));
  rerender(show(firstResult("v2")));
  rerender(show(laterResult("v3")));
  expect(within(allocated()).getByText("At least 64 bytes")).toBeDefined();
  rerender(show(firstResult("v4")));
  expect(awaited()).toBe(true);
  rerender(show(firstResult("v5")));
  expect(awaited()).toBe(true);
  rerender(show(firstResult("v6")));
  expect(awaited()).toBe(false);
});

test("another scan starts a new run", () => {
  const client = liveScan().client;
  const show = (version: string, scan: string) => (
    <Summary
      client={client}
      total={firstResult(version)}
      wait="running"
      scan={scan}
    />
  );
  const { rerender } = render(show("v1", "scan-1"));
  rerender(show("v2", "scan-1"));
  rerender(show("v3", "scan-1"));
  expect(awaited()).toBe(false);
  rerender(show("w1", "scan-2"));
  expect(awaited()).toBe(true);
  rerender(show("w2", "scan-2"));
  expect(awaited()).toBe(true);
  rerender(show("w3", "scan-2"));
  expect(awaited()).toBe(false);
});

test("measurement details hide the reasons of an awaited size and show them again", () => {
  const client = liveScan().client;
  const { rerender } = render(
    <Summary client={client} total={firstResult()} wait="running" />,
  );
  fireEvent.click(screen.getByRole("button", { name: /Measurement details/ }));
  const rows = within(screen.getByLabelText("Measurement details"));
  const rowOf = (label: string) =>
    rows.getByText(label).closest("div") as HTMLElement;
  const reasons = [
    "Allocation data was not collected",
    "Auxiliary data was not collected",
    "Some observation details are unavailable",
    "Some space data could not be verified",
  ];
  for (const label of ["Logical size", "Allocated size"]) {
    const row = within(rowOf(label));
    expect(row.getByText("Calculating…")).toBeDefined();
    // What the size waits for is said once, below the lead value.
    expect(row.queryByText("Checking file sizes")).toBeNull();
    expect(rowOf(label).querySelector(".du-readout__note")).toBeNull();
    expect(row.queryByText("Partial measurement")).toBeNull();
    for (const reason of reasons) expect(row.queryByText(reason)).toBeNull();
    // The stand-in takes the secondary text colour of the list.
    expect(rowOf(label).classList.contains("du-readout__pending")).toBe(true);
  }
  expect(screen.getAllByText("Checking file sizes")).toHaveLength(1);
  expect(within(allocated()).getByText("Checking file sizes")).toBeDefined();
  // The unique physical size is not awaited and keeps its reasons.
  const unique = within(rowOf("Unique physical size"));
  expect(unique.getByText("Unavailable")).toBeDefined();
  expect(unique.getByText("Partial measurement")).toBeDefined();
  expect(unique.getByText("Sharing evidence was not collected")).toBeDefined();
  expect(
    rowOf("Unique physical size").classList.contains("du-readout__pending"),
  ).toBe(false);

  // While paused the reasons stay hidden.
  rerender(<Summary client={client} total={firstResult()} wait="paused" />);
  expect(
    within(rowOf("Allocated size")).getByText("No value yet"),
  ).toBeDefined();
  expect(rowOf("Allocated size").querySelector(".du-readout__note")).toBeNull();
  expect(
    screen.getAllByText("It appears when the scan continues"),
  ).toHaveLength(1);
  for (const reason of reasons)
    expect(within(rowOf("Allocated size")).queryByText(reason)).toBeNull();

  // "Unavailable" shows the reasons again.
  rerender(<Summary client={client} total={firstResult()} wait={undefined} />);
  const ended = within(rowOf("Allocated size"));
  expect(ended.getByText("Unavailable")).toBeDefined();
  expect(ended.getByText("Partial measurement")).toBeDefined();
  for (const reason of reasons) expect(ended.getByText(reason)).toBeDefined();
  expect(
    rowOf("Allocated size").classList.contains("du-readout__pending"),
  ).toBe(false);

  // A value shows the limits reported with it.
  rerender(
    <Summary client={client} total={laterResult("v2")} wait="running" />,
  );
  const valued = within(rowOf("Allocated size"));
  expect(valued.getByText("At least 64 bytes")).toBeDefined();
  expect(valued.getByText("Auxiliary data was not collected")).toBeDefined();
  expect(valued.queryByText("Allocation data was not collected")).toBeNull();
});

type Entry = [key: string, text: string, parameters: unknown[]];
const bundle = JSON.parse(
  readFileSync(resolve(process.cwd(), "../user-env/i18n/ko.json"), "utf8"),
) as unknown[];
const koreanMessages = new Map(
  (bundle.find(Array.isArray) as Entry[]).map(([key, value]) => [key, value]),
);
const korean: LocaleText = (key, fallback, values) =>
  interpolate(koreanMessages.get(key) ?? fallback, (name) =>
    String(values?.[name] ?? ""),
  );

test("the awaited size reads in Korean from the bundle", () => {
  const client = liveScan().client;
  const { rerender } = render(
    <LocaleTextContext.Provider value={korean}>
      <Summary client={client} total={firstResult()} wait="running" />
    </LocaleTextContext.Provider>,
  );
  const metric = within(screen.getByRole("group", { name: "할당 크기" }));
  expect(metric.getByText("계산 중…")).toBeDefined();
  expect(metric.getByText("파일 크기를 확인하는 중입니다")).toBeDefined();
  rerender(
    <LocaleTextContext.Provider value={korean}>
      <Summary client={client} total={firstResult()} wait="paused" />
    </LocaleTextContext.Provider>,
  );
  expect(metric.getByText("아직 값이 없습니다")).toBeDefined();
  expect(metric.getByText("스캔을 계속하면 나타납니다")).toBeDefined();
  rerender(
    <LocaleTextContext.Provider value={korean}>
      <Summary client={client} total={firstResult()} wait={undefined} />
    </LocaleTextContext.Provider>,
  );
  expect(metric.getByText("확인 불가")).toBeDefined();
});

/** A panel on a scan whose results have no size until `valued` is set. */
async function startScan() {
  const scan = liveScan();
  const sizes = { valued: false };
  const client: AnalysisClient = {
    ...scan.client,
    call: (async (name: string, body: { requestSeq: number }) => {
      const response = await scan.client.call(name as never, body as never);
      if (name !== "space.total") return response;
      const { resultVersion } = response as SpaceTotal;
      return sizes.valued
        ? laterResult(resultVersion, body.requestSeq)
        : firstResult(resultVersion, body.requestSeq);
    }) as AnalysisClient["call"],
  };
  render(<AnalysisPanel client={client} preferences={preferences} />);
  fireEvent.click(await screen.findByRole("radio", { name: path.display }));
  fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start analysis",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
  await screen.findByText("Result version: Interim");
  return { scan, sizes };
}

test("the panel says Calculating while its scan runs, No value yet while paused, and shows the value in place", async () => {
  const { scan, sizes } = await startScan();
  const metric = allocated();
  expect(within(metric).getByText("Calculating…")).toBeDefined();

  scan.changeState("paused");
  expect(await within(metric).findByText("No value yet")).toBeDefined();
  expect(
    within(metric).getByText("It appears when the scan continues"),
  ).toBeDefined();

  scan.changeState("running");
  expect(await within(metric).findByText("Calculating…")).toBeDefined();

  sizes.valued = true;
  passReflectInterval();
  scan.publish("v2");
  expect(await within(metric).findByText("At least 64 bytes")).toBeDefined();
  // The value replaces the stand-in in the same place.
  expect(allocated()).toBe(metric);
  expect(metric.hasAttribute("data-pending")).toBe(false);
  expect(
    within(metric).getByText(
      "Partial measurement · Reported by the file system",
    ),
  ).toBeDefined();
  expect(within(metric).queryByText("Calculating…")).toBeNull();
});

test("the panel says Unavailable from the third result of its scan without a size", async () => {
  const { scan } = await startScan();
  const metric = allocated();
  expect(within(metric).getByText("Calculating…")).toBeDefined();

  passReflectInterval();
  scan.publish("v2");
  await waitFor(() => expect(scan.reads("space.total")).toEqual(["v1", "v2"]));
  // The second result without a size is still awaited.
  await new Promise((resolve) => window.setTimeout(resolve, 50));
  expect(within(metric).getByText("Calculating…")).toBeDefined();

  passReflectInterval();
  scan.publish("v3");
  expect(await within(metric).findByText("Unavailable")).toBeDefined();
  expect(within(metric).getByText("Partial measurement")).toBeDefined();
  expect(scan.reads("space.total")).toEqual(["v1", "v2", "v3"]);
});

test.each(["cancel_requested", "cancelled", "failed"] as const)(
  "the panel says Unavailable once its scan is %s",
  async (state) => {
    const { scan } = await startScan();
    const metric = allocated();
    expect(within(metric).getByText("Calculating…")).toBeDefined();
    scan.changeState(state);
    expect(await within(metric).findByText("Unavailable")).toBeDefined();
    expect(within(metric).getByText("Partial measurement")).toBeDefined();
    expect(metric.hasAttribute("data-pending")).toBe(false);
  },
);
