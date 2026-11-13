import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ApplicationEntry } from "../src/app/ApplicationEntry";
import { DesktopLocale } from "../src/app/DesktopLocale";
import { StartupGate } from "../src/features/startup/StartupGate";
import { LocaleTextContext } from "../src/i18n/presentation";
import { createAnalysisClient } from "../src/ipc";
import type {
  LocaleBundleBody,
  LocaleProfile,
  StartupCheckResult,
  StartupGrade,
  StartupStatus,
} from "../src/ipc";

const bridge = vi.hoisted(() => ({ available: false }));

vi.mock("../src/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../src/ipc")>()),
  hasWindowsAnalysisBridge: () => bridge.available,
}));
vi.mock("../src/app/App", () => ({
  App: ({ analysis }: { analysis?: unknown }) => (
    <div>{analysis ? "Connected analysis" : "Shell without analysis"}</div>
  ),
}));

afterEach(() => {
  cleanup();
  bridge.available = false;
  document.documentElement.lang = "en";
});

function report(
  grade: StartupGrade,
  checks: StartupCheckResult[],
  analysisAllowed = true,
): StartupStatus {
  return {
    grade,
    checks,
    firstRun: false,
    access: analysisAllowed ? "not_applicable" : "denied",
    analysisAllowed,
  };
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

type Transport = NonNullable<Parameters<typeof createAnalysisClient>[0]>;

function clientFor(
  reply: (name: string) => StartupStatus | Error,
  calls: string[] = [],
) {
  const transport: Transport = async (_, { envelope }) => {
    calls.push(envelope.name);
    const result = reply(envelope.name);
    if (result instanceof Error) throw result;
    return { ...envelope, body: result };
  };
  return createAnalysisClient(transport);
}

test("shows a dismissible startup banner above the first screen without blocking it", async () => {
  const calls: string[] = [];
  render(
    <StartupGate client={clientFor(() => outsideVerified, calls)}>
      <p>First screen</p>
    </StartupGate>,
  );

  expect(screen.getByText("First screen")).toBeTruthy();
  const banner = await screen.findByRole("region", { name: "Startup notice" });
  expect(calls).toEqual(["app.startup_status"]);
  expect(
    within(banner).getByText(
      /not one of the combinations DiskUnfold has verified/,
    ),
  ).toBeTruthy();
  expect(within(banner).getByText("Verified environment")).toBeTruthy();
  expect(within(banner).getByText("Platform capabilities")).toBeTruthy();
  expect(within(banner).queryByText("WebView availability")).toBeNull();
  expect(
    banner.compareDocumentPosition(screen.getByText("First screen")) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();

  fireEvent.click(within(banner).getByRole("button", { name: "Dismiss" }));
  expect(screen.queryByRole("region", { name: "Startup notice" })).toBeNull();
  expect(screen.getByText("First screen")).toBeTruthy();
  // The dismissed button's removal must not drop keyboard focus to the page.
  expect(document.activeElement).toBe(screen.getByText("First screen"));
});

test.each([
  ["limited", /features are limited/],
  ["unverifiable", /could not be verified/],
  ["unknown", /does not recognize/],
] as const)("explains the %s grade", async (grade, message) => {
  render(
    <StartupGate
      client={clientFor(() =>
        report(grade, [
          { check: "capability", result: "unverifiable", reasonKey: null },
        ]),
      )}
    >
      <p>First screen</p>
    </StartupGate>,
  );
  expect(await screen.findByText(message)).toBeTruthy();
});

test("a passing grade or an unreadable status shows no banner", async () => {
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
      <p>First screen</p>
    </StartupGate>,
  );
  await waitFor(() => expect(calls).toHaveLength(1));
  expect(screen.queryByRole("region")).toBeNull();
  first.unmount();

  render(
    <StartupGate client={clientFor(() => new Error("unavailable"), calls)}>
      <p>First screen</p>
    </StartupGate>,
  );
  await waitFor(() => expect(calls).toHaveLength(2));
  expect(screen.getByText("First screen")).toBeTruthy();
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
          false,
        ),
      )}
    >
      <p>First screen</p>
    </StartupGate>,
  );
  expect(
    await screen.findByRole("heading", { name: "Startup and privacy" }),
  ).toBeTruthy();
  expect(screen.queryByText("First screen")).toBeNull();
});

test("banner text follows the injected locale text", async () => {
  render(
    <LocaleTextContext.Provider
      value={(key, fallback) =>
        ({
          "ui.startup.banner_heading": "시작 안내",
          "ui.startup.banner_outside_verified": "검증하지 않은 조합입니다.",
          "ui.startup.banner_dismiss": "닫기",
          "ui.startup.check.support_matrix": "검증 환경",
          "ui.startup.outside_verified": "검증 밖",
        })[key] ?? fallback
      }
    >
      <StartupGate client={clientFor(() => outsideVerified)}>
        <p>First screen</p>
      </StartupGate>
    </LocaleTextContext.Provider>,
  );
  const banner = await screen.findByRole("region", { name: "시작 안내" });
  expect(within(banner).getByText("검증하지 않은 조합입니다.")).toBeTruthy();
  expect(within(banner).getByText("검증 환경")).toBeTruthy();
  expect(within(banner).getByRole("button", { name: "닫기" })).toBeTruthy();
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
    messages["ui.startup.banner_heading"] = text("시작 안내");
    messages["ui.startup.banner_outside_verified"] =
      text("검증하지 않은 조합입니다.");
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
      default:
        return { ...response, body: {} };
    }
  });
}

test("the Windows composition keeps the analysis shell and adds the localized banner", async () => {
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

  const banner = await screen.findByRole("region", { name: "시작 안내" });
  expect(within(banner).getByText("검증하지 않은 조합입니다.")).toBeTruthy();
  expect(await screen.findByText("Connected analysis")).toBeTruthy();
  expect(
    screen.queryByRole("heading", { name: "Startup and privacy" }),
  ).toBeNull();
  expect(calls.filter((name) => name === "app.startup_status")).toHaveLength(1);
});

test("the shell without the startup bridge makes no startup request", async () => {
  const calls: string[] = [];
  render(
    <DesktopLocale client={windowsClient(calls)} analysisEnabled={false} />,
  );
  await screen.findByText("Shell without analysis");
  expect(calls).not.toContain("app.startup_status");
  expect(screen.queryByRole("region", { name: "시작 안내" })).toBeNull();
});
