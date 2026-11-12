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

test("F14 FR6 and F01 Q8 name restored settings and identify unknown settings and errors", async () => {
  const report: SettingsRestoreStatus = {
    items: [
      { key: "display.size_unit", source: "restored", reasonKey: null },
      {
        key: "display.size_unit_system",
        source: "default",
        reasonKey: "settings.value_invalid",
      },
      { key: "scope.remember_recent", source: "restored", reasonKey: null },
      { key: "scope.recent_limit", source: "default", reasonKey: null },
      { key: "space.default_basis", source: "default", reasonKey: null },
      {
        key: "future.private_one",
        source: "default",
        reasonKey: "future.private_error",
      },
      { key: "future.private_two", source: "restored", reasonKey: null },
    ],
    storedSchema: 1,
    appSchema: 1,
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
  for (const label of [
    "Size unit",
    "Unit system",
    "Remember recent scan scopes",
    "Recent scope limit",
    "Default size basis",
    "Unrecognized setting 1",
    "Unrecognized setting 2",
  ])
    expect(await within(dialog).findByText(label)).toBeDefined();
  expect(
    within(dialog).getByText(
      "Using default The saved value failed validation.",
    ),
  ).toBeDefined();
  expect(
    within(dialog).getByText("Using default A restoration issue was reported."),
  ).toBeDefined();
  expect(dialog.textContent).not.toMatch(
    /display\.size_unit|space\.default_basis|future\.private/,
  );
});

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
  expect(within(dialog).getByText("Settings restored")).toBeDefined();
  expect(within(dialog).getByText("Defaults in use")).toBeDefined();
  expect(within(dialog).getByText("Issues reported")).toBeDefined();
  expect(within(dialog).queryByLabelText("Settings format")).toBeNull();
  fireEvent.click(
    within(dialog).getByRole("button", { name: "Format details" }),
  );
  expect(within(dialog).getByLabelText("Settings format")).toBeDefined();
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

test("F14 FR6 retries restoration without losing focus outside the dialog", async () => {
  const transport =
    vi.fn<NonNullable<Parameters<typeof createAnalysisClient>[0]>>();
  let completeRetry = () => {};
  transport.mockRejectedValueOnce(new Error("private diagnostic"));
  transport.mockImplementationOnce(async (_, { envelope }) => {
    await new Promise<void>((resolve) => {
      completeRetry = resolve;
    });
    return {
      ...envelope,
      body: {
        items: [],
        storedSchema: null,
        appSchema: 1,
        migrated: false,
        writeLocked: false,
      },
    };
  });
  render(
    <SettingsRestoration client={createAnalysisClient(transport)} visible />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Settings restoration" }));
  await screen.findByText("Settings restoration status could not be loaded.");
  expect(screen.queryByText("private diagnostic")).toBeNull();
  const retry = screen.getByRole("button", { name: "Retry" });
  retry.focus();
  fireEvent.click(retry);
  await screen.findByRole("progressbar", {
    name: "Loading restoration status",
  });
  const close = screen.getByRole("button", { name: "Close details" });
  await waitFor(() => expect(document.activeElement).toBe(close));
  await act(async () => completeRetry());
  await screen.findByText("No individual settings were reported.");
  expect(document.activeElement).toBe(close);
  expect(screen.queryByText("Settings restored")).toBeNull();
  expect(transport).toHaveBeenCalledTimes(2);
});

test("F14 FR6 ignores an old client response after the client changes", async () => {
  let release: (value: unknown) => void = () => {};
  const oldTransport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => ({
    ...envelope,
    body: await new Promise((resolve) => {
      release = resolve;
    }),
  }));
  const fresh: SettingsRestoreStatus = {
    items: [],
    storedSchema: null,
    appSchema: 3,
    migrated: false,
    writeLocked: false,
  };
  const newTransport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => ({ ...envelope, body: fresh }));
  const { rerender } = render(
    <SettingsRestoration client={createAnalysisClient(oldTransport)} visible />,
  );
  fireEvent.click(screen.getByRole("button", { name: "Settings restoration" }));
  rerender(
    <SettingsRestoration client={createAnalysisClient(newTransport)} visible />,
  );
  await screen.findByText("No individual settings were reported.");
  await act(async () => release({ ...fresh, writeLocked: true }));
  expect(
    screen.queryByText("Settings editing is locked for this startup."),
  ).toBeNull();
});
