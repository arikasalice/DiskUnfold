import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import { LocaleTextContext } from "../src/i18n/presentation";
import type { OsPath, ScanLane, ScanLaneStatus } from "../src/ipc";
import {
  ScanLaneProgress,
  createScanLaneThrottle,
} from "../src/features/analysis/ScanLaneProgress";
import { koreanText } from "../preview/result-summary-fixture";
import { windowsPath } from "../src/ipc";

const regional = {
  decimalSeparator: ".",
  groupingSeparator: ",",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "lang_default" as const,
  groupNumbers: true,
  timeZonePolicy: "utc" as const,
  localTimeZoneStatus: "identified" as const,
};
const roots = [0, 1, 2, 3].map((index) =>
  windowsPath(`C:\\fixture\\root-${index}`),
);

function lane(laneIndex: number, patch: Partial<ScanLane> = {}): ScanLane {
  return {
    laneIndex,
    rootIndexes: [laneIndex],
    inFlight: 1,
    limit: 4,
    waitReason: "none",
    waitConfirmed: true,
    remainingDirectories: "1234",
    failures: "0",
    ...patch,
  };
}

function status(
  sequence: number,
  lanes: ScanLane[],
  laneCount?: string,
): ScanLaneStatus {
  return {
    sequence,
    lanes,
    observedAt: "2026-11-20T09:00:00.000Z",
    ...(laneCount === undefined ? {} : { laneCount }),
  };
}

function renderProgress(
  value: ScanLaneStatus,
  laneRoots: readonly OsPath[] = roots,
) {
  return render(
    <LocaleTextContext.Provider value={koreanText}>
      <RegionalFormatContext.Provider value={regional}>
        <ScanLaneProgress jobRef="scan" status={value} roots={laneRoots} />
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>,
  );
}

afterEach(() => {
  cleanup();
  vi.useRealTimers();
});

test("shows each confirmed wait cause and uses the same trailing progress ellipsis", () => {
  renderProgress(
    status(1, [
      lane(0),
      lane(1, { waitReason: "io_wait" }),
      lane(2, { waitReason: "backpressure" }),
      lane(3, { waitReason: "device_cap" }),
    ]),
  );
  expect(screen.getByText("읽는 중…")).toBeTruthy();
  expect(screen.getByText("디스크 응답을 기다리는 중…")).toBeTruthy();
  expect(screen.getByText("처리가 밀려 기다리는 중…")).toBeTruthy();
  expect(screen.getByText("같은 장치의 다른 읽기를 기다리는 중…")).toBeTruthy();
});

test("unknown and unconfirmed wait causes show only the general waiting text", () => {
  const unknown = lane(0, {
    waitReason: "future_wait" as ScanLane["waitReason"],
  });
  const unconfirmed = lane(1, {
    waitReason: "io_wait",
    waitConfirmed: false,
  });
  renderProgress(status(1, [unknown, unconfirmed]));
  expect(screen.getAllByText("기다리는 중…")).toHaveLength(2);
  const table = screen.getByRole("table", { name: "저장소별 진행" });
  const rows = within(table).getAllByRole("row");
  fireEvent.click(within(rows[2]!).getByRole("button"));
  expect(screen.getByText("대기 원인 · 확인 불가")).toBeTruthy();
});

test("shows Done for an idle lane and leaves zero failures empty", () => {
  renderProgress(
    status(1, [
      lane(0, { remainingDirectories: "0", inFlight: 0 }),
      lane(1, { failures: "0" }),
      lane(2, { remainingDirectories: "0", inFlight: 1 }),
    ]),
  );
  expect(screen.getByText("끝남")).toBeTruthy();
  const rows = within(screen.getByRole("table")).getAllByRole("row");
  expect(rows[2]!.querySelectorAll("td")[3]?.textContent).toBe("");
  expect(rows[3]!.querySelectorAll("td")[2]?.textContent).toBe("0개");
});

test("shows one lane as a status line and multiple lanes as a table", () => {
  const single = renderProgress(
    status(1, [lane(0, { waitReason: "io_wait" })]),
  );
  expect(screen.getByText("디스크 응답을 기다리는 중…")).toBeTruthy();
  expect(screen.queryByRole("table")).toBeNull();
  single.unmount();
  renderProgress(status(2, [lane(0), lane(1, { waitReason: "io_wait" })]));
  const table = screen.getByRole("table", { name: "저장소별 진행" });
  expect(within(table).getAllByRole("row")).toHaveLength(3);
  expect(within(table).getByText("남은 폴더")).toBeTruthy();
  expect(within(table).getByText("읽지 못한 위치")).toBeTruthy();
});

test("shows failures as plain text and uses rootCount when paths are cut", () => {
  const longRoots = Array.from({ length: 64 }, (_, index) =>
    windowsPath(`C:\\fixture\\root-${index}`),
  );
  const longLane = lane(0, {
    rootIndexes: Array.from({ length: 64 }, (_, index) => index),
    rootCount: "65",
    failures: "2000",
  });
  renderProgress(status(1, [longLane, lane(1)]), longRoots);
  const table = screen.getByRole("table");
  const row = within(table).getAllByRole("row")[1]!;
  const pathDisclosure = within(row).getByRole("button", { name: /외 64개/ });
  expect(pathDisclosure).toBeTruthy();
  const failures = within(row).getByText("2,000");
  expect(failures.closest("button")).toBeNull();
});

test("does not put live announcements on changing rows", () => {
  const { container } = renderProgress(status(1, [lane(0), lane(1)]));
  const table = screen.getByRole("table");
  expect(table.closest('[role="status"], [aria-live]')).toBeNull();
  expect(
    container.querySelectorAll("tr[aria-live], td[aria-live]"),
  ).toHaveLength(0);
});

test("throttles each row to one update per second and keeps its first order", async () => {
  vi.useFakeTimers();
  const receive = vi.fn();
  const throttle = createScanLaneThrottle(receive);
  throttle.update(status(1, [lane(1), lane(0)]));
  expect(
    receive.mock.lastCall?.[0].map((item: ScanLane) => item.laneIndex),
  ).toEqual([1, 0]);

  throttle.update(
    status(2, [
      lane(0, { remainingDirectories: "20" }),
      lane(1, { remainingDirectories: "30" }),
    ]),
  );
  await act(async () => vi.advanceTimersByTimeAsync(500));
  throttle.update(
    status(3, [
      lane(1, { remainingDirectories: "40" }),
      lane(0, { remainingDirectories: "50" }),
    ]),
  );
  expect(
    receive.mock.lastCall?.[0].map(
      (item: ScanLane) => item.remainingDirectories,
    ),
  ).toEqual(["1234", "1234"]);
  await act(async () => vi.advanceTimersByTimeAsync(500));
  expect(
    receive.mock.lastCall?.[0].map((item: ScanLane) => item.laneIndex),
  ).toEqual([1, 0]);
  expect(
    receive.mock.lastCall?.[0].map(
      (item: ScanLane) => item.remainingDirectories,
    ),
  ).toEqual(["40", "50"]);

  throttle.update(
    status(4, [
      lane(0, { remainingDirectories: "60" }),
      lane(1, { remainingDirectories: "70" }),
    ]),
  );
  await act(async () => vi.advanceTimersByTimeAsync(999));
  expect(
    receive.mock.lastCall?.[0].map(
      (item: ScanLane) => item.remainingDirectories,
    ),
  ).toEqual(["40", "50"]);
  await act(async () => vi.advanceTimersByTimeAsync(1));
  expect(
    receive.mock.lastCall?.[0].map(
      (item: ScanLane) => item.remainingDirectories,
    ),
  ).toEqual(["70", "60"]);
  throttle.dispose();
});

test("announces the transition to idle once instead of announcing row updates", async () => {
  vi.useFakeTimers();
  const view = renderProgress(status(1, [lane(0, { inFlight: 1 })]));
  expect(screen.queryByRole("status")).toBeNull();
  view.rerender(
    <LocaleTextContext.Provider value={koreanText}>
      <RegionalFormatContext.Provider value={regional}>
        <ScanLaneProgress
          jobRef="scan"
          status={status(2, [lane(0, { inFlight: 0 })])}
          roots={roots}
        />
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>,
  );
  await act(async () => vi.advanceTimersByTimeAsync(1_000));
  expect(screen.getByRole("status").textContent).toContain(
    "모든 저장소 읽기가 멈췄습니다.",
  );
  view.rerender(
    <LocaleTextContext.Provider value={koreanText}>
      <RegionalFormatContext.Provider value={regional}>
        <ScanLaneProgress
          jobRef="scan"
          status={status(3, [lane(0, { inFlight: 0 })])}
          roots={roots}
        />
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>,
  );
  await act(async () => vi.advanceTimersByTimeAsync(1_000));
  expect(screen.getAllByRole("status")).toHaveLength(1);
});
