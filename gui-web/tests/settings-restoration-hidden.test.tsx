import {
  cleanup,
  fireEvent,
  render,
  screen,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { SettingsRestoration } from "../src/features/settings/SettingsRestoration";
import { createAnalysisClient } from "../src/ipc";
import type { SettingsRestoreStatus } from "../src/ipc";

afterEach(cleanup);

test("F03 FR14: the hidden size rule setting is not listed or counted in the restoration report", async () => {
  const report: SettingsRestoreStatus = {
    items: [
      { key: "space.default_basis", source: "default", reasonKey: null },
      {
        key: "space.aggregation_rule",
        source: "restored",
        reasonKey: "settings.value_invalid",
      },
    ],
    storedSchema: 3,
    appSchema: 3,
    migrated: false,
    writeLocked: false,
  };
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => ({ ...envelope, body: report }));
  render(
    <SettingsRestoration client={createAnalysisClient(transport)} visible />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Settings restoration" }));
  const dialog = await screen.findByRole("dialog", {
    name: "Settings restoration",
  });
  expect(await within(dialog).findByText("Default size basis")).toBeDefined();
  // No row, no unrecognized-setting placeholder, no raw key and no issue or restored badge.
  expect(within(dialog).queryByText(/Unrecognized setting/)).toBeNull();
  expect(dialog.textContent).not.toMatch(/aggregation|space\.|size\.agg/);
  expect(within(dialog).queryByText("Issues reported")).toBeNull();
  expect(within(dialog).queryByText("Settings restored")).toBeNull();
  expect(within(dialog).getByText("Defaults in use")).toBeDefined();
});
