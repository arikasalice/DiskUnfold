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
import { formatDate } from "../src/features/analysis/RecentScopeDisplay";
import { RecentScopePicker } from "../src/features/analysis/RecentScopePicker";
import { AnalysisError, createAnalysisClient } from "../src/ipc";
import type {
  CallEnvelope,
  ItemRef,
  ResultEnvelope,
  SpaceDetail,
  SpaceDetailSize,
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

test("item space details localize labels and keep capability codes intact", async () => {
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
  expect(screen.getByText("sparse_files · supported")).toBeTruthy();
  expect(screen.getByText("2026.11.10 00:00:00Z")).toBeTruthy();
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
