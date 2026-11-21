import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { TypeSummaryBar } from "../src/features/analysis/TypeSummaryBar";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { koreanText } from "../preview/result-summary-fixture";
import type { AnalysisClient, SizeValue, SpaceTypeSummary } from "../src/ipc";

afterEach(cleanup);

const korean: LocaleText = koreanText;

const version = "018f0000-0000-7000-8000-0000000000b1";
const MiB = 1024 * 1024;
const slice = (classKey: string, mebibytes: number, lower = false) => ({
  classKey,
  itemCount: { unit: "items", state: "exact" as const, value: "3" },
  total: {
    unit: "bytes",
    state: lower ? ("lower_bound" as const) : ("exact" as const),
    value: String(mebibytes * MiB),
  },
});
const legend = [
  "type.document",
  "type.audio",
  "type.image",
  "type.installer_image",
  "type.video",
  "type.archive",
  "type.unclassified",
  "type.no_extension",
  "unclassified",
];
const exact = (mebibytes: number): SizeValue => ({
  basis: "allocated",
  evidence: "os_reported",
  state: "exact",
  bytes: String(mebibytes * MiB),
});

function answering(summary: SpaceTypeSummary | Error) {
  const call = vi.fn(() =>
    summary instanceof Error
      ? Promise.reject(summary)
      : Promise.resolve(summary),
  );
  return {
    call,
    client: { call, subscribe: () => () => {} } as unknown as AnalysisClient,
  };
}

function show(
  client: AnalysisClient,
  basis: "allocated" | "unique_physical" = "allocated",
  size?: SizeValue,
) {
  return render(
    <LocaleTextContext.Provider value={korean}>
      <TypeSummaryBar
        client={client}
        resultVersion={version}
        basis={basis}
        lead={
          size === undefined
            ? undefined
            : { size, completeness: "complete", measurementLimits: [] }
        }
        awaited={false}
        display={{ system: "iec", unit: "auto" }}
      />
    </LocaleTextContext.Provider>,
  );
}

test("F02 #300: the five largest types and the rest are a bar and a list in the same order", async () => {
  const { call, client } = answering({
    resultVersion: version,
    slices: [
      slice("type.video", 400),
      slice("type.document", 300),
      slice("type.image", 200),
      slice("type.audio", 100),
      slice("type.archive", 50),
      slice("type.no_extension", 30),
      slice("unclassified", 20),
    ],
    legendOrder: legend,
    lowerBound: false,
  });
  const { container } = show(client, "allocated", exact(1200));
  const card = await screen.findByRole("region", {
    name: "이 범위는 무엇으로 차 있나",
  });
  expect(call).toHaveBeenCalledWith("space.type_summary", {
    resultVersion: version,
    basis: "allocated",
  });
  // Five named types, largest first, and one slice for the rest, in a bar 12 px high.
  const slices = container.querySelectorAll(".du-stacked-bar__slice");
  expect([...slices].map((s) => s.getAttribute("data-key"))).toEqual([
    "type.video",
    "type.document",
    "type.image",
    "type.audio",
    "type.archive",
    "other",
  ]);
  expect(
    container.querySelector(".du-stacked-bar")?.getAttribute("data-height"),
  ).toBe("12");
  // The same content as a list: a colour square before each item, spaces inside it,
  // the rest of the types, and the part no type holds last.
  const items = [...card.querySelectorAll("li")].map((li) => li.textContent);
  expect(items).toEqual([
    "동영상 400.0 MiB",
    "문서 300.0 MiB",
    "이미지 200.0 MiB",
    "오디오 100.0 MiB",
    "압축 파일 50.0 MiB",
    "나머지 유형 2개 50.0 MiB",
    "유형별로 나누지 못함 100.0 MiB",
  ]);
  expect(card.querySelectorAll("li > [data-tone]")).toHaveLength(7);
  expect(card.querySelector('[data-tone="unsplit"]')).not.toBeNull();
});

test("F02 #300: an empty answer is a scope without files only when every range was read", async () => {
  const none = answering({
    resultVersion: version,
    slices: [],
    legendOrder: legend,
    lowerBound: false,
  });
  const { container } = show(none.client);
  await waitFor(() => expect(none.call).toHaveBeenCalled());
  await Promise.resolve();
  expect(container.querySelector(".du-type-summary")).toBeNull();
  cleanup();
  const unread = answering({
    resultVersion: version,
    slices: [],
    legendOrder: legend,
    lowerBound: true,
  });
  show(unread.client);
  expect(
    (await screen.findByRole("region", { name: "이 범위는 무엇으로 차 있나" }))
      .textContent,
  ).toContain("확인 불가");
});

test("F02 #300: a failed answer, another version and unique physical size draw nothing", async () => {
  const failing = answering(new Error("refused"));
  const { container, unmount } = show(failing.client);
  await waitFor(() => expect(failing.call).toHaveBeenCalled());
  await Promise.resolve();
  expect(container.querySelector(".du-type-summary")).toBeNull();
  unmount();
  const other = answering({
    resultVersion: "018f0000-0000-7000-8000-0000000000b2",
    slices: [slice("type.video", 1)],
    legendOrder: legend,
    lowerBound: false,
  });
  const second = show(other.client);
  await waitFor(() => expect(other.call).toHaveBeenCalled());
  await Promise.resolve();
  expect(second.container.querySelector(".du-type-summary")).toBeNull();
  second.unmount();
  const physical = answering({
    resultVersion: version,
    slices: [slice("type.video", 1)],
    legendOrder: legend,
    lowerBound: false,
  });
  const third = show(physical.client, "unique_physical");
  await Promise.resolve();
  expect(physical.call).not.toHaveBeenCalled();
  expect(third.container.querySelector(".du-type-summary")).toBeNull();
});
