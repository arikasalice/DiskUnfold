import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { FileInspection } from "../src/features/analysis/FileInspection";
import { unreadValueText } from "../src/features/analysis/unread-value";
import { LocaleTextContext, interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import type { AnalysisClient, BrowseDetail } from "../src/ipc";
import { folderDetail, liveScan } from "./summary-fixture";

/*
 * F03 #239: an item detail names why the scan could not read a value. The
 * wording is the design team's (F02 README, item detail unknown reasons) and
 * comes from the bundles the product ships.
 */

afterEach(cleanup);

type Entry = [key: string, text: string, parameters: unknown[]];

function bundle(language: "en" | "ko"): LocaleText {
  const data = JSON.parse(
    readFileSync(
      resolve(process.cwd(), "../user-env/i18n/" + language + ".json"),
      "utf8",
    ),
  ) as unknown[];
  const messages = new Map(
    (data.find(Array.isArray) as Entry[]).map(([key, value]) => [key, value]),
  );
  return (key, fallback, values) =>
    interpolate(messages.get(key) ?? fallback, (name) =>
      String(values?.[name] ?? ""),
    );
}

const korean = bundle("ko");
const english = bundle("en");

test("each reason key reads as the designed value and description line", () => {
  const expected = [
    [
      "os.attribute_unsupported",
      { value: "이 항목에는 제공되지 않음", notes: [] },
      { value: "Not provided for this item", notes: [] },
    ],
    [
      "os.query_failed",
      {
        value: "확인 불가",
        notes: [
          "파일 시스템에서 이 값을 읽지 못했습니다. 다시 분석하면 확인될 수 있습니다.",
        ],
      },
      {
        value: "Unavailable",
        notes: [
          "The file system didn't return this value. It may be available if you analyze again.",
        ],
      },
    ],
    [
      "scan.boundary_other_volume",
      {
        value: "확인 불가",
        notes: ["다른 드라이브에 있어 이 분석에 포함하지 않았습니다."],
      },
      {
        value: "Unavailable",
        notes: [
          "This is on another drive, so it wasn't included in this analysis.",
        ],
      },
    ],
  ] as const;
  for (const [key, ko, en] of expected) {
    expect(unreadValueText(key, korean), key).toEqual(ko);
    expect(unreadValueText(key, english), key).toEqual(en);
  }
  // The placeholder keeps its space wording, and any other key the general one.
  for (const key of [
    "space.placeholder_local_unknown",
    "ref.unavailable",
    "space.data_insufficient",
  ]) {
    expect(unreadValueText(key, korean), key).toBeUndefined();
  }
});

test("a value the scan could not read carries its description in the same row", async () => {
  const scan = liveScan();
  const detail: BrowseDetail = {
    ...folderDetail("v1"),
    allocatedSize: {
      state: "unknown",
      reasonKey: "scan.boundary_other_volume",
    },
    modifiedAt: { state: "unknown", reasonKey: "os.attribute_unsupported" },
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
  const note = await screen.findByText(
    "다른 드라이브에 있어 이 분석에 포함하지 않았습니다.",
  );
  expect(note.classList.contains("du-readout__note")).toBe(true);
  expect(note.closest("dd")?.textContent).toContain("확인 불가");
  // A value that is already its reason has no description line.
  fireEvent.click(screen.getByRole("button", { name: /더 많은 관측 속성/ }));
  const unsupported = screen.getByText("이 항목에는 제공되지 않음");
  expect(unsupported.closest("dd")?.querySelector(".du-readout__note")).toBe(
    null,
  );
});
