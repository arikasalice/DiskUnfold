import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { App } from "../src/app/App";

const bridge = vi.hoisted(() => ({ available: false }));
vi.mock("../src/ipc", async (importOriginal) => ({
  ...(await importOriginal<typeof import("../src/ipc")>()),
  hasWindowsAnalysisBridge: () => bridge.available,
}));
vi.mock("../src/features/analysis/AnalysisPanel", () => ({
  AnalysisPanel: () => <div>Connected analysis</div>,
}));
vi.mock("../src/features/settings/SettingsRestoration", () => ({
  SettingsRestoration: () => null,
}));

afterEach(() => {
  cleanup();
  bridge.available = false;
});

test("explains the missing connection and reports an unsuccessful recheck", () => {
  render(<App />);
  expect(
    screen.getByRole("heading", { name: "Desktop connection unavailable" }),
  ).toBeTruthy();
  const retry = screen.getByRole("button", { name: "Check connection again" });
  retry.focus();
  expect(document.activeElement).toBe(retry);
  fireEvent.click(retry);
  expect(screen.getByRole("status").textContent).toContain("still unavailable");
  expect(screen.queryByRole("button", { name: /start scan/i })).toBeNull();
  expect(screen.queryByText("Connected analysis")).toBeNull();
});

test("recheck opens analysis only once a bridge is detected", () => {
  render(<App />);
  bridge.available = true;
  fireEvent.click(
    screen.getByRole("button", { name: "Check connection again" }),
  );
  expect(screen.getByText("Connected analysis")).toBeTruthy();
  expect(screen.queryByText("Desktop connection unavailable")).toBeNull();
});

test("an explicitly injected client takes precedence over missing native bridge", () => {
  const client = {} as import("../src/ipc").AnalysisClient;
  render(<App analysis={client} />);
  expect(screen.getByText("Connected analysis")).toBeTruthy();
  expect(screen.queryByText("Desktop connection unavailable")).toBeNull();
});
