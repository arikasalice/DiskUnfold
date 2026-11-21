import { cleanup, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { useDisplayPreferences } from "../src/features/analysis/use-display-preferences";
import type { AnalysisClient, SupportOsFamily } from "../src/ipc";
import { defaultSizeDisplay } from "../src/state/size-display";
import { LocaleOsFamilyContext } from "../src/state/use-locale";

afterEach(cleanup);

for (const [osFamily, expected] of [
  ["windows", "iec"],
  ["macos", "si"],
  ["linux", "si"],
  ["unknown", "iec"],
  [undefined, "iec"],
] as const satisfies readonly (readonly [
  SupportOsFamily | undefined,
  "iec" | "si",
])[]) {
  test(`automatic size units use ${String(osFamily)} fallback`, () => {
    expect(defaultSizeDisplay(osFamily)).toEqual({
      system: expected,
      unit: "auto",
    });
  });
}

test("unavailable display preferences fall back to the macOS convention", async () => {
  const client = {
    call: vi.fn().mockRejectedValue(new Error("settings unavailable")),
    subscribe: () => () => {},
  } as unknown as AnalysisClient;

  function DisplaySystem() {
    const { display } = useDisplayPreferences(client);
    return <span>{display.system}</span>;
  }

  render(
    <LocaleOsFamilyContext.Provider value="macos">
      <DisplaySystem />
    </LocaleOsFamilyContext.Provider>,
  );

  expect(await screen.findByText("si")).toBeTruthy();
});
