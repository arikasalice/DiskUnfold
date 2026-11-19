import {
  act,
  cleanup,
  fireEvent,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import type { AnalysisClient } from "../src/ipc";
import {
  logical,
  passReflectInterval,
  resetReflectOffset,
  startPanel,
} from "./summary-fixture";

/*
 * #155: right after a scan finishes, the summary can still show an earlier
 * result while the final one is already current: its totals are still being
 * read. The engine opens only a scan's current result and refuses the earlier
 * one with result.requery_required, so "Browse folders" opens the newest
 * result known for the scan, as the summary's dialogs do. The test holds the
 * read of the final totals instead of waiting on any timer.
 */

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  resetReflectOffset();
});

/** Holds every space.total read of `version` until `release` is called. */
function holdTotals(client: AnalysisClient, version: string) {
  let release = () => {};
  const gate = new Promise<void>((resolve) => {
    release = resolve;
  });
  let held = 0;
  const call = client.call.bind(client) as (
    name: string,
    body: { resultVersion?: string },
  ) => Promise<unknown>;
  client.call = ((name: string, body: { resultVersion?: string }) => {
    if (name !== "space.total" || body.resultVersion !== version)
      return call(name, body);
    held += 1;
    return gate.then(() => call(name, body));
  }) as unknown as AnalysisClient["call"];
  return { release, held: () => held };
}

test("#155: Browse folders opens the final result while the summary still shows an earlier one", async () => {
  const onBrowse = vi.fn();
  const scan = await startPanel(undefined, onBrowse);
  const finalTotals = holdTotals(scan.client, "v2");
  passReflectInterval();
  scan.publish("v2", "done");
  await screen.findByText("Scan finished");
  await waitFor(() => expect(finalTotals.held()).toBe(1));
  // The summary still shows the interim result, which is no longer current.
  expect(logical().getByText("100 bytes")).toBeDefined();
  expect(screen.getByText("Result version: Interim")).toBeDefined();

  fireEvent.click(screen.getByRole("button", { name: "Browse folders" }));
  expect(onBrowse).toHaveBeenLastCalledWith("v2", undefined, "scan");

  await act(async () => finalTotals.release());
  await screen.findByText("Result version: Final");
  fireEvent.click(screen.getByRole("button", { name: "Browse folders" }));
  expect(onBrowse).toHaveBeenLastCalledWith("v2", undefined, "scan");
  expect(onBrowse).toHaveBeenCalledTimes(2);
});
