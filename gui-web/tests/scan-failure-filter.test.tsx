import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { useRef, useState } from "react";
import { afterEach, expect, test, vi } from "vitest";
import { ScanIssues } from "../src/features/analysis/ScanIssues";
import { ScanLaneProgress } from "../src/features/analysis/ScanLaneProgress";
import type { ScanIssueLaneSelection } from "../src/features/analysis/scan-issue-filter";
import type {
  AnalysisClient,
  OsPath,
  ScanFailures,
  ScanFailuresRequest,
  ScanLane,
} from "../src/ipc";
import { AnalysisError, windowsPath } from "../src/ipc";
import { LocaleTextContext } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import type { RegionalFormat } from "../src/i18n/regional-format";
import { englishText, koreanText } from "../preview/result-summary-fixture";

afterEach(cleanup);

const regional: RegionalFormat = {
  decimalSeparator: ".",
  groupingSeparator: ",",
  groupingSize: 3,
  datePattern: "yyyy.MM.dd HH:mm:ss",
  dateFormat: "lang_default",
  groupNumbers: true,
  timeZonePolicy: "utc",
  localTimeZoneStatus: "identified",
};

const roots = [
  windowsPath("C:\\Users\\minseo\\Documents"),
  windowsPath("D:\\Projects\\Media"),
  windowsPath("E:\\Archive"),
];

function lane(
  laneIndex: number,
  rootIndexes: number[],
  failures: string,
  rootCount?: string,
): ScanLane {
  return {
    laneIndex,
    rootIndexes,
    ...(rootCount && { rootCount }),
    inFlight: 1,
    limit: 2,
    waitReason: "none",
    waitConfirmed: true,
    remainingDirectories: "12",
    failures,
  };
}

const firstPage = (request: ScanFailuresRequest) =>
  ({
    resultVersion: request.resultVersion,
    totalCount: 2,
    rows: [
      {
        kind: "unread_range" as const,
        stage: "enumerate" as const,
        pathRef: { rootIndex: 0, relative: [] },
        reasonKey: "scan.path_denied",
        displayPath: "C:\\Users\\minseo\\Documents\\blocked",
      },
    ],
  }) satisfies ScanFailures;

function clientWith(
  read: (
    request: ScanFailuresRequest,
  ) => Promise<ScanFailures> | ScanFailures = firstPage,
) {
  const requests: ScanFailuresRequest[] = [];
  const client = {
    call: (async (name: string, body: ScanFailuresRequest) => {
      if (name !== "scan.failures") throw new Error("Unexpected " + name);
      requests.push(body);
      return read(body);
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  } satisfies AnalysisClient;
  return { client, requests };
}

function FilterHarness({
  client,
  lanes = [lane(0, [0, 1], "2"), lane(1, [2], "0")],
  laneRoots = roots,
  locale = "en",
  running = false,
  onReopenLatest = vi.fn(),
}: {
  client: AnalysisClient;
  lanes?: ScanLane[];
  laneRoots?: readonly OsPath[];
  locale?: "en" | "ko";
  running?: boolean;
  onReopenLatest?: () => void;
}) {
  const [request, setRequest] = useState<
    (ScanIssueLaneSelection & { requestId: number }) | undefined
  >();
  const requestId = useRef(0);
  const selectLane = (selection: ScanIssueLaneSelection) =>
    setRequest({ ...selection, requestId: ++requestId.current });
  const status = {
    sequence: 1,
    lanes,
    observedAt: "2026-11-20T09:00:00.000Z",
  };
  return (
    <LocaleTextContext.Provider
      value={locale === "ko" ? koreanText : englishText}
    >
      <RegionalFormatContext.Provider value={regional}>
        <ScanLaneProgress
          jobRef="scan"
          status={status}
          roots={laneRoots}
          onViewUnreadableLocations={selectLane}
        />
        <ScanIssues
          client={client}
          resultVersion="v1"
          onReopenLatest={onReopenLatest}
          running={running}
          laneRequest={request}
        />
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}

async function openLaneIssues(laneIndex = 0) {
  fireEvent.click(
    screen.getByRole("link", {
      name:
        laneIndex === 0
          ? "View 2 unreadable locations"
          : "View 0 unreadable locations",
    }),
  );
  await screen.findByRole("heading", { name: /Scan issues/ });
}

test("lane link requests unreadable rows for that lane and shows the filter names", async () => {
  const { client, requests } = clientWith();
  render(<FilterHarness client={client} />);

  expect(
    screen.queryByRole("link", { name: "View 0 unreadable locations" }),
  ).toBeNull();
  fireEvent.click(
    screen.getByRole("link", { name: "View 2 unreadable locations" }),
  );
  await screen.findByText("C:\\Users\\minseo\\Documents\\blocked");

  expect(requests).toEqual([
    { resultVersion: "v1", rootIndexes: [0, 1], failuresOnly: true },
  ]);
  expect(
    screen.getByRole("button", {
      name: "Storage: C:\\Users\\minseo\\Documents and 1 more, clear storage filter",
    }),
  ).toBeTruthy();
  expect(
    screen.getByRole("button", {
      name: "Kind: unreadable locations, clear kind filter",
    }),
  ).toBeTruthy();
  const heading = await screen.findByRole("heading", {
    name: /Scan issues.*Showing 1 of 2 issues/,
  });
  expect(document.activeElement).toBe(heading);
});

test("removing Storage keeps the unreadable-only filter and focuses Kind", async () => {
  const { client, requests } = clientWith();
  render(<FilterHarness client={client} />);
  await openLaneIssues();

  const storage = screen.getByRole("button", {
    name: "Storage: C:\\Users\\minseo\\Documents and 1 more, clear storage filter",
  });
  fireEvent.click(storage);
  const kind = screen.getByRole("button", {
    name: "Kind: unreadable locations, clear kind filter",
  });
  await waitFor(() => expect(document.activeElement).toBe(kind));
  await waitFor(() => expect(requests).toHaveLength(2));
  expect(requests[1]).toEqual({ resultVersion: "v1", failuresOnly: true });
});

test("removing Kind keeps all issues for Storage and focuses Storage", async () => {
  const { client, requests } = clientWith();
  render(<FilterHarness client={client} />);
  await openLaneIssues();

  const kind = screen.getByRole("button", {
    name: "Kind: unreadable locations, clear kind filter",
  });
  fireEvent.click(kind);
  const storage = screen.getByRole("button", {
    name: "Storage: C:\\Users\\minseo\\Documents and 1 more, clear storage filter",
  });
  await waitFor(() => expect(document.activeElement).toBe(storage));
  await waitFor(() => expect(requests).toHaveLength(2));
  expect(requests[1]).toEqual({ resultVersion: "v1", rootIndexes: [0, 1] });
});

test("clearing the last condition requests all issues and focuses the title", async () => {
  const { client, requests } = clientWith();
  render(<FilterHarness client={client} />);
  await openLaneIssues();
  fireEvent.click(
    screen.getByRole("button", {
      name: "Storage: C:\\Users\\minseo\\Documents and 1 more, clear storage filter",
    }),
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "Kind: unreadable locations, clear kind filter",
    }),
  );

  await waitFor(() => expect(requests).toHaveLength(3));
  const heading = screen.getByRole("heading", { name: /Scan issues/ });
  await waitFor(() => expect(document.activeElement).toBe(heading));
  expect(requests[2]).toEqual({ resultVersion: "v1" });
});

test("a cut lane opens with only the unreadable filter and the all-storage notice", async () => {
  const { client, requests } = clientWith();
  const roots64 = Array.from({ length: 64 }, (_, index) =>
    windowsPath(`C:\\root-${index}`),
  );
  const cutLane = lane(
    0,
    Array.from({ length: 64 }, (_, index) => index),
    "3",
    "65",
  );
  render(
    <FilterHarness
      client={client}
      lanes={[cutLane, lane(1, [0], "0")]}
      laneRoots={roots64}
      locale="ko"
    />,
  );
  fireEvent.click(
    screen.getByRole("link", { name: "읽지 못한 위치 3곳 보기" }),
  );
  await screen.findByText(
    "이 저장소만 거를 수 없어 모든 저장소의 읽지 못한 위치를 보입니다",
  );
  expect(requests[0]).toEqual({ resultVersion: "v1", failuresOnly: true });
  expect(
    screen.getByRole("button", {
      name: "종류: 읽지 못한 위치, 종류 조건 지우기",
    }),
  ).toBeTruthy();
  expect(screen.queryByRole("button", { name: /저장소:/ })).toBeNull();
});

test.each(["STALE_VERSION", "cursor.stale", "cursor.invalid"] as const)(
  "a %s continuation restarts at page one with the same filters",
  async (reason) => {
    let continuation = true;
    const { client, requests } = clientWith((request) => {
      if (request.cursor && continuation) {
        continuation = false;
        throw new AnalysisError(
          reason === "cursor.invalid" ? "INVALID_ARGUMENT" : "STALE_VERSION",
          reason,
        );
      }
      return {
        ...firstPage(request),
        ...(!request.cursor && { nextCursor: "next" }),
      };
    });
    render(<FilterHarness client={client} />);
    fireEvent.click(
      screen.getByRole("link", { name: "View 2 unreadable locations" }),
    );
    await screen.findByRole("button", { name: "Load more issues" });
    fireEvent.click(screen.getByRole("button", { name: "Load more issues" }));

    await waitFor(() => expect(requests).toHaveLength(3));
    expect(requests[1]).toEqual({
      resultVersion: "v1",
      rootIndexes: [0, 1],
      failuresOnly: true,
      cursor: "next",
    });
    expect(requests[2]).toEqual({
      resultVersion: "v1",
      rootIndexes: [0, 1],
      failuresOnly: true,
    });
  },
);

test("a running scan follows a stale result without showing the reopen notice", async () => {
  let version = "v1";
  const requests: ScanFailuresRequest[] = [];
  const client = {
    call: (async (name: string, request: ScanFailuresRequest) => {
      if (name !== "scan.failures") throw new Error("Unexpected " + name);
      requests.push(request);
      if (request.resultVersion === "v1")
        throw new AnalysisError("STALE_VERSION", "result.requery_required");
      return firstPage(request);
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  } satisfies AnalysisClient;
  const reopen = vi.fn(() => {
    version = "v2";
  });
  function RunningIssues({
    laneRequest,
  }: {
    laneRequest: ScanIssueLaneSelection & { requestId: number };
  }) {
    const [current, setCurrent] = useState(version);
    return (
      <ScanIssues
        client={client}
        resultVersion={current}
        running
        onReopenLatest={() => {
          reopen();
          setCurrent(version);
        }}
        laneRequest={laneRequest}
      />
    );
  }
  const laneRequest = {
    rootIndexes: [0],
    storagePath: roots[0]!.display,
    storageLabel: roots[0]!.display,
    storagePaths: [roots[0]!.display],
    allStorages: false,
    requestId: 1,
  };
  render(
    <LocaleTextContext.Provider value={englishText}>
      <RegionalFormatContext.Provider value={regional}>
        <RunningIssues laneRequest={laneRequest} />
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>,
  );

  await screen.findByText("C:\\Users\\minseo\\Documents\\blocked");
  expect(reopen).toHaveBeenCalledTimes(1);
  expect(
    screen.queryByRole("button", { name: "Reopen with the new result" }),
  ).toBeNull();
  expect(requests).toEqual([
    { resultVersion: "v1", rootIndexes: [0], failuresOnly: true },
    { resultVersion: "v2", rootIndexes: [0], failuresOnly: true },
  ]);
});

test("the Korean lane link and filter chips use the approved accessible names", async () => {
  const { client } = clientWith();
  render(<FilterHarness client={client} locale="ko" />);
  fireEvent.click(
    screen.getByRole("link", { name: "읽지 못한 위치 2곳 보기" }),
  );
  await screen.findByRole("button", {
    name: "저장소: C:\\Users\\minseo\\Documents 외 1개, 저장소 조건 지우기",
  });
  expect(
    screen.getByRole("button", {
      name: "종류: 읽지 못한 위치, 종류 조건 지우기",
    }),
  ).toBeTruthy();
});
