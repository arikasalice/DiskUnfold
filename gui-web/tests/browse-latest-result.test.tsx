import { cleanup, fireEvent, render, screen } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { App } from "../src/app/App";
import type { ScanPriority } from "../src/state/scan-priority";
import { sampleClient } from "./sample-support";

/*
 * F02 FR8.HD1: the folder list's "Show latest result" reuses the opening path
 * of the analysis panel's "Browse folders" and only for the scan it came from.
 */

vi.mock("../src/features/analysis/AnalysisPanel", () => ({
  AnalysisPanel: ({
    onBrowse,
    onScanContext,
  }: {
    onBrowse?: (version: string, choice: undefined, scan?: string) => void;
    onScanContext?: (context: unknown) => void;
  }) => (
    <div>
      {["rv1", "rv2"].map((version) => (
        <button
          key={version}
          onClick={() =>
            onScanContext?.({
              jobRef: "scan",
              resultVersion: version,
              scanning: true,
            })
          }
        >
          {"Publish " + version}
        </button>
      ))}
      <button onClick={() => onBrowse?.("rv1", undefined, "scan")}>
        Browse this scan
      </button>
      <button onClick={() => onBrowse?.("rv1", undefined, "earlier")}>
        Browse an earlier scan
      </button>
    </div>
  ),
}));
vi.mock("../src/features/settings/SettingsRestoration", () => ({
  SettingsRestoration: () => null,
}));
vi.mock("../src/features/browse/BrowsePanel", () => ({
  BrowsePanel: ({
    resultVersion,
    priority,
  }: {
    resultVersion?: string;
    priority?: ScanPriority;
  }) => {
    const item = { resultVersion: resultVersion ?? "", itemNo: 1 };
    return (
      <div
        data-testid="browse"
        data-version={resultVersion}
        data-superseded={String(priority?.superseded(item))}
        data-current={String(priority?.canRequest(item))}
      >
        {priority?.showLatest && (
          <button onClick={priority.showLatest}>Show latest result</button>
        )}
      </div>
    );
  },
}));

afterEach(cleanup);

const press = (name: string) =>
  fireEvent.click(screen.getByRole("button", { name, hidden: true }));
const list = () => screen.getByTestId("browse").dataset;

test("F02 FR8.HD1: Show latest result reopens the list on the scan's newest result", () => {
  render(<App analysis={sampleClient({}).client} />);
  press("Publish rv1");
  press("Browse this scan");
  expect(list()).toMatchObject({
    version: "rv1",
    superseded: "false",
    current: "true",
  });
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();

  press("Publish rv2");
  expect(list()).toMatchObject({ version: "rv1", superseded: "true" });
  press("Show latest result");
  expect(list()).toMatchObject({
    version: "rv2",
    superseded: "false",
    current: "true",
  });
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
});

test("F02 FR8.HD1: a list opened from another scan's result is never superseded", () => {
  render(<App analysis={sampleClient({}).client} />);
  press("Publish rv2");
  press("Browse an earlier scan");
  expect(list()).toMatchObject({
    version: "rv1",
    superseded: "false",
    current: "false",
  });
  expect(
    screen.queryByRole("button", { name: "Show latest result" }),
  ).toBeNull();
});
