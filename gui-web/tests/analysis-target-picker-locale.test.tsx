import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { TargetPicker } from "../src/features/analysis/TargetPicker";
import { LocaleTextContext } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import type { LocaleProfile } from "../src/ipc";
import { createLocaleText } from "../src/state/locale-text";
import type { useTargets } from "../src/state/analysis-targets";
import { realLocales } from "./locale-real";

afterEach(cleanup);

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
