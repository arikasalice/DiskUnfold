import {
  cleanup,
  act,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { DesktopLocale } from "../src/app/DesktopLocale";
import { createAnalysisClient } from "../src/ipc";
import type { LocaleBundleBody, LocaleProfile } from "../src/ipc";

afterEach(() => {
  cleanup();
  vi.unstubAllGlobals();
});

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
      "ui.locale.motion": "Motion",
      "ui.locale.scale": "App scale",
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

function desktopFixture() {
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
          ...(envelope.body as { changes: Partial<LocaleProfile> }).changes,
          languageSource: "stored",
          profileRevision: String(BigInt(profile.profileRevision) + 1n),
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
  return { client, requests, targetCalls: () => targets };
}

test("switches the mounted desktop language without discarding the scope draft", async () => {
  const { client, requests, targetCalls } = desktopFixture();
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
  expect(targetCalls()).toBe(1);
  expect(requests).toEqual([
    {
      expectedProfileRevision: "9007199254740993",
      changes: { displayLanguage: "ko" },
    },
  ]);
});

test("updates app scale and respects both sources of reduced motion without remounting the workspace", async () => {
  let reduceMotion = false;
  const media = new EventTarget();
  Object.defineProperty(media, "matches", { get: () => reduceMotion });
  vi.stubGlobal("matchMedia", (query: string) => {
    expect(query).toBe("(prefers-reduced-motion: reduce)");
    return media;
  });
  const { client, targetCalls } = desktopFixture();
  render(<DesktopLocale client={client} />);
  const input = await screen.findByRole("textbox", {
    name: "Windows folder path",
  });
  const root = document.documentElement;
  expect(root.dataset.duMotion).toBe("standard");
  fireEvent.click(screen.getByRole("button", { name: "Language and display" }));
  const motion = screen.getByRole("combobox", { name: "Motion" });
  const scale = screen.getByRole("combobox", { name: "App scale" });

  act(() => {
    reduceMotion = true;
    media.dispatchEvent(new Event("change"));
  });
  expect(root.dataset.duMotion).toBe("reduce");
  fireEvent.change(motion, { target: { value: "reduce" } });
  await waitFor(() =>
    expect((motion as HTMLSelectElement).value).toBe("reduce"),
  );
  act(() => {
    reduceMotion = false;
    media.dispatchEvent(new Event("change"));
  });
  expect(root.dataset.duMotion).toBe("reduce");
  fireEvent.change(motion, { target: { value: "follow_os" } });
  await waitFor(() => expect(root.dataset.duMotion).toBe("standard"));

  for (const [value, factor] of [
    [150, "1.5"],
    [175, "1.75"],
    [200, "2"],
  ]) {
    fireEvent.change(scale, { target: { value: String(value) } });
    await waitFor(() =>
      expect(root.style.getPropertyValue("--du-app-scale")).toBe(factor),
    );
  }
  expect(input.isConnected).toBe(true);
  expect(targetCalls()).toBe(1);
});
