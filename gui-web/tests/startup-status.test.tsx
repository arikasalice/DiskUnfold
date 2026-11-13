import axe from "axe-core";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import {
  act,
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import {
  PERMISSION_POLL_INTERVAL_MS,
  StartupStatusScreen,
} from "../src/features/startup/StartupStatusScreen";
import { LocaleTextContext } from "../src/i18n/presentation";
import { StartupGate } from "../src/features/startup/StartupGate";
import { createAnalysisClient } from "../src/ipc";
import type {
  StartupAccessAction,
  StartupAccessResponse,
  StartupStatus,
} from "../src/ipc";

afterEach(() => {
  cleanup();
  vi.useRealTimers();
  vi.unstubAllGlobals();
  document.documentElement.lang = "en";
});

/** Fakes only the polling interval, so promises and the client keep running normally. */
function useIntervalFakes() {
  vi.useFakeTimers({ toFake: ["setInterval", "clearInterval"] });
}

/** Advances the fake interval clock and lets the pending requests settle. */
async function advance(milliseconds: number) {
  await act(async () => {
    vi.advanceTimersByTime(milliseconds);
    for (let turn = 0; turn < 4; turn += 1) {
      await new Promise<void>((resolve) => setTimeout(resolve, 0));
    }
  });
}

function report(access: string, analysisAllowed: boolean): StartupStatus {
  return {
    grade: "unverifiable",
    checks: [
      { check: "webview", result: "pass", reasonKey: null },
      {
        check: "full_disk_access",
        result: access === "denied" ? "blocked" : "unverifiable",
        reasonKey:
          access === "denied" ? "startup.full_disk_access_required" : null,
      },
    ],
    firstRun: false,
    access,
    analysisAllowed,
  };
}

test("uses shared locale text instead of navigator language preferences", async () => {
  document.documentElement.lang = "en";
  vi.stubGlobal("navigator", {
    languages: ["ko-KR"],
    language: "ko-KR",
  });
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => ({
    ...envelope,
    body: report("unknown", true),
  }));

  render(
    <LocaleTextContext.Provider
      value={(key, fallback) =>
        key === "ui.startup.status_heading" ? "공통 시작 상태" : fallback
      }
    >
      <StartupStatusScreen client={createAnalysisClient(transport)} />
    </LocaleTextContext.Provider>,
  );

  expect(document.documentElement.lang).toBe("en");
  expect(
    await screen.findByRole("heading", { name: "공통 시작 상태" }),
  ).toBeTruthy();
});

test("keeps blocked analysis separate from an unknown access observation", async () => {
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => {
    if (envelope.name !== "app.startup_status") {
      throw new Error(`Unexpected command: ${envelope.name}`);
    }
    return { ...envelope, body: report("unknown", false) };
  });

  render(<StartupStatusScreen client={createAnalysisClient(transport)} />);

  expect(
    await screen.findByText(
      "Startup checks do not allow analysis. Review the listed results for details.",
    ),
  ).toBeTruthy();
  expect(screen.getAllByText("Unknown").length).toBeGreaterThan(0);
  expect(screen.queryByText(/Full Disk Access is denied/)).toBeNull();
});

test("offers a retry after the initial startup status read fails", async () => {
  let attempts = 0;
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => {
    if (envelope.name !== "app.startup_status") {
      throw new Error(`Unexpected command: ${envelope.name}`);
    }
    attempts += 1;
    if (attempts === 1) throw new Error("Status read failed");
    return { ...envelope, body: report("unknown", true) };
  });

  render(<StartupStatusScreen client={createAnalysisClient(transport)} />);

  expect(
    await screen.findByText("Startup status could not be read. Try again."),
  ).toBeTruthy();
  fireEvent.click(screen.getByRole("button", { name: "Try again" }));
  expect(
    await screen.findByRole("heading", { name: "Startup status" }),
  ).toBeTruthy();
  expect(attempts).toBe(2);
});

test("uses the refreshed startup status when permission changes after IF-20", async () => {
  let status = report("unknown", true);
  let opened = false;
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => {
    if (envelope.name === "app.startup_status") {
      return { ...envelope, body: status };
    }
    if (envelope.name === "app.startup_access") {
      // Model the OS permission changing again between the IF-20 reply and IF-05.
      if (opened) status = report("denied", false);
      opened = true;
      const response: StartupAccessResponse = {
        access: "confirmed",
        analysisAllowed: true,
        settingsOpened: false,
      };
      return { ...envelope, body: response };
    }
    throw new Error(`Unexpected command: ${envelope.name}`);
  });

  render(<StartupStatusScreen client={createAnalysisClient(transport)} />);
  await screen.findByRole("heading", { name: "Startup status" });
  status = report("denied", false);
  fireEvent.click(
    screen.getByRole("button", { name: "Open Privacy & Security Settings" }),
  );

  await waitFor(() => {
    expect(screen.getAllByText("Denied").length).toBeGreaterThan(0);
    expect(screen.getByText("Blocked by startup checks")).toBeTruthy();
    expect(screen.getByText(/Full Disk Access is denied/)).toBeTruthy();
  });
});

test("keeps IF-20 settings feedback when the follow-up status read fails", async () => {
  let statusReads = 0;
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => {
    if (envelope.name === "app.startup_status") {
      statusReads += 1;
      if (statusReads > 1) throw new Error("Status read failed");
      return { ...envelope, body: report("unknown", true) };
    }
    if (envelope.name === "app.startup_access") {
      const response: StartupAccessResponse = {
        access: "unknown",
        analysisAllowed: true,
        settingsOpened: true,
      };
      return { ...envelope, body: response };
    }
    throw new Error(`Unexpected command: ${envelope.name}`);
  });

  render(<StartupStatusScreen client={createAnalysisClient(transport)} />);
  await screen.findByRole("heading", { name: "Startup status" });
  fireEvent.click(
    screen.getByRole("button", { name: "Open Privacy & Security Settings" }),
  );

  expect(
    await screen.findByText(
      /System Settings reported that its privacy page opened/i,
    ),
  ).toBeTruthy();
  expect(
    screen.getByText("The permission status could not be updated. Try again."),
  ).toBeTruthy();
  expect(screen.getByText("Unknown")).toBeTruthy();
});

test("shows F15 status and permission actions without invoking analysis", async () => {
  let access = "unknown";
  let analysisAllowed = true;
  const commands: string[] = [];
  const actions: StartupAccessAction[] = [];
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => {
    commands.push(envelope.name);
    if (envelope.name === "app.startup_status") {
      return { ...envelope, body: report(access, analysisAllowed) };
    }
    if (envelope.name === "app.startup_access") {
      const body = envelope.body as { action: StartupAccessAction };
      actions.push(body.action);
      if (body.action === "recheck") {
        access = "denied";
        analysisAllowed = false;
      }
      const response: StartupAccessResponse = {
        access,
        analysisAllowed,
        settingsOpened: false,
      };
      return { ...envelope, body: response };
    }
    throw new Error(`Unexpected command: ${envelope.name}`);
  });

  const { container } = render(
    <StartupStatusScreen client={createAnalysisClient(transport)} />,
  );
  expect(
    await screen.findByRole("heading", { name: "Startup status" }),
  ).toBeTruthy();
  expect(screen.getByText("Unknown")).toBeTruthy();
  expect(
    screen.getByText(/Disk analysis is not enabled in this build/),
  ).toBeTruthy();

  expect((await axe.run(container)).violations).toEqual([]);

  fireEvent.click(
    screen.getByRole("button", { name: "Open Privacy & Security Settings" }),
  );
  expect(
    await screen.findByText(/macOS did not confirm that Settings opened/i),
  ).toBeTruthy();

  expect(
    screen.queryByRole("button", { name: "Check permission again" }),
  ).toBeNull();
  access = "denied";
  analysisAllowed = false;
  fireEvent.focus(window);
  await waitFor(() => {
    expect(screen.getAllByText("Denied").length).toBeGreaterThan(0);
    expect(screen.getByText("Blocked by startup checks")).toBeTruthy();
    expect(screen.getByText(/Full Disk Access is denied/)).toBeTruthy();
  });
  // The denied row carries the same word as the status area (D3).
  expect(screen.getAllByText("Denied").length).toBe(2);
  expect(actions).toEqual(["open_privacy_settings", "recheck"]);
  expect(
    commands.every(
      (command) =>
        command === "app.startup_status" || command === "app.startup_access",
    ),
  ).toBe(true);
  expect(commands).not.toContain("scan.start");
});

test("keeps action and readout layout responsive below 24rem", async () => {
  const styles = await readFile(
    resolve(process.cwd(), "src/features/startup/startup-status.css"),
    "utf8",
  );
  expect(styles).toContain("flex-wrap: wrap");
  expect(styles).toContain("@media (max-width: 24rem)");
  expect(styles).toContain("grid-template-columns: minmax(0, 1fr)");
});

function pollingHarness(sequence: Array<[string, boolean]>) {
  const calls: string[] = [];
  let step = 0;
  let inFlight = 0;
  let maxInFlight = 0;
  const at = (index: number): [string, boolean] =>
    sequence[Math.min(index, sequence.length - 1)] ?? ["unknown", true];
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => {
    inFlight += 1;
    maxInFlight = Math.max(maxInFlight, inFlight);
    calls.push(envelope.name);
    await Promise.resolve();
    try {
      if (envelope.name === "app.startup_status") {
        const [access, allowed] = at(step);
        return { ...envelope, body: report(access, allowed) };
      }
      if (envelope.name === "app.startup_access") {
        step += 1;
        const [access, allowed] = at(step);
        const response: StartupAccessResponse = {
          access,
          analysisAllowed: allowed,
          settingsOpened: false,
        };
        return { ...envelope, body: response };
      }
      throw new Error(`Unexpected command: ${envelope.name}`);
    } finally {
      inFlight -= 1;
    }
  });
  return {
    client: createAnalysisClient(transport),
    calls,
    maxInFlight: () => maxInFlight,
  };
}

test("leaves the permission screen by itself when access becomes granted", async () => {
  useIntervalFakes();
  try {
    const harness = pollingHarness([
      ["denied", false],
      ["denied", false],
      ["confirmed", true],
    ]);
    const onAllowed = vi.fn();
    render(
      <StartupStatusScreen client={harness.client} onAllowed={onAllowed} />,
    );
    await advance(0);
    expect(screen.getAllByText("Denied")[0]).toBeTruthy();
    expect(onAllowed).not.toHaveBeenCalled();
    // No manual recheck control is offered.
    expect(
      screen.queryByRole("button", { name: "Check permission again" }),
    ).toBeNull();

    await advance(PERMISSION_POLL_INTERVAL_MS);
    expect(onAllowed).not.toHaveBeenCalled();
    await advance(PERMISSION_POLL_INTERVAL_MS);
    expect(onAllowed).toHaveBeenCalledTimes(1);
    expect(onAllowed.mock.calls[0]?.[0]).toMatchObject({
      access: "confirmed",
      analysisAllowed: true,
    });

    // Polling stops once granted.
    const settled = harness.calls.length;
    await advance(PERMISSION_POLL_INTERVAL_MS * 5);
    expect(harness.calls.length).toBe(settled);
    expect(onAllowed).toHaveBeenCalledTimes(1);
  } finally {
    vi.useRealTimers();
  }
});

test("re-checks on a fixed interval, on focus and when the window becomes visible", async () => {
  useIntervalFakes();
  try {
    const harness = pollingHarness([["denied", false]]);
    render(<StartupStatusScreen client={harness.client} />);
    await advance(0);
    const rechecks = () =>
      harness.calls.filter((name) => name === "app.startup_access").length;
    expect(rechecks()).toBe(0);

    await advance(PERMISSION_POLL_INTERVAL_MS - 1);
    expect(rechecks()).toBe(0);
    await advance(1);
    expect(rechecks()).toBe(1);

    fireEvent.focus(window);
    await advance(0);
    expect(rechecks()).toBe(2);

    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "hidden",
    });
    document.dispatchEvent(new Event("visibilitychange"));
    await advance(0);
    expect(rechecks()).toBe(2);
    Object.defineProperty(document, "visibilityState", {
      configurable: true,
      value: "visible",
    });
    document.dispatchEvent(new Event("visibilitychange"));
    await advance(0);
    expect(rechecks()).toBe(3);
  } finally {
    vi.useRealTimers();
  }
});

test("never overlaps permission requests and ignores a reply after unmount", async () => {
  useIntervalFakes();
  try {
    let release: (() => void) | undefined;
    let rechecks = 0;
    let inFlight = 0;
    let maxInFlight = 0;
    const transport = vi.fn<
      NonNullable<Parameters<typeof createAnalysisClient>[0]>
    >(async (_, { envelope }) => {
      if (envelope.name === "app.startup_status") {
        return { ...envelope, body: report("denied", false) };
      }
      rechecks += 1;
      inFlight += 1;
      maxInFlight = Math.max(maxInFlight, inFlight);
      // The first recheck stays pending while more ticks and focus events arrive.
      if (rechecks === 1) {
        await new Promise<void>((resolve) => {
          release = resolve;
        });
      }
      inFlight -= 1;
      const response: StartupAccessResponse = {
        access: "confirmed",
        analysisAllowed: true,
        settingsOpened: false,
      };
      return { ...envelope, body: response };
    });
    const onAllowed = vi.fn();
    const view = render(
      <StartupStatusScreen
        client={createAnalysisClient(transport)}
        onAllowed={onAllowed}
      />,
    );
    await advance(0);
    await advance(PERMISSION_POLL_INTERVAL_MS);
    expect(rechecks).toBe(1);
    await advance(PERMISSION_POLL_INTERVAL_MS * 3);
    fireEvent.focus(window);
    await advance(0);
    expect(rechecks).toBe(1);
    expect(maxInFlight).toBe(1);

    view.unmount();
    release?.();
    await advance(PERMISSION_POLL_INTERVAL_MS * 3);
    expect(onAllowed).not.toHaveBeenCalled();
    expect(rechecks).toBe(1);
  } finally {
    vi.useRealTimers();
  }
});

test("the gate swaps the permission screen for the main screen once access is granted", async () => {
  useIntervalFakes();
  const harness = pollingHarness([
    ["denied", false],
    ["confirmed", true],
  ]);
  render(
    <StartupGate client={harness.client}>
      <p>Main analysis screen</p>
    </StartupGate>,
  );
  await advance(0);
  expect(screen.getByRole("heading", { name: "Startup status" })).toBeTruthy();
  expect(screen.queryByText("Main analysis screen")).toBeNull();

  // No button is pressed: the next poll finds the grant.
  await advance(PERMISSION_POLL_INTERVAL_MS);
  expect(screen.getByText("Main analysis screen")).toBeTruthy();
  expect(screen.queryByRole("heading", { name: "Startup status" })).toBeNull();
});

test("asks the user to move the app to Applications when it runs elsewhere", async () => {
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => ({
    ...envelope,
    body: { ...report("denied", false), appLocation: "elsewhere" },
  }));
  render(<StartupStatusScreen client={createAnalysisClient(transport)} />);
  expect(
    await screen.findByText(/Move DiskUnfold.app to the Applications folder/),
  ).toBeTruthy();
});

function platformReport(access: string): StartupStatus {
  return {
    ...report(access, true),
    checks: [
      { check: "webview", result: "pass", reasonKey: null },
      {
        check: "full_disk_access",
        result: access === "not_applicable" ? "not_applicable" : "pass",
        reasonKey: null,
      },
    ],
  };
}

async function renderStartup(status: StartupStatus) {
  const transport = vi.fn<
    NonNullable<Parameters<typeof createAnalysisClient>[0]>
  >(async (_, { envelope }) => ({ ...envelope, body: status }));
  render(<StartupStatusScreen client={createAnalysisClient(transport)} />);
  await screen.findByRole("heading", { name: "Startup status" });
}

test("macOS: Full Disk Access guidance, row and settings button are shown", async () => {
  await renderStartup(platformReport("confirmed"));
  expect(screen.getByText(/Full Disk Access for this Mac/)).toBeTruthy();
  expect(screen.getAllByText("Full Disk Access").length).toBeGreaterThan(1);
  expect(
    screen.getByRole("button", { name: "Open Privacy & Security Settings" }),
  ).toBeTruthy();
});

test("Windows and Linux: neutral copy without macOS permission rows or actions", async () => {
  await renderStartup(platformReport("not_applicable"));
  expect(
    screen.getByText(
      "Review startup checks for this computer. Disk analysis is not enabled in this build.",
    ),
  ).toBeTruthy();
  expect(
    screen.getByText(
      "This build provides startup checks only; disk analysis is not enabled.",
    ),
  ).toBeTruthy();
  expect(document.body.textContent).not.toMatch(
    /Mac|Full Disk Access|Privacy & Security/,
  );
  expect(screen.queryByRole("button")).toBeNull();
  expect(screen.getAllByText("Passed").length).toBeGreaterThan(0);
});

test("the neutral variant has Korean copy", async () => {
  const raw = await readFile(
    resolve(process.cwd(), "../user-env/i18n/ko.json"),
    "utf8",
  );
  expect(raw).toContain("ui.startup.introduction_other");
  expect(raw).toContain("이 컴퓨터의 시작 점검");
  expect(raw).toContain("ui.startup.not_applicable_notice");
});
