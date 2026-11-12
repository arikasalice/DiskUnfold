import axe from "axe-core";
import { readFile } from "node:fs/promises";
import { resolve } from "node:path";
import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import {
  StartupStatusScreen,
  startupLanguageFrom,
} from "../src/features/startup/StartupStatusScreen";
import { createAnalysisClient } from "../src/ipc";
import type {
  StartupAccessAction,
  StartupAccessResponse,
  StartupStatus,
} from "../src/ipc";

afterEach(cleanup);

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

test("selects Korean only when a preferred locale is Korean", () => {
  expect(startupLanguageFrom(["ko-KR", "en-US"])).toBe("ko");
  expect(startupLanguageFrom(["en-US", "ko-KR"])).toBe("en");
  expect(startupLanguageFrom(["fr-FR", "en-US"])).toBe("en");
  expect(startupLanguageFrom([])).toBe("en");
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

  const recheck = screen.getByRole("button", {
    name: "Check permission again",
  });
  recheck.focus();
  fireEvent.keyDown(recheck, { key: "Enter", code: "Enter" });
  fireEvent.keyUp(recheck, { key: "Enter", code: "Enter" });
  await waitFor(() => {
    expect(screen.getByText("Denied")).toBeTruthy();
    expect(screen.getByText("Blocked by startup checks")).toBeTruthy();
    expect(screen.getByText(/Full Disk Access is denied/)).toBeTruthy();
  });
  expect(document.activeElement).toBe(recheck);
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
