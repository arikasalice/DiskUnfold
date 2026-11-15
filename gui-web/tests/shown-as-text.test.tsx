import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { AnalysisPanel } from "../src/features/analysis/AnalysisPanel";
import { FileInspection } from "../src/features/analysis/FileInspection";
import { PinnedResult } from "../src/features/analysis/ResultSummary";
import { displayMeasurementLimit } from "../src/features/analysis/space-format";
import { useSummaryBatch } from "../src/features/analysis/summary-batch";
import {
  amount,
  observationRows,
  unavailableValue,
} from "../src/features/analysis/volume-format";
import { LocaleTextContext, interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import type {
  AnalysisClient,
  BrowseDetail,
  SpaceDetail,
  SpaceTotal,
  TargetList,
  VolumeGapResponse,
} from "../src/ipc";
import {
  SummaryHarness,
  folderDetail,
  liveScan,
  path,
  resetReflectOffset,
  totals,
  volumeReport,
} from "./summary-fixture";

/*
 * Values that used to be printed as the engine sends them read as text:
 * the kind of an item, the reasons of fields without a value, the unit
 * system, the reason of a missing volume value, and incoming values the
 * screen has no name for. Texts come from the bundles the product ships.
 */

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  resetReflectOffset();
});

type Entry = [key: string, text: string, parameters: unknown[]];
const bundle = JSON.parse(
  readFileSync(resolve(process.cwd(), "../user-env/i18n/ko.json"), "utf8"),
) as unknown[];
const messages = new Map(
  (bundle.find(Array.isArray) as Entry[]).map(([key, value]) => [key, value]),
);
const ko = (key: string) => {
  const message = messages.get(key);
  if (message === undefined) throw new Error("No Korean message for " + key);
  return message;
};
const korean: LocaleText = (key, fallback, values) =>
  interpolate(messages.get(key) ?? fallback, (name) =>
    String(values?.[name] ?? ""),
  );
const english: LocaleText = (_key, fallback, values) =>
  interpolate(fallback, (name) => String(values?.[name] ?? ""));

/** The item details of `detail`; space details are not part of these tests. */
function inspect(
  detail: BrowseDetail,
  text: LocaleText,
  itemKind: "file" | "folder" = "folder",
  space?: SpaceDetail,
) {
  const scan = liveScan();
  const client: AnalysisClient = {
    ...scan.client,
    call: ((name: string, body: never) =>
      name === "browse.detail"
        ? Promise.resolve(detail)
        : name === "space.detail" && space
          ? Promise.resolve(space)
          : scan.client.call(name as never, body)) as AnalysisClient["call"],
  };
  render(
    <LocaleTextContext.Provider value={text}>
      <FileInspection
        client={client}
        item={{ resultVersion: "v1", itemNo: 0 }}
        itemKind={itemKind}
      />
    </LocaleTextContext.Provider>,
  );
}

const rowOf = (label: string) =>
  within(screen.getByText(label).closest("div") as HTMLElement);

test.each([
  ["directory", "폴더", "Folder"],
  ["file", "파일", "File"],
  ["untraversed_link", "따라가지 않은 링크", "Link (not followed)"],
  ["socket", "알 수 없음", "Unknown"],
] as const)(
  "the kind %s of an item reads as text",
  async (kind, koreanName, englishName) => {
    const detail: BrowseDetail = {
      ...folderDetail("v1"),
      kind: { state: "present", value: kind as never },
    };
    inspect(detail, korean);
    await screen.findByText("종류");
    expect(rowOf("종류").getByText(koreanName)).toBeDefined();
    expect(screen.queryByText(kind)).toBeNull();
    cleanup();
    inspect(detail, english);
    await screen.findByText("Kind");
    expect(rowOf("Kind").getByText(englishName)).toBeDefined();
    expect(screen.queryByText(kind)).toBeNull();
  },
);

const withoutValues: BrowseDetail = {
  ...folderDetail("v1"),
  displayName: { state: "unknown", reasonKey: "ref.unavailable" },
  kind: { state: "unknown", reasonKey: "future.reason" },
  allocatedSize: { state: "unknown", reasonKey: "space.data_insufficient" },
  createdAt: { state: "unknown", reasonKey: "ref.unavailable" },
};

test("diagnostic reasons read as one line per field: its label, then why", async () => {
  inspect(withoutValues, korean);
  const toggle = await screen.findByRole("button", { name: /진단 사유/ });
  // The section stays collapsed until it is opened.
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(screen.queryByText(/이 결과에 값이 없습니다/)).toBeNull();
  fireEvent.click(toggle);
  for (const line of [
    "폴더 이름: 이 결과에 값이 없습니다",
    "종류: 알 수 없는 사유 (future.reason)",
    "할당 크기: 일부 공간 데이터를 확인할 수 없습니다.",
    ko("ui.inspection.created") + ": 이 결과에 값이 없습니다",
  ])
    expect(screen.getByText(line)).toBeDefined();
  // A key with a text is not shown; a key without one shows only here.
  expect(
    screen.queryByText(/ref\.unavailable|space\.data_insufficient/),
  ).toBeNull();
  expect(screen.getAllByText(/future\.reason/)).toHaveLength(1);
});

test("diagnostic reasons name the file name of a file, and read in English without a bundle", async () => {
  inspect(withoutValues, english, "file");
  fireEvent.click(
    await screen.findByRole("button", { name: /Diagnostic reasons/ }),
  );
  for (const line of [
    "File name: Not included in this result",
    "Kind: Unknown reason (future.reason)",
    "Allocated size: Some space data could not be verified",
    "Created: Not included in this result",
  ])
    expect(screen.getByText(line)).toBeDefined();
});

test("the hard-link identity limit has its text in both languages", () => {
  expect(
    displayMeasurementLimit("space.hardlink_identity_unknown", korean),
  ).toBe("식별할 수 없는 파일의 할당 크기는 합계에 넣지 않았습니다.");
  expect(
    displayMeasurementLimit("space.hardlink_identity_unknown", english),
  ).toBe(
    "The allocated size of files that could not be identified was left out of the total.",
  );
});

function Summary({
  client,
  total,
  system,
}: {
  client: AnalysisClient;
  total: SpaceTotal;
  system: "iec" | "si";
}) {
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
      exclusions={[]}
      preferences={{
        display: { system, unit: "auto" },
        basis: "logical",
        warning: undefined,
      }}
      displayChoice={undefined}
      onDisplayChoice={() => {}}
      onReopenLatest={() => {}}
    />
  );
}

test.each([
  ["iec", "이진 단위(KiB, MiB)", "Binary units (KiB, MiB)"],
  ["si", "십진 단위(kB, MB)", "Decimal units (kB, MB)"],
] as const)(
  "the summary names the unit system %s by its units",
  (system, koreanName, englishName) => {
    const client = liveScan().client;
    const total = totals("v1", 1, true);
    const { rerender } = render(
      <LocaleTextContext.Provider value={korean}>
        <Summary client={client} total={total} system={system} />
      </LocaleTextContext.Provider>,
    );
    expect(
      screen.getByText("논리 크기 · " + koreanName + " · 자동"),
    ).toBeDefined();
    rerender(
      <LocaleTextContext.Provider value={english}>
        <Summary client={client} total={total} system={system} />
      </LocaleTextContext.Provider>,
    );
    expect(
      screen.getByText("Logical size · " + englishName + " · Automatic"),
    ).toBeDefined();
    expect(screen.queryByText(/· (IEC|SI) ·/)).toBeNull();
  },
);

test.each([
  [
    korean,
    "표시 옵션",
    "단위 체계",
    "이진 단위(KiB, MiB)",
    "십진 단위(kB, MB)",
  ],
  [
    english,
    "Display options",
    "Unit system",
    "Binary units (KiB, MiB)",
    "Decimal units (kB, MB)",
  ],
] as const)(
  "the display options offer the unit systems by the names of the summary",
  (text, options, field, binary, decimal) => {
    render(
      <LocaleTextContext.Provider value={text}>
        <Summary
          client={liveScan().client}
          total={totals("v1", 1, true)}
          system="iec"
        />
      </LocaleTextContext.Provider>,
    );
    fireEvent.click(screen.getByRole("button", { name: new RegExp(options) }));
    const choices = within(
      screen.getByRole("combobox", { name: field }),
    ).getAllByRole("option");
    expect(choices.map((choice) => choice.textContent)).toEqual([
      binary,
      decimal,
    ]);
  },
);

test("details are called 상세 정보 and a link that was not followed has one name", () => {
  expect(ko("ui.browse.folder_details")).toBe("폴더 상세 정보");
  expect(ko("ui.browse.file_details")).toBe("파일 상세 정보");
  expect(ko("ui.browse.observation_details")).toBe("관측 상세 정보");
  expect(ko("ui.analysis.volume_details_title")).toBe("관측 및 차이 상세 정보");
  expect(
    [...messages].filter(
      ([key, value]) =>
        /^ui\.(analysis|browse|inspection)\./.test(key) &&
        JSON.stringify(value).includes("세부 정보"),
    ),
  ).toEqual([]);
  // The browse list and the item details share the name of the summary.
  expect(ko("ui.browse.type_untraversed_link")).toBe(
    ko("ui.analysis.untraversed_links"),
  );
  expect(messages.has("ui.inspection.kind_untraversed_link")).toBe(false);
});

const display = { system: "iec", unit: "auto" } as const;

test("a missing volume value says why when its reason has a text", () => {
  expect(unavailableValue("space.volume_report_unavailable", korean)).toBe(
    "확인 불가 · 운영 체제가 볼륨 사용량을 알려 주지 않았습니다.",
  );
  expect(unavailableValue("space.volume_report_unavailable", english)).toBe(
    "Unavailable · The operating system did not report the volume usage.",
  );
  expect(
    amount(
      { state: "unknown", reasonKey: "space.data_insufficient" },
      display,
      korean,
    ),
  ).toBe("확인 불가 · 일부 공간 데이터를 확인할 수 없습니다.");
  // A reason without a text is not shown.
  expect(
    amount({ state: "unknown", reasonKey: "space.future" }, display, korean),
  ).toBe("확인 불가");
  const [row] = observationRows(
    "end",
    {
      state: "unavailable",
      observedAt: null,
      reasonKey: "space.volume_report_unavailable",
    },
    display,
    korean,
  );
  expect(row?.value).toBe(
    "확인 불가 · " +
      ko("ui.analysis.volume_time_unavailable") +
      " · 운영 체제가 볼륨 사용량을 알려 주지 않았습니다.",
  );
});

test("the volume explanation shows reasons and no factor or category keys", async () => {
  const report: VolumeGapResponse = {
    ...volumeReport("v1"),
    end: {
      state: "unavailable",
      observedAt: null,
      reasonKey: "space.volume_report_unavailable",
    },
    factors: [
      {
        key: "gap.excluded",
        value: { state: "exact", bytes: "100", evidence: "estimated" },
      },
      {
        key: "gap.future_factor",
        value: {
          state: "unknown",
          reasonKey: "space.volume_report_unavailable",
        },
      },
    ],
    osCategories: {
      state: "available",
      rows: [
        {
          key: "system_data",
          value: { state: "exact", bytes: "200", evidence: "os_reported" },
        },
      ],
    },
  };
  const scan = liveScan();
  const client: AnalysisClient = {
    ...scan.client,
    call: ((name: string, body: { requestSeq: number }) =>
      name === "space.volume_gap"
        ? Promise.resolve({ ...report, requestSeq: body.requestSeq })
        : scan.client.call(
            name as never,
            body as never,
          )) as AnalysisClient["call"],
  };
  const unavailable =
    "확인 불가 · 운영 체제가 볼륨 사용량을 알려 주지 않았습니다.";
  const view = render(
    <LocaleTextContext.Provider value={korean}>
      <SummaryHarness client={client} version="v1" live={false} />
    </LocaleTextContext.Provider>,
  );
  fireEvent.click(
    screen.getByRole("button", { name: ko("ui.analysis.volume_explain") }),
  );
  const summary = within(
    await screen.findByLabelText(ko("ui.analysis.volume_summary")),
  );
  expect(
    within(
      summary.getByText(ko("ui.analysis.volume_used_end")).closest("div")!,
    ).getByText(unavailable),
  ).toBeDefined();
  fireEvent.click(
    screen.getByRole("button", {
      name: new RegExp(ko("ui.analysis.volume_details_title")),
    }),
  );
  const differences = within(
    screen.getByLabelText(ko("ui.analysis.volume_difference")),
  );
  // The factor without a name reads as unknown, with why it has no value.
  expect(
    within(differences.getByText("알 수 없음").closest("div")!).getByText(
      unavailable,
    ),
  ).toBeDefined();
  expect(
    within(
      differences
        .getByText(ko("ui.analysis.volume_os_system_categories"))
        .closest("div")!,
    ).getByText(/^알 수 없음 · /),
  ).toBeDefined();
  expect(
    view.container.textContent?.match(/gap\.|system_data|space\./),
  ).toBeNull();
});

test("item space details show no capability key, state or residency value", async () => {
  const missing = {
    state: "unknown",
    reasonKey: "space.data_insufficient",
  } as const;
  const space: SpaceDetail = {
    item: { resultVersion: "v1", itemNo: 0 },
    logical: missing,
    allocated: missing,
    uniquePhysical: missing,
    exclusive: missing,
    shared: missing,
    linkGroup: missing,
    placeholderLocalState: { state: "present", value: "partial" },
    auxiliary: missing,
    exclusiveDecision: { confirmed: false, reasonKey: null },
    observedAt: missing,
    capabilities: {
      state: "present",
      value: [{ key: "sparse_files", state: "supported", reasonKey: null }],
    },
  };
  inspect(folderDetail("v1"), korean, "folder", space);
  fireEvent.click(
    await screen.findByRole("button", {
      name: new RegExp(ko("ui.analysis.space_measurement_details")),
    }),
  );
  for (const label of [
    ko("ui.analysis.space_capabilities"),
    ko("ui.analysis.space_placeholder_residency"),
  ])
    expect(rowOf(label).getByText("알 수 없음")).toBeDefined();
  expect(screen.queryByText(/sparse_files|supported|partial/)).toBeNull();
});

test("a target kind or access state without a name reads as unknown", async () => {
  const scan = liveScan();
  const client: AnalysisClient = {
    ...scan.client,
    call: (async (name: string, body: never) => {
      const response = await scan.client.call(name as never, body);
      if (name !== "target.list") return response;
      const list = response as TargetList;
      return {
        ...list,
        targets: list.targets.map((target) => ({
          ...target,
          kind: "ramdisk",
          access: "half_open",
        })),
      };
    }) as AnalysisClient["call"],
  };
  const view = render(
    <AnalysisPanel
      client={client}
      preferences={{
        display: { system: "iec", unit: "auto" },
        basis: "logical",
        warning: undefined,
      }}
    />,
  );
  await screen.findByRole("radio", { name: path.display });
  expect(screen.getByText(/^Unknown · /)).toBeDefined();
  expect(screen.getByText("unknown")).toBeDefined();
  expect(
    view.container.textContent?.match(/ramdisk|half_open|half open/),
  ).toBeNull();
});
