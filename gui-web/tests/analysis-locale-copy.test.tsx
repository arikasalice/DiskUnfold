import type { ReactNode } from "react";
import { afterEach, expect, test, vi } from "vitest";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
} from "@testing-library/react";
import { LocaleTextContext, interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import { CopyPath } from "../src/features/analysis/CopyPath";
import { ItemSpaceDetails } from "../src/features/analysis/ItemSpaceDetails";
import { PinnedResult } from "../src/features/analysis/ResultSummary";
import { formatDate } from "../src/features/analysis/RecentScopeDisplay";
import { RecentScopePicker } from "../src/features/analysis/RecentScopePicker";
import { AnalysisError, createAnalysisClient, unixPath } from "../src/ipc";
import type {
  CallEnvelope,
  ItemRef,
  ResultEnvelope,
  SpaceDetail,
  SpaceDetailSize,
  SpaceTotal,
} from "../src/ipc";

afterEach(cleanup);

const regional = {
  decimalSeparator: ",",
  groupingSeparator: ".",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "iso8601",
  groupNumbers: true,
  timeZonePolicy: "utc",
  localTimeZoneStatus: "identified",
} as const;

const translations: Record<string, string> = {
  "ui.analysis.recent_ranges": "최근 분석 범위",
  "ui.browse.copy_path": "경로 복사",
  "ui.analysis.copying_path": "경로 복사 중…",
  "ui.analysis.copied_paths": "{count}개 경로를 복사했습니다.",
  "ui.analysis.dismiss_copy_result": "복사 결과 닫기",
  "ui.analysis.full_path": "전체 경로",
  "ui.analysis.space_measurement_details": "공간 측정 세부 정보",
  "ui.analysis.observed_item_space": "관찰한 항목 공간",
  "ui.analysis.space_observed_logical": "관찰된 논리 크기",
  "ui.analysis.space_shared": "공유 공간",
  "ui.analysis.shared_space_counted_per_file": "파일마다 셈",
  "ui.analysis.allocated_may_include_shared":
    "복제된 파일이 같은 공간을 나눠 써서 실제 디스크 사용량보다 크게 보일 수 있습니다.",
  "ui.analysis.evidence_measured": "측정됨",
};

const localizedText: LocaleText = (key, fallback, values) =>
  interpolate(translations[key] ?? fallback, (name) =>
    String(values?.[name] ?? "Unknown"),
  );

function localeTree(children: ReactNode, text: LocaleText) {
  return (
    <LocaleTextContext.Provider value={text}>
      <RegionalFormatContext.Provider value={regional}>
        {children}
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}

const englishText: LocaleText = (key, fallback, values) =>
  interpolate(fallback || key, (name) => String(values?.[name] ?? "Unknown"));

function renderLocalized(children: ReactNode) {
  return render(localeTree(children, localizedText));
}

function response(
  name: string,
  correlationId: string,
  body: unknown,
): ResultEnvelope {
  return { name, correlationId, contractVersion: 1, body };
}

test("recent range opener retains a localized accessible name", async () => {
  const client = createAnalysisClient(async (_command, { envelope }) =>
    response(envelope.name, envelope.correlationId, { rows: [] }),
  );
  renderLocalized(
    <RecentScopePicker
      client={client}
      hasDraftEdits={false}
      nextRequestSeq={() => 1}
      isRequestSeqCurrent={() => true}
      onApply={() => {}}
      onChoosePath={() => {}}
    />,
  );

  fireEvent.click(screen.getByRole("button", { name: "최근 분석 범위" }));
  expect(
    await screen.findByRole("heading", { name: "최근 분석 범위" }),
  ).toBeTruthy();
});

test("recent range dates use the selected date pattern and time zone", () => {
  expect(formatDate("2026-11-10T00:00:00.000Z", regional)).toBe(
    "2026.11.10 00:00:00Z",
  );
  expect(formatDate("invalid date", regional)).toBe("invalid date");
});

test("copy path button and live result use the selected language", async () => {
  let finish!: (result: ResultEnvelope) => void;
  let pendingEnvelope!: CallEnvelope;
  const client = createAnalysisClient(
    (_command, { envelope }) =>
      new Promise<ResultEnvelope>((resolve) => {
        pendingEnvelope = envelope;
        finish = resolve;
      }),
  );
  const item = { resultVersion: "result-a", itemNo: 1 };
  const copyPath = (
    <CopyPath
      client={client}
      item={item}
      path={{ state: "present", value: "C:\\fixture" }}
    />
  );
  const view = render(localeTree(copyPath, localizedText));

  fireEvent.click(screen.getByRole("button", { name: "경로 복사" }));
  expect(screen.getByRole("status").textContent).toBe("경로 복사 중…");
  view.rerender(localeTree(copyPath, englishText));
  expect(screen.getByRole("status").textContent).toBe("Copying path…");
  await act(async () =>
    finish(
      response(pendingEnvelope.name, pendingEnvelope.correlationId, {
        outcome: "copied",
        count: 1,
      }),
    ),
  );
  expect(screen.getByRole("status").textContent).toBe("Copied 1 path.");
  view.rerender(localeTree(copyPath, localizedText));
  expect((await screen.findByRole("status")).textContent).toBe(
    "1개 경로를 복사했습니다.",
  );
  expect(screen.getByRole("button", { name: "전체 경로" })).toBeTruthy();
});

test("item space details localize labels and show no capability codes", async () => {
  const item: ItemRef = { resultVersion: "result-a", itemNo: 1 };
  const size: SpaceDetailSize = {
    size: {
      basis: "logical",
      evidence: "measured",
      state: "exact",
      bytes: "1024",
    },
    measurementLimits: [],
  };
  const detail: SpaceDetail = {
    item,
    logical: { state: "present", value: size },
    allocated: { state: "absent" },
    uniquePhysical: { state: "absent" },
    exclusive: { state: "absent" },
    shared: { state: "absent" },
    linkGroup: { state: "absent" },
    auxiliary: {
      state: "present",
      value: {
        size: {
          basis: "allocated",
          evidence: "unknown",
          state: "unknown",
        },
        includedInAllocated: null,
      },
    },
    exclusiveDecision: { confirmed: true, reasonKey: null },
    observedAt: { state: "present", value: "2026-11-10T00:00:00.000Z" },
    capabilities: {
      state: "present",
      value: [{ key: "sparse_files", state: "supported", reasonKey: null }],
    },
  };
  const client = createAnalysisClient(async (_command, { envelope }) =>
    response(envelope.name, envelope.correlationId, detail),
  );
  renderLocalized(
    <ItemSpaceDetails
      client={client}
      item={item}
      display={{ system: "iec", unit: "auto", regional }}
    />,
  );

  fireEvent.click(
    await screen.findByRole("button", { name: "공간 측정 세부 정보" }),
  );
  expect(screen.getByLabelText("관찰한 항목 공간")).toBeTruthy();
  expect(screen.getByText("관찰된 논리 크기")).toBeTruthy();
  expect(screen.getByText("1 KiB · 1.024 bytes · 측정됨")).toBeTruthy();
  expect(screen.getByText("공유 공간").nextElementSibling?.textContent).toBe(
    "Not applicable",
  );
  expect(
    screen.queryByText(
      "복제된 파일이 같은 공간을 나눠 써서 실제 디스크 사용량보다 크게 보일 수 있습니다.",
    ),
  ).toBeNull();
  // No capability has a name yet: it reads as unknown, without its code.
  expect(screen.getByText("Unknown")).toBeTruthy();
  expect(screen.queryByText(/sparse_files|supported/)).toBeNull();
  expect(screen.getByText("2026.11.10 00:00:00Z")).toBeTruthy();
});

function summaryTotal(sharedLimit: boolean): SpaceTotal {
  return {
    resultVersion: "result-a",
    requestSeq: 1,
    completeness: "missing",
    sizes: [
      {
        size: {
          basis: "logical",
          evidence: "os_reported",
          state: "lower_bound",
          bytes: "1100",
        },
        completeness: "missing",
        measurementLimits: [],
      },
      {
        size: {
          basis: "allocated",
          evidence: "os_reported",
          state: "lower_bound",
          bytes: "2200",
        },
        completeness: "missing",
        measurementLimits: sharedLimit
          ? ["space.allocated_may_include_shared"]
          : [],
      },
    ],
    missingItems: "0",
    inaccessibleRanges: "0",
    untraversedLinks: "0",
  };
}

function renderSummary(total: SpaceTotal, basis: "allocated" | "logical") {
  const client = createAnalysisClient(async () => {
    throw new Error("Unexpected request in the pinned result summary");
  });
  return render(
    localeTree(
      <PinnedResult
        client={client}
        total={total}
        latestVersion={total.resultVersion}
        volume={{
          target: undefined,
          view: { state: "closed" },
          open: () => {},
          close: () => {},
          retry: () => {},
        }}
        scan={undefined}
        wait={undefined}
        roots={[unixPath("/")]}
        exclusions={[]}
        preferences={{
          display: { system: "iec", unit: 0, regional },
          basis,
          warning: undefined,
        }}
        displayChoice={undefined}
        onDisplayChoice={() => {}}
        onReopenLatest={() => {}}
      />,
      englishText,
    ),
  );
}

const sharedSpaceSentence =
  "Cloned files share space, so this can be larger than the actual disk usage.";

test("F03 #351 shows the allocated shared-space limit in the summary and measurement details", async () => {
  renderSummary(summaryTotal(true), "allocated");
  const metric = screen.getByRole("group", { name: "Allocated size" });
  expect(screen.getByText("At least 2.200 bytes")).toBeTruthy();
  const notice = screen.getByText(sharedSpaceSentence).closest(".du-notice");
  expect(notice?.classList.contains("du-notice--info")).toBe(true);
  fireEvent.click(screen.getByRole("button", { name: "Measurement details" }));
  const measurements = screen.getByLabelText("Measurement details");
  expect(measurements.querySelector("dt")?.textContent).toBe("Logical size");
  const sharedLabel = screen.getByText("Shared space");
  expect(sharedLabel.nextElementSibling?.textContent).toContain(
    "Counted per file",
  );
  expect(sharedLabel.nextElementSibling?.textContent).toContain(
    sharedSpaceSentence,
  );
  expect(metric).toBeTruthy();
});

test("F03 #351 does not promote the allocated limit to logical size or show it when absent", async () => {
  const logical = renderSummary(summaryTotal(true), "logical");
  expect(screen.getByText("At least 1.100 bytes")).toBeTruthy();
  expect(screen.queryByText(sharedSpaceSentence)).toBeNull();
  logical.unmount();

  renderSummary(summaryTotal(false), "allocated");
  expect(screen.queryByText(sharedSpaceSentence)).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Measurement details" }));
  expect(screen.queryByText("Shared space")).toBeNull();
});

test("F03 #351 space.detail uses Counted per file only when allocated carries the limit", async () => {
  const item: ItemRef = { resultVersion: "result-a", itemNo: 2 };
  const allocated: SpaceDetailSize = {
    size: {
      basis: "allocated",
      evidence: "os_reported",
      state: "exact",
      bytes: "1200",
    },
    measurementLimits: ["space.allocated_may_include_shared"],
  };
  const detail: SpaceDetail = {
    item,
    logical: { state: "absent" },
    allocated: { state: "present", value: allocated },
    uniquePhysical: { state: "absent" },
    exclusive: { state: "absent" },
    shared: { state: "absent" },
    linkGroup: { state: "absent" },
    auxiliary: { state: "absent" },
    exclusiveDecision: { confirmed: false, reasonKey: null },
    observedAt: { state: "absent" },
    capabilities: { state: "absent" },
  };
  const client = createAnalysisClient(async (_command, { envelope }) =>
    response(envelope.name, envelope.correlationId, detail),
  );
  renderLocalized(
    <ItemSpaceDetails
      client={client}
      item={item}
      display={{ system: "iec", unit: "auto", regional }}
    />,
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "공간 측정 세부 정보" }),
  );
  const shared = screen.getByText("공유 공간").nextElementSibling;
  expect(shared?.textContent).toContain("파일마다 셈");
  expect(shared?.textContent).toContain(
    "복제된 파일이 같은 공간을 나눠 써서 실제 디스크 사용량보다 크게 보일 수 있습니다.",
  );
});

test("item space problems use the active locale without reloading details", async () => {
  const item: ItemRef = { resultVersion: "result-a", itemNo: 1 };
  const transport = vi.fn(async () => {
    throw new AnalysisError("STALE_VERSION", "result.requery_required");
  });
  const client = createAnalysisClient(transport);
  const details = (
    <ItemSpaceDetails
      client={client}
      item={item}
      display={{ system: "iec", unit: "auto", regional }}
    />
  );
  const view = render(localeTree(details, englishText));

  expect(
    await screen.findByText("Open the latest result to see its space details."),
  ).toBeTruthy();
  view.rerender(
    localeTree(details, (key, fallback) =>
      key === "ui.analysis.space_open_latest"
        ? "Abre el resultado más reciente para ver sus detalles de espacio."
        : fallback,
    ),
  );
  expect(
    screen.getByText(
      "Abre el resultado más reciente para ver sus detalles de espacio.",
    ),
  ).toBeTruthy();
  expect(transport).toHaveBeenCalledTimes(1);
});
