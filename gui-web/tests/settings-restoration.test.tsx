import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { SettingsRestoration } from "../src/features/settings/SettingsRestoration";
import { createAnalysisClient } from "../src/ipc";
import type { SettingsRestoreStatus } from "../src/ipc";

afterEach(cleanup);

test("F14 FR6 opens a value-free startup report and shows the edit lock", async () => {
  const report: SettingsRestoreStatus = {
    items: [
      { key: "locale.display_language", source: "restored", reasonKey: null },
      {
        key: "space.shared_table_max_entries",
        source: "default",
        reasonKey: "settings.value_invalid",
      },
    ],
    storedSchema: 1,
    appSchema: 2,
    migrated: false,
    writeLocked: true,
  };
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => ({ ...envelope, body: report }));
  render(<SettingsRestoration client={createAnalysisClient(transport)} />);

  const opener = screen.getByRole("button", { name: "Settings restoration" });
  expect(transport).not.toHaveBeenCalled();
  opener.focus();
  fireEvent.click(opener);
  const dialog = await screen.findByRole("dialog", {
    name: "Settings restoration",
  });
  expect(
    await within(dialog).findByText("Restored", { exact: true }),
  ).toBeDefined();
  expect(
    within(dialog).getByText(
      "Using default The saved value failed validation.",
    ),
  ).toBeDefined();
  expect(
    within(dialog).getByText("Settings editing is locked for this startup."),
  ).toBeDefined();
  expect(transport).toHaveBeenCalledExactlyOnceWith("du_call", {
    envelope: expect.objectContaining({
      name: "settings.restore_status",
      body: {},
    }),
  });

  fireEvent.click(
    within(dialog).getByRole("button", { name: "Close details" }),
  );
  expect(screen.queryByRole("dialog")).toBeNull();
  await waitFor(() => expect(document.activeElement).toBe(opener));
});

test("F14 FR6 exposes a failed read and can read again when reopened", async () => {
  const transport =
    vi.fn<NonNullable<Parameters<typeof createAnalysisClient>[0]>>();
  transport.mockRejectedValueOnce(new Error("private diagnostic"));
  transport.mockImplementationOnce(async (_, { envelope }) => ({
    ...envelope,
    body: {
      items: [],
      storedSchema: null,
      appSchema: 1,
      migrated: false,
      writeLocked: false,
    },
  }));
  render(<SettingsRestoration client={createAnalysisClient(transport)} />);
  fireEvent.click(screen.getByRole("button", { name: "Settings restoration" }));
  expect(
    await screen.findByText("Settings restoration could not be read."),
  ).toBeDefined();
  expect(screen.queryByText("private diagnostic")).toBeNull();
  fireEvent.click(screen.getByRole("button", { name: "Close details" }));
  fireEvent.click(screen.getByRole("button", { name: "Settings restoration" }));
  expect(await screen.findByText("Unavailable", { exact: true })).toBeDefined();
  expect(transport).toHaveBeenCalledTimes(2);
});
