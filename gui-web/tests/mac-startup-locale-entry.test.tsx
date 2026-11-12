import { cleanup, render, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { ApplicationEntry } from "../src/app/ApplicationEntry";
import { createAnalysisClient } from "../src/ipc";
import type {
  LocaleBundleBody,
  LocaleProfile,
  StartupStatus,
} from "../src/ipc";

afterEach(() => {
  cleanup();
  Reflect.deleteProperty(window, "webkit");
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
  const messages: LocaleBundleBody["messages"] = {
    "ui.common.translation_unverified": {
      kind: "text",
      template: "Translation unverified",
      parameters: {},
    },
  };
  if (language === "ko") {
    messages["ui.startup.title"] = {
      kind: "text",
      template: "공통 시작 화면",
      parameters: {},
    };
    messages["ui.startup.status_heading"] = {
      kind: "text",
      template: "공통 시작 상태",
      parameters: {},
    };
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

const startupStatus: StartupStatus = {
  grade: "unverifiable",
  checks: [
    { check: "webview", result: "pass", reasonKey: null },
    { check: "full_disk_access", result: "unverifiable", reasonKey: null },
  ],
  firstRun: false,
  access: "unknown",
  analysisAllowed: true,
};

test("Mac capabilities render the localized F15 screen through the WebKit IPC bridge", async () => {
  const requests: string[] = [];
  const postMessage = vi.fn(async (payload: string) => {
    const request = JSON.parse(payload) as {
      command: string;
      args: {
        envelope: {
          name: string;
          contractVersion: number;
          correlationId: string;
          body: Record<string, unknown>;
        };
      };
    };
    expect(request.command).toBe("du_call");
    const { envelope } = request.args;
    requests.push(envelope.name);
    let body: unknown;
    if (envelope.name === "i18n.get_profile") body = profile;
    else if (envelope.name === "i18n.get_bundle") {
      const language = String(envelope.body.language);
      body = {
        bundleRevision: language === "en" ? "en-rev" : "ko-rev",
        bundle: bundle(language),
      };
    } else if (envelope.name === "app.startup_status") body = startupStatus;
    else body = {};
    return JSON.stringify({ ...envelope, body });
  });
  Object.defineProperty(window, "webkit", {
    configurable: true,
    value: { messageHandlers: { diskunfold: { postMessage } } },
  });

  const client = createAnalysisClient();
  render(
    <ApplicationEntry
      client={client}
      localeEnabled
      startupStatusEnabled
      analysisEnabled={false}
    />,
  );

  expect(
    await screen.findByRole("heading", { name: "공통 시작 상태" }),
  ).toBeTruthy();
  expect(screen.getByRole("heading", { name: "공통 시작 화면" })).toBeTruthy();
  expect(document.documentElement.lang).toBe("ko");
  await waitFor(() => expect(requests).toContain("app.startup_status"));
  expect(requests.slice(0, 4)).toEqual([
    "i18n.get_profile",
    "i18n.get_bundle",
    "i18n.get_bundle",
    "app.startup_status",
  ]);
  expect(requests).not.toContain("scan.start");
  expect(requests).not.toContain("target.list");
  expect(requests).not.toContain("i18n.set_profile");
});
