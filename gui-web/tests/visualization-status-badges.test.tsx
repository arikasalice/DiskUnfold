import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { modelFixture } from "../preview/model-fixture";
import { VisualizationModelPresenter } from "../src/features/visualization/VisualizationModelPresenter";
import { createProductionVizText, visualizationMessage } from "../src/viz";
import type { VizText } from "../src/viz";

afterEach(cleanup);

type Language = "en" | "ko";
const textFor = (language: Language): VizText =>
  createProductionVizText(
    (key, values) =>
      Object.entries(values ?? {}).reduce(
        (message, [name, value]) => message.replace(`{${name}}`, String(value)),
        visualizationMessage(language, key),
      ),
    { system: "iec", unit: "auto" },
  );
const view = {
  viewport: { width: 960, height: 330, scale: 1 },
  visibleWindow: { left: 0, top: 0, right: 1000000, bottom: 1000000 },
};

function presenter(
  edit: (input: ReturnType<typeof modelFixture>) => void,
  language: Language = "en",
) {
  const input = modelFixture();
  edit(input);
  return render(
    <VisualizationModelPresenter
      {...input}
      view={view}
      selected={null}
      onSelect={vi.fn()}
      text={textFor(language)}
    />,
  );
}

const unwatchedLine = {
  en: "Changes made after the analysis aren't reflected",
  ko: "분석한 뒤에 바뀐 내용은 반영되지 않습니다",
} as const;
const badges = (container: HTMLElement) =>
  [...container.querySelectorAll(".du-badge")].map((node) => node.textContent);
const header = (container: HTMLElement) =>
  container.querySelector(".du-review-context")?.textContent ?? "";

test.each(["en", "ko"] as const)(
  "a complete, current-analysis, unwatched result has no badge and one quiet line (%s)",
  (language) => {
    const { container } = presenter((input) => {
      input.frame.completeness = "complete";
      input.frame.model.origin = "scan";
      input.frame.model.staleness = {
        state: "unknown",
        reasonKey: "viz.staleness.unwatched",
      };
    }, language);
    expect(badges(container)).toEqual([]);
    expect(header(container)).toContain(unwatchedLine[language]);
    // The line is plain text outside the status region, so it is never announced.
    expect(
      screen
        .getAllByRole("status")
        .some((node) => node.textContent?.includes(unwatchedLine[language])),
    ).toBe(false);
  },
);

test.each([
  ["en", "Scanning · Interim"],
  ["ko", "스캔 중 · 중간 결과"],
] as const)(
  "an interim result has one info badge and no unwatched line (%s)",
  (language, label) => {
    const { container } = presenter((input) => {
      input.frame.completeness = "partial";
      input.frame.model.origin = "current_analysis";
      input.frame.model.staleness = {
        state: "unknown",
        reasonKey: "viz.staleness.unwatched",
      };
    }, language);
    expect(badges(container)).toEqual([label]);
    expect(container.querySelector(".du-badge--info")).not.toBeNull();
    expect(header(container)).not.toContain(unwatchedLine[language]);
  },
);

test.each([
  ["en", "Has scan gaps"],
  ["ko", "누락 있음"],
] as const)(
  "a result with missing items has a warning badge (%s)",
  (language, label) => {
    const { container } = presenter((input) => {
      input.frame.completeness = "missing";
      input.frame.model.staleness = { state: "current", reasonKey: null };
    }, language);
    expect(badges(container)).toEqual([label]);
    expect(container.querySelector(".du-badge--warning")).not.toBeNull();
    expect(header(container)).not.toContain(unwatchedLine[language]);
  },
);

test.each([
  ["en", "Changed since analysis"],
  ["ko", "분석한 뒤 변경됨"],
] as const)("a stale result has a warning badge (%s)", (language, label) => {
  const { container } = presenter((input) => {
    input.frame.completeness = "complete";
    input.frame.model.staleness = { state: "stale", reasonKey: null };
  }, language);
  expect(badges(container)).toEqual([label]);
  expect(container.querySelector(".du-badge--warning")).not.toBeNull();
});

test.each([
  ["en", "Freshness unknown", "The reason is unavailable"],
  ["ko", "최신 여부 확인 불가", "사유를 확인할 수 없습니다"],
] as const)(
  "freshness unknown for another reason has a neutral badge and the reason (%s)",
  (language, label, reason) => {
    const { container } = presenter((input) => {
      input.frame.completeness = "complete";
      input.frame.model.staleness = {
        state: "unknown",
        reasonKey: "viz.staleness.some_future_reason",
      };
    }, language);
    expect(badges(container)).toEqual([label]);
    expect(container.querySelector(".du-badge--neutral")).not.toBeNull();
    expect(header(container)).toContain(reason);
    expect(header(container)).not.toContain(unwatchedLine[language]);
  },
);

test("a result from another origin keeps an origin badge", () => {
  const { container } = presenter((input) => {
    input.frame.completeness = "complete";
    input.frame.model.origin = "snapshot";
    input.frame.model.staleness = { state: "current", reasonKey: null };
  });
  expect(badges(container)).toEqual(["Unknown"]);
});

test("the evidence table names completeness, freshness and origin", () => {
  presenter((input) => {
    input.frame.completeness = "complete";
    input.frame.model.origin = "scan";
    input.frame.model.staleness = {
      state: "unknown",
      reasonKey: "viz.staleness.unwatched",
    };
  });
  fireEvent.click(
    screen.getByRole("button", { name: "Measurement basis and totals" }),
  );
  const status = screen.getByLabelText("Result status");
  expect(status.textContent).toContain("Final");
  expect(status.textContent).toContain("Freshness unknown");
  expect(status.textContent).toContain("Current analysis");
});
