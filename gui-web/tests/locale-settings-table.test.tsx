import axe from "axe-core";
import { readFileSync } from "node:fs";
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
import { createAnalysisClient } from "../src/ipc";
import type { LocaleBundleBody, LocaleProfile } from "../src/ipc";

// F13 FR3.HD1.gui-web.DD1, FR4.HD1.gui-web.DD1, FR6.HD1.gui-web.DD1, FR11.HD1, FR12.HD1, FR7.HD1:
// the appearance and language table. The messages are the shipped en and ko bundles, so the
// board's wording is checked as well. happy-dom lays nothing out, so reflow at 1024x700 and 200%
// is covered by reading the stylesheet; the real layout needs the native GUI check.

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  document.documentElement.removeAttribute("style");
  document.documentElement.lang = "en";
});

const languages = [
  { language: "en", selfNameKey: "locale.language.en", bundleRevision: "en" },
  { language: "ko", selfNameKey: "locale.language.ko", bundleRevision: "ko" },
];

type ShippedMessage =
  | string
  | { kind: "plural"; parameter: string; variants: Record<string, string> };

/** Reads a shipped language file as the bundle the host would send. */
function shipped(
  language: "en" | "ko",
  omit: (key: string) => boolean = () => false,
): LocaleBundleBody {
  const data = JSON.parse(
    readFileSync(`../user-env/i18n/${language}.json`, "utf8"),
  ) as [
    string,
    string,
    LocaleBundleBody["formatTable"],
    string,
    string,
    [string, ShippedMessage, [string, string][]][],
  ];
  const messages: Record<string, unknown> = {};
  for (const [key, message, parameters] of data[5]) {
    if (omit(key)) continue;
    const named = Object.fromEntries(parameters);
    messages[key] =
      typeof message === "string"
        ? { kind: "text", template: message, parameters: named }
        : { parameters: named, ...message };
  }
  return {
    messages,
    formatTable: data[2],
    pluralRule: data[3],
    reasonMessages: {},
    metadataRevision: "fixture",
  } as LocaleBundleBody;
}

type Fixture = {
  profile?: Partial<LocaleProfile>;
  /** Keys that the Korean bundle lacks. */
  koreanLacks?: (key: string) => boolean;
};

function fixture({ profile: initial, koreanLacks }: Fixture = {}) {
  let profile: LocaleProfile = {
    displayLanguage: "en",
    languageSource: "stored",
    dateFormat: "lang_default",
    numberFormat: "lang_default",
    timeZonePolicy: "utc",
    localTimeZoneStatus: "identified",
    motion: "follow_os",
    appScale: 100,
    profileRevision: "1",
    supportedLanguages: languages,
    ...initial,
  };
  const calls: string[] = [];
  const saves: unknown[] = [];
  const client = createAnalysisClient(async (_command, { envelope }) => {
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
        const { language } = envelope.body as { language: "en" | "ko" };
        return {
          ...response,
          body: {
            bundleRevision: language,
            bundle: shipped(
              language,
              language === "ko" ? koreanLacks : undefined,
            ),
          },
        };
      }
      case "i18n.set_profile": {
        const { changes } = envelope.body as {
          changes: Partial<LocaleProfile>;
        };
        saves.push(changes);
        profile = {
          ...profile,
          ...changes,
          profileRevision: String(BigInt(profile.profileRevision) + 1n),
        };
        return { ...response, body: profile };
      }
      case "target.list":
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
      case "app.startup_status":
        return {
          ...response,
          body: {
            grade: "pass",
            checks: [{ check: "webview", result: "pass", reasonKey: null }],
            firstRun: false,
            access: "not_applicable",
            analysisAllowed: true,
          },
        };
      default:
        return { ...response, body: {} };
    }
  });
  return { client, calls, saves };
}

/** Opens the dialog from the entry button and returns the table. */
async function openTable(entryName: string, tableName: string) {
  fireEvent.click(await screen.findByRole("button", { name: entryName }));
  const dialog = await screen.findByRole("dialog");
  return {
    dialog,
    table: within(dialog).getByRole("table", { name: tableName }),
  };
}

const english = () =>
  openTable("Language and display", "Appearance and language settings");
const korean = () => openTable("언어 및 표시", "모양과 언어 설정");

function rowOf(table: HTMLElement, name: string) {
  const row = within(table)
    .getByRole("rowheader", { name })
    .closest<HTMLElement>('[role="row"]');
  if (!row) throw new Error(`No row for ${name}`);
  return row;
}

function utcNoon() {
  vi.useFakeTimers({ toFake: ["Date"] });
  vi.setSystemTime(new Date("2026-11-14T05:32:00Z"));
}

test("the table has the board's columns and rows, and each format row shows a sample", async () => {
  utcNoon();
  const { client } = fixture({ profile: { displayLanguage: "ko" } });
  render(<DesktopLocale client={client} />);
  const { dialog, table } = await korean();

  expect(
    within(dialog).getByRole("heading", { name: "모양과 언어" }),
  ).toBeTruthy();
  expect(
    within(table)
      .getAllByRole("columnheader")
      .map((header) => header.textContent),
  ).toEqual(["설정", "현재 값", "적용"]);
  expect(
    within(table)
      .getAllByRole("rowheader")
      .map((header) => header.textContent),
  ).toEqual([
    "표시 언어",
    "날짜 형식",
    "숫자 형식",
    "시간대",
    "화면 배율",
    "모션",
    "용량 단위",
  ]);
  expect(within(table).getAllByRole("cell", { name: "즉시" })).toHaveLength(7);

  expect(
    within(rowOf(table, "날짜 형식")).getByText(
      "예시: 2026.11.14 05:32:00 UTC",
    ),
  ).toBeTruthy();
  expect(
    within(rowOf(table, "숫자 형식")).getByText("예시: 1,234.5"),
  ).toBeTruthy();
  expect(within(rowOf(table, "시간대")).getByText("예시: UTC")).toBeTruthy();
  expect(
    within(rowOf(table, "용량 단위")).getByText("IEC · 자동"),
  ).toBeTruthy();
  expect(
    within(rowOf(table, "용량 단위")).getByText("여기서는 읽기만 가능"),
  ).toBeTruthy();
  // The size unit is shown only; it has no control here.
  expect(within(rowOf(table, "용량 단위")).queryByRole("combobox")).toBeNull();
});

test("the samples follow the saved date and number formats", async () => {
  utcNoon();
  const { client } = fixture({
    profile: { dateFormat: "iso8601", numberFormat: "no_grouping" },
  });
  render(<DesktopLocale client={client} />);
  const { table } = await english();
  expect(
    within(rowOf(table, "Date format")).getByText(
      "Example: 2026-11-14T05:32:00Z",
    ),
  ).toBeTruthy();
  expect(
    within(rowOf(table, "Number format")).getByText("Example: 1234.5"),
  ).toBeTruthy();

  fireEvent.change(
    within(table).getByRole("combobox", { name: "Date format" }),
    {
      target: { value: "lang_default" },
    },
  );
  expect(
    await within(table).findByText("Example: 11/14/2026 05:32:00 UTC"),
  ).toBeTruthy();
  fireEvent.change(
    within(table).getByRole("combobox", { name: "Number format" }),
    {
      target: { value: "lang_default" },
    },
  );
  expect(await within(table).findByText("Example: 1,234.5")).toBeTruthy();
});

test("an unknown local time zone says so instead of showing an offset", async () => {
  const { client } = fixture({
    profile: { timeZonePolicy: "local", localTimeZoneStatus: "unavailable" },
  });
  render(<DesktopLocale client={client} />);
  const { table } = await english();
  const row = rowOf(table, "Time zone");
  expect(within(row).getByText("Local time zone unavailable")).toBeTruthy();
  expect(within(row).queryByText(/^Example:/)).toBeNull();
});

test("a scale change applies at once and can be reverted to the previous value", async () => {
  const { client, saves } = fixture({ profile: { displayLanguage: "ko" } });
  render(<DesktopLocale client={client} />);
  const { table } = await korean();
  const scale = within(table).getByRole<HTMLSelectElement>("combobox", {
    name: "화면 배율",
  });
  expect(Array.from(scale.options, (option) => option.value)).toEqual([
    "100",
    "110",
    "125",
    "150",
    "175",
    "200",
  ]);
  expect(within(table).queryByText(/이전 값/)).toBeNull();

  fireEvent.change(scale, { target: { value: "125" } });
  expect(await within(table).findByText("이전 값 100%")).toBeTruthy();
  expect(
    document.documentElement.style.getPropertyValue("--du-app-scale"),
  ).toBe("1.25");
  const revert = within(table).getByRole("button", { name: "100%로 되돌리기" });
  expect(revert.textContent).toBe("되돌리기");

  fireEvent.click(revert);
  await waitFor(() =>
    expect(
      document.documentElement.style.getPropertyValue("--du-app-scale"),
    ).toBe("1"),
  );
  expect(saves).toEqual([{ appScale: 125 }, { appScale: 100 }]);
  expect(within(table).queryByText(/이전 값/)).toBeNull();
  // The revert button is gone, so the focus is on the scale selector.
  expect(document.activeElement).toBe(scale);
});

test("the motion row explains that the operating system's request always wins", async () => {
  const { client } = fixture({ profile: { displayLanguage: "ko" } });
  render(<DesktopLocale client={client} />);
  const { table } = await korean();
  const row = rowOf(table, "모션");
  expect(
    within(row).getByText(
      "운영체제가 움직임 줄이기를 켜 두면 여기서 고른 값과 관계없이 항상 줄입니다.",
    ),
  ).toBeTruthy();
  const motion = within(row).getByRole<HTMLSelectElement>("combobox", {
    name: "모션",
  });
  expect(Array.from(motion.options, (option) => option.textContent)).toEqual([
    "시스템 설정 따르기",
    "움직임 줄이기",
  ]);
});

test("the unverified-translation band and badge appear only for a language with missing messages", async () => {
  const { client } = fixture({
    koreanLacks: (key) => key.startsWith("ui.history."),
  });
  render(<DesktopLocale client={client} />);
  const { table } = await english();
  const language = within(table).getByRole("combobox", {
    name: "Display language",
  });
  expect(within(table).queryByText("Some unverified")).toBeNull();
  expect(within(table).queryByText(/could not be verified/)).toBeNull();

  fireEvent.change(language, { target: { value: "ko" } });
  const band = await within(table).findByText(
    "일부 문구는 번역을 확인하지 못해 영어로 표시됩니다.",
  );
  expect(band.closest('[role="status"]')).toBeTruthy();
  // The band is a row of its own under the language row, and the badge sits beside the selector.
  expect(band.closest('[role="row"]')?.previousElementSibling).toBe(
    rowOf(table, "표시 언어"),
  );
  expect(
    within(rowOf(table, "표시 언어")).getByText("일부 미확인"),
  ).toBeTruthy();

  fireEvent.change(within(table).getByRole("combobox", { name: "표시 언어" }), {
    target: { value: "en" },
  });
  await waitFor(() =>
    expect(within(table).queryByText("일부 미확인")).toBeNull(),
  );
  expect(within(table).queryByText(/영어로 표시됩니다/)).toBeNull();
});

test("a complete translation shows neither band nor badge", async () => {
  const { client } = fixture({ profile: { displayLanguage: "ko" } });
  render(<DesktopLocale client={client} />);
  const { table } = await korean();
  expect(within(table).queryByText("일부 미확인")).toBeNull();
  expect(within(table).queryByText(/영어로 표시됩니다/)).toBeNull();
});

test("switching the language redraws the open table at once and keeps the focused selector", async () => {
  const { client } = fixture();
  render(<DesktopLocale client={client} />);
  const { dialog, table } = await english();
  const language = within(table).getByRole("combobox", {
    name: "Display language",
  });
  language.focus();

  fireEvent.change(language, { target: { value: "ko" } });
  await waitFor(() =>
    expect(within(table).getByRole("combobox", { name: "표시 언어" })).toBe(
      language,
    ),
  );
  expect(document.activeElement).toBe(language);
  expect(screen.getByRole("dialog", { name: "모양과 언어" })).toBe(dialog);
  expect(
    within(table)
      .getAllByRole("columnheader")
      .map((header) => header.textContent),
  ).toEqual(["설정", "현재 값", "적용"]);
  expect(document.documentElement.lang).toBe("ko");
});

test("every row can be operated from the keyboard", async () => {
  const { client } = fixture({ profile: { displayLanguage: "ko" } });
  render(<DesktopLocale client={client} />);
  const { dialog, table } = await korean();
  const stops = Array.from(
    dialog.querySelectorAll<HTMLElement>(
      'button, select, input, a[href], [tabindex]:not([tabindex="-1"])',
    ),
  );
  // Native selectors take the arrow, Home and End keys themselves; each row has exactly one
  // tab stop, in row order, and none is disabled or taken out of the tab order.
  const names = [
    "표시 언어",
    "날짜 형식",
    "숫자 형식",
    "시간대",
    "화면 배율",
    "모션",
  ];
  const selectors = names.map((name) =>
    within(table).getByRole("combobox", { name }),
  );
  expect(stops.filter((stop) => stop.tagName === "SELECT")).toEqual(selectors);
  for (const stop of stops) {
    expect((stop as HTMLButtonElement).disabled).toBe(false);
    expect(stop.tabIndex).toBeGreaterThanOrEqual(0);
  }
  for (let index = 1; index < selectors.length; index += 1) {
    expect(
      selectors[index - 1]!.compareDocumentPosition(selectors[index]!) &
        Node.DOCUMENT_POSITION_FOLLOWING,
    ).toBeTruthy();
  }
  // After a scale change the revert button follows the scale selector.
  fireEvent.change(selectors[4]!, { target: { value: "150" } });
  const revert = await within(table).findByRole("button", {
    name: "100%로 되돌리기",
  });
  expect(
    selectors[4]!.compareDocumentPosition(revert) &
      Node.DOCUMENT_POSITION_FOLLOWING,
  ).toBeTruthy();
  expect(
    selectors[5]!.compareDocumentPosition(revert) &
      Node.DOCUMENT_POSITION_PRECEDING,
  ).toBeTruthy();
});

test("the open table has no accessibility violations in the states that add rows and buttons", async () => {
  const { client } = fixture({
    profile: { displayLanguage: "ko" },
    koreanLacks: (key) => key.startsWith("ui.history."),
  });
  render(<DesktopLocale client={client} />);
  const { dialog, table } = await korean();
  await within(table).findByText("일부 미확인");
  fireEvent.change(within(table).getByRole("combobox", { name: "화면 배율" }), {
    target: { value: "175" },
  });
  await within(table).findByText("이전 값 100%");
  expect((await axe.run(dialog)).violations).toEqual([]);
});

for (const platform of ["Linux", "macOS"]) {
  test(`${platform}: the startup screen without analysis opens the same table, with no size unit request`, async () => {
    const { client, calls } = fixture({ profile: { displayLanguage: "ko" } });
    render(
      <ApplicationEntry
        client={client}
        localeEnabled
        startupStatusEnabled
        analysisEnabled={false}
      />,
    );
    const { table } = await korean();
    expect(
      within(table)
        .getAllByRole("rowheader")
        .map((header) => header.textContent),
    ).toEqual([
      "표시 언어",
      "날짜 형식",
      "숫자 형식",
      "시간대",
      "화면 배율",
      "모션",
    ]);
    expect(calls).not.toContain("space.display_preferences");
  });
}

test("the stylesheet stacks the rows in a narrow table and never scrolls or cuts text", () => {
  const css = readFileSync("src/theme/locale-settings.css", "utf8");
  expect(css).toMatch(/\.du-locale-table \{[^}]*container-type: inline-size/);
  expect(css).toMatch(
    /@container \(max-width: 34rem\) \{[\s\S]*\.du-locale-row \{[^}]*grid-template-columns: minmax\(0, 1fr\)/,
  );
  expect(css).toMatch(/\.du-locale-table \{[^}]*word-break: keep-all/);
  // Only the visually hidden copies (clip-path) may use the no-wrap form.
  const visible = css
    .split("}")
    .filter((rule) => !rule.includes("clip-path"))
    .join("}");
  expect(visible).not.toMatch(
    /overflow-x|text-overflow|white-space: nowrap|line-clamp/,
  );
  expect(visible).not.toMatch(/[\s;{](inline-size|width): \d+(\.\d+)?(px|rem)/);
  // Colours come from the theme tokens, so Light and Dark both apply.
  expect(css).not.toMatch(/#[0-9a-fA-F]{3,8}\b/);
  const source = readFileSync(
    "src/features/settings/LocaleSettings.tsx",
    "utf8",
  );
  expect(source).not.toMatch(/ClampedText|MiddleEllipsisText/);
  const shared = readFileSync("src/theme/workspace.css", "utf8");
  expect(shared.match(/\.du-action \{[^}]*\}/)?.[0]).toMatch(
    /white-space: normal/,
  );
});
