import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { useState } from "react";
import type { ReactNode } from "react";
import { afterEach, expect, test } from "vitest";
import { AnalysisPanel } from "../src/features/analysis/AnalysisPanel";
import { PinnedResult } from "../src/features/analysis/ResultSummary";
import { exclusionText } from "../src/features/analysis/scope-exclusion";
import { useSummaryBatch } from "../src/features/analysis/summary-batch";
import { LocaleTextContext, interpolate } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import { windowsPath } from "../src/ipc";
import type {
  AnalysisClient,
  ScopeDraft,
  ScopeExclusion,
  ScopeReview,
} from "../src/ipc";
import { liveScan, path, totals } from "./summary-fixture";

/*
 * The result summary names the exclusion rules of the scope its result was
 * analyzed with, below the scanned paths.
 */

afterEach(cleanup);

type Message =
  string | { kind: "plural"; variants: { one?: string; other: string } };

/** Texts from the bundle the product ships, with its plural rule. */
function bundleText(language: "en" | "ko"): LocaleText {
  const data = JSON.parse(
    readFileSync(
      resolve(process.cwd(), "../user-env/i18n/" + language + ".json"),
      "utf8",
    ),
  ) as unknown[];
  const messages = new Map(
    (data.find(Array.isArray) as [string, Message][]).map(([key, value]) => [
      key,
      value,
    ]),
  );
  return (key, fallback, values) => {
    const message = messages.get(key);
    const template =
      message === undefined
        ? fallback
        : typeof message === "string"
          ? message
          : ((language === "en" && values?.count === 1n
              ? message.variants.one
              : undefined) ?? message.variants.other);
    return interpolate(template, (name) => String(values?.[name] ?? ""));
  };
}

const english = bundleText("en");
const korean = bundleText("ko");

const pathRule = (raw: string): ScopeExclusion => ({
  kind: "path",
  path: windowsPath(raw),
});
const patternRule = (pattern: string): ScopeExclusion => ({
  kind: "name_pattern",
  pattern,
});
const rules = (count: number) =>
  Array.from({ length: count }, (_, index) =>
    pathRule("C:\\fixture\\excluded-" + String(index + 1)),
  );

function Summary({
  exclusions,
}: {
  exclusions: readonly ScopeExclusion[] | undefined;
}) {
  const [scan] = useState(liveScan);
  const [total] = useState(() => totals("v1", 1, true));
  const summary = useSummaryBatch(scan.client, total, false, false);
  return (
    <PinnedResult
      client={scan.client}
      total={total}
      latestVersion={total.resultVersion}
      volume={summary.volume}
      scan={undefined}
      wait={undefined}
      roots={[path]}
      exclusions={exclusions}
      preferences={{
        display: { system: "iec", unit: "auto" },
        basis: "logical",
        warning: undefined,
      }}
      displayChoice={undefined}
      onDisplayChoice={() => {}}
      onReopenLatest={() => {}}
    />
  );
}

function inLocale(text: LocaleText, children: ReactNode) {
  return (
    <LocaleTextContext.Provider value={text}>
      {children}
    </LocaleTextContext.Provider>
  );
}

function showSummary(
  exclusions: readonly ScopeExclusion[] | undefined,
  text = english,
) {
  return render(inLocale(text, <Summary exclusions={exclusions} />));
}

const LABEL = 'dt, dt [role="note"]';

/** The label element of a result summary row. */
function rowLabel(label: string) {
  const summary = document.querySelector<HTMLElement>(".du-results")!;
  return within(summary).getByText(label, { selector: LABEL });
}

/** The value of a result summary row, found by its label. */
function rowValue(label: string) {
  return rowLabel(label).closest("dt")!.nextElementSibling as HTMLElement;
}

/** The accessible description of a row's label. */
function labelDescription(label: string) {
  const id = rowLabel(label).getAttribute("aria-describedby");
  return id ? document.getElementById(id)?.textContent : undefined;
}

test("a scope without exclusion rules shows no exclusions row", () => {
  showSummary([]);
  expect(rowValue("Scanned paths").textContent).toBe(path.display);
  expect(screen.queryByText("Exclusions", { selector: LABEL })).toBeNull();
});

test("one path rule follows the scanned paths and breaks after its separators", () => {
  showSummary([pathRule("C:\\fixture\\cache")]);
  const terms = screen.getAllByRole("term");
  expect(terms[0]).toBe(rowLabel("Scanned paths"));
  expect(terms[1]).toBe(rowLabel("Exclusions").closest("dt"));
  const value = rowValue("Exclusions");
  expect(value.textContent).toBe("C:\\fixture\\cache");
  expect(value.querySelector(".du-path")).not.toBeNull();
  expect(value.querySelectorAll("wbr")).toHaveLength(2);
  expect(within(value).queryByRole("button")).toBeNull();
});

test("three rules show in full without a toggle, patterns as written", () => {
  showSummary([
    pathRule("C:\\fixture\\a"),
    patternRule("*.tmp"),
    pathRule("C:\\fixture\\b"),
  ]);
  const value = rowValue("Exclusions");
  expect(value.textContent).toBe("C:\\fixture\\a, *.tmp, C:\\fixture\\b");
  expect(within(value).queryByRole("button")).toBeNull();
});

test("a rule and its review row read the same", () => {
  expect(exclusionText(pathRule("C:\\fixture\\a"))).toBe("C:\\fixture\\a");
  expect(exclusionText(patternRule(" *.tmp"))).toBe(" *.tmp");
});

test("more than three rules fold behind a toggle that keeps the focus", () => {
  showSummary(rules(5));
  const value = rowValue("Exclusions");
  const toggle = within(value).getByRole("button", {
    name: "2 more exclusions",
  });
  expect(toggle.textContent).toBe("2 more");
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(value.textContent).toBe(
    "C:\\fixture\\excluded-1, C:\\fixture\\excluded-2, C:\\fixture\\excluded-3 2 more",
  );

  toggle.focus();
  fireEvent.click(toggle);
  expect(document.activeElement).toBe(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("true");
  expect(toggle.getAttribute("aria-label")).toBe("Show less of the exclusions");
  expect(toggle.textContent).toBe("Show less");
  expect(value.textContent).toContain("C:\\fixture\\excluded-5 Show less");

  fireEvent.click(toggle);
  expect(document.activeElement).toBe(toggle);
  expect(toggle.getAttribute("aria-expanded")).toBe("false");
  expect(value.textContent).not.toContain("excluded-4");
});

test("one folded rule reads in the singular", () => {
  showSummary(rules(4));
  const toggle = screen.getByRole("button", { name: "1 more exclusion" });
  expect(toggle.textContent).toBe("1 more");
});

test("the rules of another analysis start folded", () => {
  const view = showSummary(rules(5));
  fireEvent.click(screen.getByRole("button", { name: "2 more exclusions" }));
  expect(rowValue("Exclusions").textContent).toContain("excluded-5");
  view.rerender(inLocale(english, <Summary exclusions={rules(6)} />));
  expect(
    screen.getByRole("button", { name: "3 more exclusions" }),
  ).toBeDefined();
  expect(rowValue("Exclusions").textContent).not.toContain("excluded-4");
});

test("an unknown scope keeps the row and reads as unavailable", () => {
  showSummary(undefined);
  expect(rowValue("Exclusions").textContent).toBe("Unavailable");
  expect(labelDescription("Exclusions")).toBe(
    "The scope used for this result isn't available now.",
  );
});

test("the label explains the rules on focus, and no other label takes focus", async () => {
  const explanation =
    "Folders and files matching these weren't analyzed. They aren't included in the size or the item count.";
  showSummary(rules(2));
  const label = rowLabel("Exclusions");
  expect(labelDescription("Exclusions")).toBe(explanation);
  expect(rowLabel("Scanned paths").tagName).toBe("DT");
  expect(rowLabel("Scanned paths").querySelector("[tabindex]")).toBeNull();
  act(() => label.focus());
  expect((await screen.findByRole("tooltip")).textContent).toBe(explanation);
  fireEvent.keyDown(label, { key: "Escape" });
  expect(screen.queryByRole("tooltip")).toBeNull();
});

test("the row, its toggle and an unknown scope read in Korean", () => {
  const view = showSummary(rules(5), korean);
  const value = rowValue("제외 규칙");
  const toggle = within(value).getByRole("button", {
    name: "제외 규칙 외 2개",
  });
  expect(toggle.textContent).toBe("외 2개");
  fireEvent.click(toggle);
  expect(toggle.getAttribute("aria-label")).toBe("제외 규칙 접기");
  expect(toggle.textContent).toBe("접기");
  expect(labelDescription("제외 규칙")).toBe(
    "이 규칙에 걸린 폴더와 파일은 분석하지 않았습니다. 크기와 항목 수에 들어 있지 않습니다",
  );
  view.unmount();
  showSummary(undefined, korean);
  expect(rowValue("제외 규칙").textContent).toBe("확인 불가");
  expect(labelDescription("제외 규칙")).toBe(
    "이 결과를 만든 분석 범위를 지금 확인할 수 없습니다",
  );
});

const timestamp = "2026-11-15T00:00:00.000Z";

/** A saved scope whose review returns the rules in the engine's own text. */
function savedScope(raws: readonly string[]): {
  draft: ScopeDraft;
  reviewed: ScopeDraft;
} {
  const draft: ScopeDraft = {
    includes: [path],
    exclusions: raws.map((raw) => pathRule(raw.toLowerCase())),
    boundary: "current_volume",
    includedMounts: [],
    source: "recent",
  };
  return {
    draft,
    reviewed: { ...draft, exclusions: raws.map((raw) => pathRule(raw)) },
  };
}

/** The live scan fixture with two saved scopes to start from. */
function scanWithSavedScopes(saved: ReturnType<typeof savedScope>[]) {
  const scan = liveScan();
  const base = scan.client.call;
  const client: AnalysisClient = {
    ...scan.client,
    recentScopesEnabled: true,
    call: (async (name: string, body: unknown) => {
      const request = body as { recentScopeRef: string; requestSeq: number };
      switch (name) {
        case "scope.recent_list":
          return {
            rows: saved.map((_, index) => ({
              recentScopeRef: "saved-" + String(index),
              includeCount: 1,
              lastUsedAt: timestamp,
            })),
          };
        case "scope.recent_open": {
          const { draft, reviewed } =
            saved[Number(request.recentScopeRef.slice(6))]!;
          return {
            draft,
            targets: [{ path, state: "matched", proposedEntry: null }],
            review: {
              scope: reviewed,
              targets: [{ path, access: "accessible", reasonKey: null }],
              notices: [],
              reviewFingerprint: "recent-review",
              observedAt: timestamp,
              requestSeq: request.requestSeq,
            } satisfies ScopeReview,
          };
        }
        default:
          return base(name as never, body as never);
      }
    }) as AnalysisClient["call"],
  };
  return { ...scan, client };
}

async function startSaved(number: number, replace: boolean) {
  fireEvent.click(
    screen.getByRole("button", { name: "Recent analysis ranges" }),
  );
  fireEvent.click(
    await screen.findByRole("button", {
      name: "Review range " + String(number),
    }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Use this range" }),
  );
  if (replace)
    fireEvent.click(
      screen.getByRole("button", { name: "Replace current scope" }),
    );
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start analysis",
  });
  await waitFor(() => expect(start.disabled).toBe(false));
  fireEvent.click(start);
}

test("the summary keeps the started scope's rules until another analysis starts", async () => {
  const two = savedScope(["C:\\fixture\\Cache", "C:\\fixture\\Logs"]);
  const five = savedScope(rules(5).map(exclusionText));
  const scan = scanWithSavedScopes([two, five]);
  render(
    <AnalysisPanel
      client={scan.client}
      preferences={{
        display: { system: "iec", unit: "auto" },
        basis: "logical",
        warning: undefined,
      }}
    />,
  );
  await startSaved(1, false);
  await screen.findByText("Result version: Interim");
  // The rules come from the review the scan was admitted with, as the
  // scanned paths do, not from the draft that was submitted.
  expect(rowValue("Exclusions").textContent).toBe(
    "C:\\fixture\\Cache, C:\\fixture\\Logs",
  );

  scan.changeState("done");
  fireEvent.click(await screen.findByRole("button", { name: "Analyze again" }));
  fireEvent.change(
    screen.getByRole("textbox", { name: "Windows folder path" }),
    { target: { value: "C:\\elsewhere" } },
  );
  expect(rowValue("Exclusions").textContent).toBe(
    "C:\\fixture\\Cache, C:\\fixture\\Logs",
  );

  await startSaved(2, true);
  await waitFor(() =>
    expect(
      screen.getByRole("button", { name: "2 more exclusions" }),
    ).toBeDefined(),
  );
  expect(rowValue("Exclusions").textContent).toContain(
    "C:\\fixture\\excluded-3",
  );
});
