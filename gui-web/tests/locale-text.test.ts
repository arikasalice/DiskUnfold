import { expect, test } from "vitest";
import type { LocaleBundleBody, LocaleProfile } from "../src/ipc";
import {
  createLocaleText,
  createVerifiedBundleText,
} from "../src/state/locale-text";
import type { LocaleView } from "../src/state/use-locale";
import { realLocales } from "./locale-real";

function textFor(
  osFamily: LocaleProfile["osFamily"],
  language: "en" | "ko" = "en",
  selected: LocaleBundleBody = realLocales[language].body,
  report: (language: string, key: string, reason: string) => void = () => {},
) {
  const view: LocaleView = {
    profile: { osFamily } as LocaleProfile,
    baseline: realLocales.en.body,
    selected,
    language,
  };
  return createLocaleText(view, realLocales[language].regional, report);
}

test("Windows and older hosts use base message keys", () => {
  const expected = "e.g. C:\\Users\\Minseo\\Documents";
  expect(textFor("windows")("ui.analysis.path_example_first", "")).toBe(
    expected,
  );
  expect(textFor(undefined)("ui.analysis.path_example_first", "")).toBe(
    expected,
  );
});

test("macOS and Linux prefer their variant, then the shared Unix variant", () => {
  expect(textFor("macos")("ui.analysis.path_example_first", "")).toBe(
    "e.g. /Users/minseo/Documents",
  );
  expect(textFor("macos")("ui.analysis.path_hint", "")).toBe(
    "Enter a local folder path.",
  );
  expect(textFor("linux")("ui.analysis.path_example_first", "")).toBe(
    "e.g. /home/minseo/Documents",
  );
  expect(textFor("linux")("ui.analysis.path_hint", "")).toBe(
    "Enter a local folder path.",
  );
});

test("a platform-specific message takes precedence over the Unix fallback", () => {
  const selected: LocaleBundleBody = {
    ...realLocales.en.body,
    messages: {
      ...realLocales.en.body.messages,
      "ui.analysis.path_hint.macos": {
        kind: "text",
        template: "macOS-specific path hint",
        parameters: {},
      },
      "ui.analysis.path_hint.linux": {
        kind: "text",
        template: "Linux-specific path hint",
        parameters: {},
      },
    },
  };

  expect(textFor("macos", "en", selected)("ui.analysis.path_hint", "")).toBe(
    "macOS-specific path hint",
  );
  expect(textFor("linux", "en", selected)("ui.analysis.path_hint", "")).toBe(
    "Linux-specific path hint",
  );
});

test("a missing OS variant uses the same-language base before English fallback", () => {
  const messages = { ...realLocales.ko.body.messages };
  delete messages["ui.analysis.path_hint.unix"];
  delete messages["ui.analysis.path_hint.macos"];
  delete messages["ui.analysis.path_hint.linux"];
  const selected = { ...realLocales.ko.body, messages };
  const reports: string[] = [];
  const text = textFor("macos", "ko", selected, (_language, key) => {
    reports.push(key);
  });

  expect(text("ui.analysis.path_hint", "")).toBe(
    "로컬 폴더 또는 드라이브 경로를 입력하세요.",
  );
  expect(reports).toEqual([]);
});

test("language fallback reports the base key and still selects its OS variant", () => {
  const messages = { ...realLocales.ko.body.messages };
  delete messages["ui.analysis.path_hint"];
  delete messages["ui.analysis.path_hint.unix"];
  delete messages["ui.analysis.path_hint.macos"];
  delete messages["ui.analysis.path_hint.linux"];
  const selected = { ...realLocales.ko.body, messages };
  const reports: string[] = [];
  const text = textFor("macos", "ko", selected, (_language, key) => {
    reports.push(key);
  });

  expect(text("ui.analysis.path_hint", "")).toBe(
    "Enter a local folder path. (번역 미확인)",
  );
  expect(reports).toEqual(["ui.analysis.path_hint"]);
});

test("verified startup text accepts an OS family and defaults to Windows", () => {
  expect(
    createVerifiedBundleText(realLocales.en.body, "macos")(
      "ui.analysis.path_example_first",
      "",
    ),
  ).toBe("e.g. /Users/minseo/Documents");
  expect(
    createVerifiedBundleText(realLocales.en.body)(
      "ui.analysis.path_example_first",
      "",
    ),
  ).toBe("e.g. C:\\Users\\Minseo\\Documents");
});
