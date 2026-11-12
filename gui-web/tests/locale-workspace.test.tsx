import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { DesktopLocale } from "../src/app/DesktopLocale";
import { createAnalysisClient } from "../src/ipc";
import type { LocaleBundleBody, LocaleProfile } from "../src/ipc";

afterEach(cleanup);

const languages = [
  {
    language: "en",
    selfNameKey: "locale.language.en",
    bundleRevision: "english",
  },
  {
    language: "ko",
    selfNameKey: "locale.language.ko",
    bundleRevision: "korean",
  },
];

function bundle(language: string): LocaleBundleBody {
  const messages = Object.fromEntries(
    Object.entries({
      "locale.language.en": "English",
      "locale.language.ko": "한국어",
      "ui.locale.title":
        language === "ko" ? "언어 및 표시" : "Language and display",
      "ui.locale.language":
        language === "ko" ? "표시 언어" : "Display language",
      "ui.analysis.folder_path":
        language === "ko" ? "Windows 폴더 경로" : "Windows folder path",
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
        lang_default: "MM/dd/yyyy HH:mm:ss",
        iso8601: "yyyy-MM-dd'T'HH:mm:ss",
      },
      unitSymbols: {},
    },
    pluralRule: language,
    reasonMessages: {},
    metadataRevision: "metadata",
  };
}

test("switches the mounted desktop language without discarding the scope draft", async () => {
  let profile: LocaleProfile = {
    displayLanguage: "en",
    languageSource: "os",
    dateFormat: "lang_default",
    numberFormat: "lang_default",
    timeZonePolicy: "local",
    localTimeZoneStatus: "unavailable",
    motion: "follow_os",
    appScale: 100,
    profileRevision: "9007199254740993",
    supportedLanguages: languages,
  };
  let targets = 0;
  const requests: unknown[] = [];
  const client = createAnalysisClient(async (_command, { envelope }) => {
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
            bundleRevision: language === "en" ? "english" : "korean",
            bundle: bundle(language),
          },
        };
      }
      case "i18n.set_profile":
        requests.push(envelope.body);
        profile = {
          ...profile,
          displayLanguage: "ko",
          languageSource: "stored",
          profileRevision: "9007199254740994",
        };
        return { ...response, body: profile };
      case "target.list":
        targets += 1;
        return {
          ...response,
          body: {
            targets: [],
            observedAt: "2026-11-12T10:00:00Z",
            probeJobRef: null,
            unavailableReason: null,
          },
        };
      case "space.display_preferences":
        return {
          ...response,
          body: {
            settingsRevision: "0",
            unitSystem: "iec",
            unit: "auto",
            defaultBasis: "allocated",
            warningKey: null,
          },
        };
      default:
        return { ...response, body: {} };
    }
  });
  render(<DesktopLocale client={client} />);
  const input = await screen.findByRole("textbox", {
    name: "Windows folder path",
  });
  fireEvent.change(input, { target: { value: "C:\\Fixture" } });
  fireEvent.click(screen.getByRole("button", { name: "Language and display" }));
  fireEvent.change(screen.getByRole("combobox", { name: "Display language" }), {
    target: { value: "ko" },
  });
  await waitFor(() => expect(document.documentElement.lang).toBe("ko"));
  expect(screen.getByRole("combobox", { name: "표시 언어" })).toBeTruthy();
  expect((input as HTMLInputElement).value).toBe("C:\\Fixture");
  expect(input.getAttribute("id")).toBeTruthy();
  expect(targets).toBe(1);
  expect(requests).toEqual([
    {
      expectedProfileRevision: "9007199254740993",
      changes: { displayLanguage: "ko" },
    },
  ]);
});
