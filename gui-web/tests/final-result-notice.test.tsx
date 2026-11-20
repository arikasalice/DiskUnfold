import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { act, cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import {
  FINAL_RESULT_NOTICE_MS,
  ScanOutputHeader,
} from "../src/features/analysis/ScanOutputHeader";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { LocaleText } from "../src/i18n/presentation";
import type { SpaceTotal } from "../src/ipc";
import {
  logical,
  passReflectInterval,
  resetReflectOffset,
  startPanel,
  totals,
} from "./summary-fixture";

/*
 * After a scan ends, its badge can say so before the final result is read.
 * If that takes 0.3 s or longer, the second header line says that the final
 * result is being read; the values stay as received and change together
 * with the result version when it arrives.
 */

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.restoreAllMocks();
  resetReflectOffset();
});

type Scan = Parameters<typeof ScanOutputHeader>[0]["scan"];
const notice = "Reading the final result…";
const interim = totals("v1", 1, false);
const final = totals("v2", 2, true);

/** A scan that ended on "v2" while the hook still holds the interim "v1". */
function ended(overrides: Partial<Scan> = {}): Scan {
  return {
    status: {
      jobRef: "scan",
      kind: "scan.metadata",
      state: "done",
      sequence: 3,
      resultVersion: "v2",
      reasonKey: null,
    },
    progress: undefined,
    currentVersion: "v2",
    running: false,
    control: undefined,
    controlError: undefined,
    requestControl: () => {},
    total: interim,
    hasNewResult: false,
    showLatest: () => {},
    switchingResult: false,
    error: undefined,
    unavailable: false,
    ...overrides,
  } as Scan;
}

function Header({
  scan = ended(),
  total = interim,
  holding = false,
}: {
  scan?: Scan;
  total?: SpaceTotal;
  holding?: boolean;
}) {
  return (
    <ScanOutputHeader
      scan={scan}
      total={total}
      versionSummary={{ current: null }}
      started
      running={scan.running}
      holding={holding}
      awaitingTotals={false}
      actions={null}
      roots={[]}
    />
  );
}

const pass = (milliseconds: number) =>
  act(() => {
    vi.advanceTimersByTime(milliseconds);
  });

test("the notice appears once the final result has taken 0.3 s", () => {
  vi.useFakeTimers();
  render(<Header />);
  expect(screen.getByText("Scan finished")).toBeDefined();
  expect(screen.queryByText(notice)).toBeNull();
  pass(290);
  expect(screen.queryByText(notice)).toBeNull();
  pass(FINAL_RESULT_NOTICE_MS - 291);
  expect(screen.queryByText(notice)).toBeNull();
  pass(1);
  expect(screen.getByText(notice)).toBeDefined();
  // The values stay as received.
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  // The line is not in the status region: the badge announced the end.
  expect(screen.getByText(notice).closest('[role="status"]')).toBeNull();
});

test("a final result that arrives within 0.3 s shows no notice", () => {
  vi.useFakeTimers();
  const { rerender } = render(<Header />);
  pass(FINAL_RESULT_NOTICE_MS - 1);
  rerender(<Header scan={ended({ total: final })} total={final} />);
  expect(screen.getByText("Result version: Final")).toBeDefined();
  pass(5 * FINAL_RESULT_NOTICE_MS);
  expect(screen.queryByText(notice)).toBeNull();
});

test("restarting the same final result waits again after an interruption", () => {
  vi.useFakeTimers();
  const { rerender } = render(<Header />);
  pass(FINAL_RESULT_NOTICE_MS);
  expect(screen.getByText(notice)).toBeDefined();

  rerender(<Header holding />);
  expect(screen.queryByText(notice)).toBeNull();
  rerender(<Header />);
  expect(screen.queryByText(notice)).toBeNull();

  pass(FINAL_RESULT_NOTICE_MS - 1);
  expect(screen.queryByText(notice)).toBeNull();
  pass(1);
  expect(screen.getByText(notice)).toBeDefined();
});

test("the arriving final result replaces the notice and the result version at once", () => {
  vi.useFakeTimers();
  const { rerender } = render(<Header />);
  pass(FINAL_RESULT_NOTICE_MS);
  expect(screen.getByText(notice)).toBeDefined();
  rerender(<Header scan={ended({ total: final })} total={final} />);
  expect(screen.queryByText(notice)).toBeNull();
  expect(screen.getByText("Result version: Final")).toBeDefined();
  expect(screen.queryByText("Result version: Interim")).toBeNull();
});

test("a final result whose read failed keeps the existing handling", () => {
  vi.useFakeTimers();
  // The failed version waits for "Show latest result".
  const { rerender } = render(<Header scan={ended({ hasNewResult: true })} />);
  pass(5 * FINAL_RESULT_NOTICE_MS);
  expect(screen.queryByText(notice)).toBeNull();
  expect(
    screen.getByRole("button", { name: "Show latest result" }),
  ).toBeDefined();
  rerender(<Header scan={ended({ error: { kind: "totals_mismatch" } })} />);
  pass(5 * FINAL_RESULT_NOTICE_MS);
  expect(screen.queryByText(notice)).toBeNull();
});

test("a running scan and a held selection show no notice", () => {
  vi.useFakeTimers();
  const running = ended({
    running: true,
    status: { ...ended().status!, state: "running", resultVersion: null },
  });
  const { rerender } = render(<Header scan={running} />);
  pass(5 * FINAL_RESULT_NOTICE_MS);
  expect(screen.queryByText(notice)).toBeNull();
  expect(screen.getByText("Values update as the scan runs")).toBeDefined();
  rerender(<Header holding />);
  pass(5 * FINAL_RESULT_NOTICE_MS);
  expect(screen.queryByText(notice)).toBeNull();
});

test("the notice reads in Korean from the bundle", () => {
  vi.useFakeTimers();
  type Entry = [key: string, text: string, parameters: unknown[]];
  const bundle = JSON.parse(
    readFileSync(resolve(process.cwd(), "../user-env/i18n/ko.json"), "utf8"),
  ) as unknown[];
  const messages = new Map(
    (bundle.find(Array.isArray) as Entry[]).map(([key, value]) => [key, value]),
  );
  const korean: LocaleText = (key, fallback) => messages.get(key) ?? fallback;
  render(
    <LocaleTextContext.Provider value={korean}>
      <Header />
    </LocaleTextContext.Provider>,
  );
  pass(FINAL_RESULT_NOTICE_MS);
  expect(screen.getByText("최종 결과를 읽는 중…")).toBeDefined();
});

test("the panel shows the notice while it reads the final result and changes the values with the version", async () => {
  /** What each commit of the view shows: version line, notice, lead value. */
  const seen: string[] = [];
  const record = () => {
    const version = screen.queryByText(/^Result version: /)?.textContent;
    const value = screen
      .queryByRole("group", { name: "Logical size" })
      ?.querySelector(".du-metric__value")?.textContent;
    if (!version || !value) return;
    seen.push(
      [version, screen.queryByText(notice) ? "reading" : "-", value].join(
        " | ",
      ),
    );
  };
  const scan = await startPanel(record);
  passReflectInterval();
  scan.publish("v2");
  await logical().findByText("200 bytes");

  // The end is known at once; the final result is read at the next interval.
  scan.publish("v3", "done");
  await screen.findByText("Scan finished");
  await screen.findByText(notice, {}, { timeout: 3000 });
  expect(screen.getByText("Result version: Interim")).toBeDefined();
  expect(logical().getByText("200 bytes")).toBeDefined();

  await screen.findByText("Result version: Final", {}, { timeout: 3000 });
  expect(screen.queryByText(notice)).toBeNull();
  expect(logical().getByText("300 bytes")).toBeDefined();
  const fromNotice = seen.slice(
    seen.findIndex((commit) => commit.includes("reading")),
  );
  expect([...new Set(fromNotice)]).toEqual([
    "Result version: Interim | reading | 200 bytes",
    "Result version: Final | - | 300 bytes",
  ]);
});
