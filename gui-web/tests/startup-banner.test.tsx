import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import axe from "axe-core";
import { afterEach, expect, test, vi } from "vitest";
import { ApplicationEntry } from "../src/app/ApplicationEntry";
import { DesktopLocale } from "../src/app/DesktopLocale";
import { InfoEntry } from "../src/app/InfoEntry";
import { StartupGate } from "../src/features/startup/StartupGate";
import {
  reviewKey,
  STATE_KEY_LIMIT,
  startupMemory,
} from "../src/features/startup/startup-memory";
import { LocaleTextContext } from "../src/i18n/presentation";
import { createAnalysisClient } from "../src/ipc";
import { supportReport } from "./info-host";
import type {
  AnalysisClient,
  LocaleBundleBody,
  LocaleProfile,
  StartupAccessState,
  StartupCheckResult,
  StartupGrade,
  StartupStatus,
} from "../src/ipc";

const bridge = vi.hoisted(() => ({ available: false }));

vi.mock("../src/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../src/ipc")>()),
  hasWindowsAnalysisBridge: () => bridge.available,
}));
vi.mock("../src/app/App", async () => {
  const { InfoEntry } = await import("../src/app/InfoEntry");
  return {
    App: ({
      analysis,
      infoClient,
    }: {
      analysis?: AnalysisClient;
      infoClient?: AnalysisClient;
    }) => (
      <div>
        {analysis ? "Connected analysis" : "Shell without analysis"}
        <InfoEntry client={analysis} fallbackClient={infoClient} />
      </div>
    ),
  };
});

afterEach(() => {
  cleanup();
  startupMemory.reset();
  bridge.available = false;
  document.documentElement.lang = "en";
});

function report(
  grade: StartupGrade,
  checks: StartupCheckResult[],
  access: StartupAccessState = "not_applicable",
  analysisAllowed = access !== "denied",
): StartupStatus {
  return { grade, checks, firstRun: false, access, analysisAllowed };
}

const outsideVerified = report("outside_verified", [
  { check: "webview", result: "pass", reasonKey: null },
  {
    check: "support_matrix",
    result: "outside_verified",
    reasonKey: "support.outside_verified",
  },
  { check: "capability", result: "limited", reasonKey: null },
  { check: "full_disk_access", result: "not_applicable", reasonKey: null },
]);

/** macOS could not confirm Full Disk Access; analysis is still allowed. */
function accessUnknown(access: StartupAccessState = "unknown") {
  return report(
    "pass",
    [
      { check: "webview", result: "pass", reasonKey: null },
      {
        check: "full_disk_access",
        result: access === "unknown" ? "unverifiable" : "pass",
        reasonKey: null,
      },
    ],
    access,
  );
}

type Transport = NonNullable<Parameters<typeof createAnalysisClient>[0]>;

function clientFor(
  reply: (name: string, body: unknown) => unknown,
  calls: string[] = [],
) {
  const transport: Transport = async (_, { envelope }) => {
    calls.push(envelope.name);
    const result = reply(envelope.name, envelope.body);
    if (result instanceof Error) throw result;
    return { ...envelope, body: result };
  };
  return createAnalysisClient(transport);
}

function FirstScreen() {
  return (
    <main>
      <h1>First screen</h1>
    </main>
  );
}

test("a grade alone puts nothing above the first screen", async () => {
  const calls: string[] = [];
  render(
    <StartupGate client={clientFor(() => outsideVerified, calls)}>
      <FirstScreen />
    </StartupGate>,
  );
  await waitFor(() => expect(startupMemory.get().status).toBeDefined());
  expect(calls).toEqual(["app.startup_status"]);
  expect(screen.queryByRole("region")).toBeNull();
  expect(screen.getByRole("status").textContent).toBe("");
  expect(screen.getByRole("heading", { name: "First screen" })).toBeTruthy();
});

test("unconfirmed Full Disk Access shows one line with its action, and dismissing remembers the state", async () => {
  let status = accessUnknown();
  const client = clientFor(() => status);
  const first = render(
    <StartupGate client={client}>
      <FirstScreen />
    </StartupGate>,
  );
  const notice = await screen.findByRole("region", { name: "Startup notice" });
  expect(within(notice).getByRole("status").textContent).toBe(
    "Full Disk Access couldn't be confirmed, so some folders may be missing from results.",
  );
  expect(
    within(notice).getByRole("button", { name: "Open Settings" }),
  ).toBeTruthy();
  // No table of checks in the line; support information carries it.
  expect(within(notice).queryByRole("term")).toBeNull();
  expect(
    notice.compareDocumentPosition(screen.getByRole("main")) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();

  fireEvent.click(
    within(notice).getByRole("button", { name: "Dismiss notice" }),
  );
  expect(screen.queryByRole("region")).toBeNull();
  expect(document.activeElement).toBe(
    screen.getByRole("heading", { name: "First screen" }),
  );
  first.unmount();

  // The same state again stays quiet.
  const second = render(
    <StartupGate client={client}>
      <FirstScreen />
    </StartupGate>,
  );
  await waitFor(() => expect(startupMemory.get().dismissed).not.toBe(""));
  expect(screen.queryByRole("region")).toBeNull();
  second.unmount();

  // Confirmed, then unconfirmed again: the notice returns.
  status = accessUnknown("confirmed");
  const third = render(
    <StartupGate client={client}>
      <FirstScreen />
    </StartupGate>,
  );
  await waitFor(() => expect(startupMemory.get().dismissed).toBe(""));
  third.unmount();
  status = accessUnknown();
  render(
    <StartupGate client={client}>
      <FirstScreen />
    </StartupGate>,
  );
  expect(
    await screen.findByRole("region", { name: "Startup notice" }),
  ).toBeTruthy();
});

test("Open Settings announces the result once and reads the status again", async () => {
  let status = accessUnknown();
  const calls: string[] = [];
  const requests: unknown[] = [];
  const client = clientFor((name, body) => {
    if (name === "app.startup_access") {
      requests.push(body);
      status = accessUnknown("confirmed");
      return { settingsOpened: true };
    }
    return status;
  }, calls);
  render(
    <StartupGate client={client}>
      <FirstScreen />
    </StartupGate>,
  );
  const notice = await screen.findByRole("region", { name: "Startup notice" });
  const live = within(notice).getByRole("status");
  fireEvent.click(
    within(notice).getByRole("button", { name: "Open Settings" }),
  );
  await waitFor(() => expect(screen.queryByRole("region")).toBeNull());
  expect(requests).toEqual([{ action: "open_privacy_settings" }]);
  expect(calls.filter((name) => name === "app.startup_status")).toHaveLength(2);
  // The status region stays in the page.
  expect(screen.getByRole("status")).toBe(live);
});

test("returning to the window checks the access again", async () => {
  let status = accessUnknown();
  const requests: unknown[] = [];
  const client = clientFor((name, body) => {
    if (name === "app.startup_access") {
      requests.push(body);
      return { settingsOpened: false };
    }
    return status;
  });
  render(
    <StartupGate client={client}>
      <FirstScreen />
    </StartupGate>,
  );
  await screen.findByRole("region", { name: "Startup notice" });
  status = accessUnknown("confirmed");
  fireEvent.focus(window);
  await waitFor(() => expect(screen.queryByRole("region")).toBeNull());
  expect(requests).toEqual([{ action: "recheck" }]);
});

test("a passing grade or an unreadable status shows nothing", async () => {
  const calls: string[] = [];
  const first = render(
    <StartupGate
      client={clientFor(
        () =>
          report("pass", [
            { check: "webview", result: "pass", reasonKey: null },
          ]),
        calls,
      )}
    >
      <FirstScreen />
    </StartupGate>,
  );
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(screen.queryByRole("region")).toBeNull();
  first.unmount();

  render(
    <StartupGate client={clientFor(() => new Error("unavailable"), calls)}>
      <FirstScreen />
    </StartupGate>,
  );
  await waitFor(() => expect(calls).toHaveLength(2));
  expect(screen.getByRole("heading", { name: "First screen" })).toBeTruthy();
  expect(screen.queryByRole("region")).toBeNull();
});

test("an analysis block replaces the first screen with the permission screen", async () => {
  render(
    <StartupGate
      client={clientFor(() =>
        report(
          "unverifiable",
          [
            {
              check: "full_disk_access",
              result: "blocked",
              reasonKey: "startup.full_disk_access_required",
            },
          ],
          "denied",
        ),
      )}
    >
      <FirstScreen />
    </StartupGate>,
  );
  expect(
    await screen.findByRole("heading", { name: "Startup and privacy" }),
  ).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "First screen" })).toBeNull();
});

test("notice text follows the injected locale text", async () => {
  render(
    <LocaleTextContext.Provider
      value={(key, fallback) =>
        ({
          "ui.startup.banner_heading": "시작 안내",
          "ui.startup.notice_full_disk_access":
            "전체 디스크 접근을 확인하지 못해 일부 폴더가 결과에서 빠질 수 있습니다.",
          "ui.startup.notice_open_settings": "설정 열기",
          "ui.startup.notice_dismiss": "알림 닫기",
        })[key] ?? fallback
      }
    >
      <StartupGate client={clientFor(() => accessUnknown())}>
        <FirstScreen />
      </StartupGate>
    </LocaleTextContext.Provider>,
  );
  const notice = await screen.findByRole("region", { name: "시작 안내" });
  expect(
    within(notice).getByText(/전체 디스크 접근을 확인하지 못해/),
  ).toBeTruthy();
  expect(
    within(notice).getByRole("button", { name: "설정 열기" }),
  ).toBeTruthy();
  expect(
    within(notice).getByRole("button", { name: "알림 닫기" }),
  ).toBeTruthy();
});

test("the notice holds only its action and the dismiss button", async () => {
  render(
    <StartupGate client={clientFor(() => accessUnknown())}>
      <FirstScreen />
    </StartupGate>,
  );
  const notice = await screen.findByRole("region", { name: "Startup notice" });
  // The dot beside "Support information" is the way to the details.
  expect(
    within(notice)
      .getAllByRole("button")
      .map((button) => button.getAttribute("aria-label") ?? button.textContent),
  ).toEqual(["Open Settings", "Dismiss notice"]);
});

test("the status region exists before the notice text and receives it as a change", async () => {
  let release: (status: StartupStatus) => void = () => undefined;
  const pending = new Promise<StartupStatus>((resolve) => {
    release = resolve;
  });
  const transport: Transport = async (_, { envelope }) => ({
    ...envelope,
    body: await pending,
  });
  const { container } = render(
    <StartupGate client={createAnalysisClient(transport)}>
      <FirstScreen />
    </StartupGate>,
  );
  const live = screen.getByRole("status");
  expect(live.textContent).toBe("");
  expect((await axe.run(container)).violations).toEqual([]);

  release(accessUnknown());
  const notice = await screen.findByRole("region", { name: "Startup notice" });
  expect(screen.getAllByRole("status")).toEqual([live]);
  expect(live.textContent).toMatch(/Full Disk Access couldn't be confirmed/);
  expect(notice.contains(live)).toBe(true);
  expect((await axe.run(container)).violations).toEqual([]);

  fireEvent.click(
    within(notice).getByRole("button", { name: "Dismiss notice" }),
  );
  expect(screen.getByRole("status")).toBe(live);
  expect(live.textContent).toBe("");
});

test("support information leads with the startup checks", async () => {
  startupMemory.setStatus(outsideVerified);
  render(
    <InfoEntry
      client={clientFor((name) =>
        name === "app.support_info" ? supportReport() : {},
      )}
    />,
  );
  fireEvent.click(
    screen.getByRole("button", {
      name: "Support information, items to review",
    }),
  );
  const dialog = await screen.findByRole("dialog", {
    name: "Support information",
  });
  const checks = within(dialog).getByRole("region", { name: "Startup checks" });
  expect(
    within(checks).getByText("Checked when DiskUnfold started"),
  ).toBeTruthy();
  expect(within(checks).getByText("Untested combination")).toBeTruthy();
  expect(within(checks).getByText("Tested environment")).toBeTruthy();
  expect(within(checks).getByText("Platform capabilities")).toBeTruthy();
  expect(within(checks).queryByText("WebView availability")).toBeNull();
  // A short summary; the long explanation stays in the sections below.
  expect(
    within(checks).getByText(
      "This combination hasn't been tested. This doesn't mean there's a problem.",
    ),
  ).toBeTruthy();
  // The section comes before the support table sections.
  await within(dialog).findByRole("region", { name: "Combination status" });
  expect(
    checks.compareDocumentPosition(
      within(dialog).getByRole("region", { name: "Combination status" }),
    ) & Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();

  fireEvent.click(
    within(checks).getByRole("button", { name: /Show all checks/ }),
  );
  // Not macOS: the Full Disk Access row is left out.
  expect(within(checks).getByText("WebView availability")).toBeTruthy();
  expect(within(checks).queryByText("Full Disk Access")).toBeNull();

  fireEvent.click(
    within(checks).getByRole("button", { name: "Show combination status" }),
  );
  expect(document.activeElement).toBe(
    within(dialog).getByRole("heading", { name: "Combination status" }),
  );
  expect(
    within(dialog).getByRole("button", { name: "Refresh support information" }),
  ).toBeTruthy();
  expect((await axe.run(dialog)).violations).toEqual([]);

  // Opening the screen marks this state as seen.
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Close details" }),
  );
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  expect(
    screen.getByRole("button", { name: "Support information" }),
  ).toBeTruthy();
});

test("a state key keeps to the stored grammar and size", () => {
  const names = [
    "resource_integrity",
    "webview",
    "data_format_and_open",
    "temporary_files",
    "support_matrix",
    "capability",
    "full_disk_access",
  ];
  const key = reviewKey(
    report(
      "outside_verified",
      names.map((check) => ({
        check,
        result: "outside_verified",
        reasonKey: null,
      })),
    ),
  );
  expect(key.length).toBeLessThanOrEqual(STATE_KEY_LIMIT);
  expect(key).toMatch(/^grade:outside_verified(\|[a-z0-9_.:]+)+$/);
  // Rows are sorted by name, so the same state always gives the same key.
  expect(key.split("|")[1]).toBe("capability:outside_verified");
});

test("a passing state shows no dot and a changed state shows it again", async () => {
  const pass = report("pass", [
    { check: "webview", result: "pass", reasonKey: null },
  ]);
  startupMemory.setStatus(pass);
  render(<InfoEntry client={clientFor(() => supportReport())} />);
  const entry = screen.getByRole("navigation");
  expect(entry.hasAttribute("data-review")).toBe(false);

  startupMemory.setStatus(outsideVerified);
  await waitFor(() => expect(entry.hasAttribute("data-review")).toBe(true));
  startupMemory.markSeen();
  await waitFor(() => expect(entry.hasAttribute("data-review")).toBe(false));
  startupMemory.setStatus(
    report("unverifiable", [
      {
        check: "support_matrix",
        result: "unverifiable",
        reasonKey: "support.matrix_unavailable",
      },
    ]),
  );
  await waitFor(() => expect(entry.hasAttribute("data-review")).toBe(true));
});

const languages = [
  {
    language: "en",
    selfNameKey: "locale.language.en",
    bundleRevision: "en-rev",
  },
  {
    language: "ko",
    selfNameKey: "locale.language.ko",
    bundleRevision: "ko-rev",
  },
];

const profile: LocaleProfile = {
  displayLanguage: "ko",
  languageSource: "stored",
  dateFormat: "lang_default",
  numberFormat: "lang_default",
  timeZonePolicy: "local",
  localTimeZoneStatus: "offset_only",
  motion: "follow_os",
  appScale: 100,
  profileRevision: "3",
  supportedLanguages: languages,
};

function bundle(language: string): LocaleBundleBody {
  const text = (template: string) => ({
    kind: "text" as const,
    template,
    parameters: {},
  });
  const messages: LocaleBundleBody["messages"] = {
    "ui.common.translation_unverified": text("Translation unverified"),
    "locale.language.en": text("English"),
    "locale.language.ko": text("한국어"),
  };
  if (language === "ko") {
    messages["ui.info.support_open"] = text("지원 정보");
    messages["ui.info.support_open_review"] =
      text("지원 정보, 확인할 항목 있음");
    messages["ui.startup.checks_section"] = text("시작 점검");
    messages["ui.startup.check.support_matrix"] = text("테스트한 환경");
    messages["ui.common.close_details"] = text("상세 닫기");
  }
  return {
    messages,
    formatTable: {
      decimalSeparator: ".",
      groupingSeparator: ",",
      groupingSize: 3,
      datePatterns: {
        lang_default: "yyyy.MM.dd HH:mm:ss",
        iso8601: "yyyy-MM-dd'T'HH:mm:ss",
      },
      unitSymbols: {},
    },
    pluralRule: language,
    reasonMessages: {},
    metadataRevision: "fixture",
  };
}

function windowsClient(calls: string[]) {
  return createAnalysisClient(async (_command, { envelope }) => {
    calls.push(envelope.name);
    const response = {
      name: envelope.name,
      correlationId: envelope.correlationId,
      contractVersion: 1,
    };
    switch (envelope.name) {
      case "i18n.get_profile":
        return { ...response, body: profile };
      case "i18n.get_bundle": {
        const { language } = envelope.body as { language: string };
        return {
          ...response,
          body: {
            bundleRevision: language === "en" ? "en-rev" : "ko-rev",
            bundle: bundle(language),
          },
        };
      }
      case "app.startup_status":
        return { ...response, body: outsideVerified };
      case "app.support_info":
        return { ...response, body: supportReport() };
      default:
        return { ...response, body: {} };
    }
  });
}

test("the Windows composition keeps the analysis shell and puts the dot on support information", async () => {
  bridge.available = true;
  const calls: string[] = [];
  render(
    <ApplicationEntry
      client={windowsClient(calls)}
      localeEnabled
      startupStatusEnabled
      analysisEnabled
    />,
  );

  expect(await screen.findByText("Connected analysis")).toBeTruthy();
  const entry = await screen.findByRole("button", {
    name: "지원 정보, 확인할 항목 있음",
  });
  expect(screen.queryByRole("region")).toBeNull();
  expect(
    screen.queryByRole("heading", { name: "Startup and privacy" }),
  ).toBeNull();
  expect(calls.filter((name) => name === "app.startup_status")).toHaveLength(1);

  entry.focus();
  fireEvent.click(entry);
  const dialog = await screen.findByRole("dialog", { name: "지원 정보" });
  const checks = within(dialog).getByRole("region", { name: "시작 점검" });
  expect(within(checks).getByText("테스트한 환경")).toBeTruthy();
  expect(await within(dialog).findByText("sm-7")).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "상세 닫기" }));
  await waitFor(() => expect(screen.queryByRole("dialog")).toBeNull());
  await waitFor(() =>
    expect(document.activeElement).toBe(
      screen.getByRole("button", { name: "지원 정보" }),
    ),
  );
});

test("the shell without the startup bridge makes no startup request", async () => {
  const calls: string[] = [];
  render(
    <DesktopLocale client={windowsClient(calls)} analysisEnabled={false} />,
  );
  await screen.findByText("Shell without analysis");
  expect(calls).not.toContain("app.startup_status");
  expect(screen.queryByRole("region")).toBeNull();
});
