import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { ApplicationEntry } from "../src/app/ApplicationEntry";
import { createAnalysisClient } from "../src/ipc";
import type {
  LocaleBundleBody,
  LocaleProfile,
  StartupStatus,
} from "../src/ipc";

// F13 FR3.HD1.gui-web.DD1, FR8.HD2.gui-web.DD1, FR11, FR12: the startup screen that replaces the
// shell where analysis is off keeps the shell's language and display entry.

afterEach(() => {
  cleanup();
  document.documentElement.lang = "en";
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

function bundle(language: string): LocaleBundleBody {
  const korean = language === "ko";
  const messages = Object.fromEntries(
    Object.entries({
      "locale.language.en": "English",
      "locale.language.ko": "한국어",
      "ui.locale.title": korean ? "언어 및 표시" : "Language and display",
      "ui.locale.language": korean ? "표시 언어" : "Display language",
      "ui.common.close": korean ? "닫기" : "Close",
      "ui.startup.title": korean ? "시작 및 개인정보" : "Startup and privacy",
      "ui.startup.status_heading": korean ? "시작 상태" : "Startup status",
      "ui.info.product_open": korean ? "DiskUnfold 정보" : "About DiskUnfold",
      "ui.info.support_open": korean ? "지원 정보" : "Support information",
      "ui.info.nav": korean ? "제품 및 지원" : "Product and support",
    }).map(([key, template]) => [
      key,
      { kind: "text", template, parameters: {} } as const,
    ]),
  );
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

function fixture(access: StartupStatus["access"]) {
  let profile: LocaleProfile = {
    displayLanguage: "en",
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
  const status: StartupStatus = {
    grade: "pass",
    checks: [{ check: "webview", result: "pass", reasonKey: null }],
    firstRun: false,
    access,
    analysisAllowed: true,
  };
  const requests: string[] = [];
  const client = createAnalysisClient(async (_command, { envelope }) => {
    requests.push(envelope.name);
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
      case "i18n.set_profile":
        profile = {
          ...profile,
          ...(envelope.body as { changes: Partial<LocaleProfile> }).changes,
          profileRevision: String(BigInt(profile.profileRevision) + 1n),
        };
        return { ...response, body: profile };
      case "app.startup_status":
        return { ...response, body: status };
      default:
        return { ...response, body: {} };
    }
  });
  return { client, requests };
}

const capabilities = [
  // Linux reports Full Disk Access as not applicable; macOS reports it as unknown until the
  // host has read it. Neither enables analysis.
  { platform: "Linux", access: "not_applicable" },
  { platform: "macOS", access: "unknown" },
] as const;

for (const { platform, access } of capabilities) {
  test(`${platform} startup screen offers language and display, and a language change redraws it`, async () => {
    const { client, requests } = fixture(access);
    const { container } = render(
      <ApplicationEntry
        client={client}
        localeEnabled
        startupStatusEnabled
        analysisEnabled={false}
      />,
    );

    expect(
      await screen.findByRole("heading", { name: "Startup status" }),
    ).toBeTruthy();
    const entry = screen.getByRole("button", { name: "Language and display" });
    expect(container.querySelectorAll("main")).toHaveLength(1);
    expect(container.querySelectorAll("nav.du-info-entry")).toHaveLength(1);

    fireEvent.click(entry);
    fireEvent.change(
      screen.getByRole("combobox", { name: "Display language" }),
      { target: { value: "ko" } },
    );

    expect(
      await screen.findByRole("heading", { name: "시작 상태" }),
    ).toBeTruthy();
    expect(
      screen.getByRole("heading", { name: "시작 및 개인정보" }),
    ).toBeTruthy();
    expect(screen.getByRole("combobox", { name: "표시 언어" })).toBeTruthy();
    expect(document.documentElement.lang).toBe("ko");
    expect(
      within(
        screen.getByRole("navigation", { name: "제품 및 지원" }),
      ).getByRole("button", { name: "지원 정보" }),
    ).toBeTruthy();
    expect(requests).not.toContain("scan.start");
    expect(requests).not.toContain("target.list");
  });
}

test("the startup screen has no tab stop beyond its named buttons", async () => {
  const { client } = fixture("not_applicable");
  const { container } = render(
    <ApplicationEntry
      client={client}
      localeEnabled
      startupStatusEnabled
      analysisEnabled={false}
    />,
  );
  await screen.findByRole("heading", { name: "Startup status" });
  const stops = container.querySelectorAll<HTMLElement>(
    'a[href], button, input, select, textarea, [tabindex]:not([tabindex="-1"])',
  );
  // The support button is the last stop. Nothing hidden or unnamed follows it, so a Tab after it
  // leaves the document through the webview, not through a page element (FR8.HD2).
  expect(Array.from(stops, (stop) => stop.textContent)).toEqual([
    "Language and display",
    "About DiskUnfold",
    "Support information",
  ]);
});
