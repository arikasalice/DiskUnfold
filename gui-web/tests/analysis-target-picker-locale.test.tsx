import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { TargetPicker } from "../src/features/analysis/TargetPicker";
import { LocaleTextContext } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import type { LocaleProfile } from "../src/ipc";
import { createLocaleText } from "../src/state/locale-text";
import type { useTargets } from "../src/state/analysis-targets";
import { unixPath } from "../src/ipc";
import { realLocales } from "./locale-real";

afterEach(cleanup);

function renderTargets(entries: Array<Record<string, unknown>>) {
  const targets = {
    list: {
      targets: entries,
      probeJobRef: null,
      observedAt: "2026-11-20T00:00:00.000Z",
      unavailableReason: null,
    },
    probes: {},
    loading: false,
    error: undefined,
    finished: true,
    refresh: vi.fn(async () => {}),
    cancel: vi.fn(),
  } as unknown as ReturnType<typeof useTargets>;
  const text = createLocaleText(
    {
      profile: { osFamily: "macos" } as LocaleProfile,
      baseline: realLocales.en.body,
      selected: realLocales.ko.body,
      language: "ko",
    },
    realLocales.ko.regional,
    () => {},
  );
  return render(
    <LocaleTextContext.Provider value={text}>
      <RegionalFormatContext.Provider value={realLocales.ko.regional}>
        <TargetPicker
          targets={targets}
          disabled={false}
          selected={[undefined]}
          choose={() => {}}
          addPath={() => {}}
          removePath={() => {}}
          dropEnabled={false}
          dropError={undefined}
          pathDescription=""
          reviewDescriptionId={undefined}
          feedback={() => null}
          isInitial
          onPathCompositionChange={() => {}}
          review={{
            reviewing: false,
            targets: undefined,
            openPrivacySettings: () => {},
          }}
        />
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>,
  );
}

const startupTarget = (extra: Record<string, unknown> = {}) => ({
  kind: "internal",
  path: unixPath("/"),
  connected: true,
  volumeId: null,
  identitySource: "unavailable",
  access: "checking",
  reasonKey: null,
  isStartupVolume: true,
  ...extra,
});

test("startup target uses the volume name and exposes root path and marker", () => {
  renderTargets([startupTarget({ volumeName: "Macintosh HD" })]);
  const option = screen.getByRole("radio", {
    name: "Macintosh HD, /",
  });
  expect(screen.getByText("Macintosh HD").getAttribute("title")).toBe(
    "Macintosh HD · / · 시동 디스크",
  );
  const description = option
    .getAttribute("aria-describedby")!
    .split(" ")
    .map((id) => document.getElementById(id)?.textContent ?? "")
    .join(" ");
  expect(description).toContain("시동 디스크");
});

test("startup target falls back to the startup disk name when volume name is absent", () => {
  renderTargets([startupTarget()]);
  expect(screen.getByRole("radio", { name: "시동 디스크, /" })).toBeTruthy();
});

test("target picker shows the macOS path example and the shared folder label", () => {
  const targets = {
    list: {
      targets: [],
      probeJobRef: null,
      observedAt: "2026-11-20T00:00:00.000Z",
      unavailableReason: null,
    },
    probes: {},
    loading: false,
    error: undefined,
    finished: true,
    refresh: vi.fn(async () => {}),
    cancel: vi.fn(),
  } as ReturnType<typeof useTargets>;
  const text = createLocaleText(
    {
      profile: { osFamily: "macos" } as LocaleProfile,
      baseline: realLocales.en.body,
      selected: realLocales.ko.body,
      language: "ko",
    },
    realLocales.ko.regional,
    () => {},
  );

  render(
    <LocaleTextContext.Provider value={text}>
      <RegionalFormatContext.Provider value={realLocales.ko.regional}>
        <TargetPicker
          targets={targets}
          disabled={false}
          selected={[undefined]}
          choose={() => {}}
          addPath={() => {}}
          removePath={() => {}}
          dropEnabled={false}
          dropError={undefined}
          pathDescription=""
          reviewDescriptionId={undefined}
          feedback={() => null}
          isInitial
          onPathCompositionChange={() => {}}
          review={{
            reviewing: false,
            targets: undefined,
            openPrivacySettings: () => {},
          }}
        />
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>,
  );

  expect(screen.getByRole("textbox", { name: "폴더 경로" })).toBeTruthy();
  expect(
    screen.getByPlaceholderText("예: /Users/minseo/Documents"),
  ).toBeTruthy();
});
