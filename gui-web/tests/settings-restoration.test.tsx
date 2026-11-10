import {
  act,
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
  const client = createAnalysisClient(transport);
  const { rerender } = render(<SettingsRestoration client={client} visible />);

  const opener = await screen.findByRole("button", {
    name: "Settings restoration",
  });
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
  rerender(<SettingsRestoration client={client} visible={false} />);
  expect(
    screen.queryByRole("button", { name: "Settings restoration" }),
  ).toBeNull();
  rerender(<SettingsRestoration client={client} visible />);
  fireEvent.click(screen.getByRole("button", { name: "Settings restoration" }));
  expect(await screen.findByText("Restored", { exact: true })).toBeDefined();
  expect(transport).toHaveBeenCalledTimes(1);
});

test("F14 FR6 omits the restoration notice if the startup read fails", async () => {
  const transport =
    vi.fn<NonNullable<Parameters<typeof createAnalysisClient>[0]>>();
  transport.mockRejectedValueOnce(new Error("private diagnostic"));
  await act(async () => {
    render(
      <SettingsRestoration client={createAnalysisClient(transport)} visible />,
    );
  });
  expect(
    screen.queryByRole("button", { name: "Settings restoration" }),
  ).toBeNull();
  expect(screen.queryByRole("alert")).toBeNull();
  expect(screen.queryByText("private diagnostic")).toBeNull();
  expect(transport).toHaveBeenCalledTimes(1);
});
