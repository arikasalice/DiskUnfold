import {
  cleanup,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { App } from "../src/app/App";
import { ApplicationEntry } from "../src/app/ApplicationEntry";
import { DesktopLocale } from "../src/app/DesktopLocale";
import { createAnalysisClient } from "../src/ipc";
import type {
  LocaleBundleBody,
  LocaleProfile,
  StartupStatus,
} from "../src/ipc";
import { fakeHost, productInfo, supportReport } from "./info-host";

const bridge = vi.hoisted(() => ({ available: false }));
vi.mock("../src/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../src/ipc")>()),
  hasWindowsAnalysisBridge: () => bridge.available,
}));
vi.mock("../src/features/analysis/AnalysisPanel", () => ({
  AnalysisPanel: () => <div>Connected analysis</div>,
}));
vi.mock("../src/features/settings/SettingsRestoration", () => ({
  SettingsRestoration: () => null,
}));

afterEach(() => {
  cleanup();
  bridge.available = false;
  document.documentElement.lang = "en";
});

const startupStatus: StartupStatus = {
  grade: "unverifiable",
  checks: [{ check: "webview", result: "pass", reasonKey: null }],
  firstRun: false,
  access: "unknown",
  analysisAllowed: true,
};

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
    messages["ui.info.product_open"] = text("DiskUnfold 정보");
    messages["ui.info.support_open"] = text("지원 정보");
    messages["ui.info.nav"] = text("제품 및 지원");
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

function localeClient() {
  return createAnalysisClient(async (_command, { envelope }) => {
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
        return { ...response, body: startupStatus };
      default:
        return { ...response, body: {} };
    }
  });
}

const infoNav = () =>
  screen.queryByRole("navigation", { name: "Product and support" });

test("App shows the entry in its header, from the analysis client or the information client", () => {
  const { client } = fakeHost({
    "app.product_info": () => productInfo(),
    "app.support_info": () => supportReport(),
  });
  const first = render(<App analysis={client} />);
  const header = first.container.querySelector("header.du-app-header");
  expect(header).toBeTruthy();
  const nav = within(header as HTMLElement).getByRole("navigation", {
    name: "Product and support",
  });
  expect(
    within(nav)
      .getAllByRole("button")
      .map((button) => button.textContent),
  ).toEqual(["About DiskUnfold", "Support information"]);
  first.unmount();

  const second = render(<App infoClient={client} />);
  expect(
    within(
      second.container.querySelector("header.du-app-header") as HTMLElement,
    ).getByRole("navigation", { name: "Product and support" }),
  ).toBeTruthy();
  second.unmount();

  render(<App />);
  expect(infoNav()).toBeNull();
});

test("ApplicationEntry puts the entry after the startup screen when the locale host is off", async () => {
  const { client } = fakeHost({
    "app.startup_status": () => startupStatus,
    "app.product_info": () => productInfo(),
    "app.support_info": () => supportReport(),
  });
  const { container } = render(
    <ApplicationEntry
      client={client}
      localeEnabled={false}
      startupStatusEnabled
      analysisEnabled={false}
    />,
  );
  const nav = await screen.findByRole("navigation", {
    name: "Product and support",
  });
  expect(nav.className).toContain("du-info-entry--standalone");
  const startup = container.firstElementChild;
  expect(startup).toBeTruthy();
  expect(startup).not.toBe(nav);
  expect(
    (startup as Element).compareDocumentPosition(nav) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(within(nav).getAllByRole("button")).toHaveLength(2);
});

test("ApplicationEntry without the startup screen or a connection offers no entry", () => {
  const { client } = fakeHost({});
  render(
    <ApplicationEntry
      client={client}
      localeEnabled={false}
      startupStatusEnabled={false}
      analysisEnabled={false}
    />,
  );
  expect(infoNav()).toBeNull();
});

test("ApplicationEntry inside the locale host uses the selected language for the entry", async () => {
  render(
    <ApplicationEntry
      client={localeClient()}
      localeEnabled
      startupStatusEnabled
      analysisEnabled={false}
    />,
  );
  const nav = await screen.findByRole("navigation", { name: "제품 및 지원" });
  expect(nav.className).toContain("du-info-entry--standalone");
  expect(
    within(nav).getByRole("button", { name: "DiskUnfold 정보" }),
  ).toBeTruthy();
  expect(within(nav).getByRole("button", { name: "지원 정보" })).toBeTruthy();
  expect(document.documentElement.lang).toBe("ko");
});

test("DesktopLocale shows the entry in the app header from its own connection", async () => {
  const { container } = render(
    <DesktopLocale client={localeClient()} analysisEnabled={false} />,
  );
  const nav = await screen.findByRole("navigation", { name: "제품 및 지원" });
  expect(nav.closest("header.du-app-header")).toBeTruthy();
  expect(container.querySelectorAll("nav.du-info-entry")).toHaveLength(1);
  await waitFor(() =>
    expect(within(nav).getByRole("button", { name: "지원 정보" })).toBeTruthy(),
  );
});
