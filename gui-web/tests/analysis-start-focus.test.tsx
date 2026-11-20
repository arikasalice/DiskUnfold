import { cleanup, fireEvent, screen, waitFor } from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { resetReflectOffset, startPanel } from "./summary-fixture";

/*
 * Focus after an analysis starts and when a scan control goes away (shared
 * focus rules, design review of the result header): the screen title takes
 * the focus without a frame, the status badge never does, and the scan state
 * is announced once by its status region.
 */

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
  resetReflectOffset();
});

const title = () =>
  screen.getByRole("heading", { level: 2, name: "Disk space results" });

/** The status region around the scan state badge. */
function scanStatus(label: string) {
  const region = screen.getByText(label).closest('[role="status"]');
  if (!region) throw new Error("The scan state is outside a status region");
  return region;
}

test("starting from the start screen leaves the focus on the screen title", async () => {
  await startPanel();
  expect(document.activeElement).toBe(title());
  // A title focused by script is no tab stop; the theme draws no frame on it.
  expect(title().getAttribute("tabindex")).toBe("-1");
  expect(title().classList.contains("du-heading")).toBe(true);
});

test("the status badge is never a focus stop and announces the scan state once", async () => {
  const scan = await startPanel();
  const region = scanStatus("Scanning");
  expect(region.hasAttribute("tabindex")).toBe(false);
  expect(region.contains(document.activeElement)).toBe(false);
  expect(
    screen
      .getAllByRole("status")
      .filter((status) => status.textContent?.includes("Scanning")),
  ).toEqual([region]);
  // The focused title does not repeat the state through a description.
  expect(title().hasAttribute("aria-describedby")).toBe(false);
  expect(title().textContent).toBe("Disk space results");

  // Later states change the text of the same region, which announces them.
  scan.changeState("paused");
  await waitFor(() => expect(scanStatus("Scan paused")).toBe(region));
  expect(document.activeElement).toBe(title());
});

test("starting from the target editor of a result leaves the focus on the screen title", async () => {
  const scan = await startPanel();
  scan.changeState("done");
  fireEvent.click(await screen.findByRole("button", { name: "Analyze again" }));
  const start = screen.getByRole<HTMLButtonElement>("button", {
    name: "Start analysis",
  });
  if (start.disabled)
    fireEvent.click(screen.getByRole("button", { name: "Review scope" }));
  await waitFor(() => expect(start.disabled).toBe(false));
  expect(document.activeElement).not.toBe(title());

  scan.job.state = "running";
  scan.job.sequence += 1;
  fireEvent.click(start);
  await waitFor(() =>
    expect(screen.queryByRole("textbox", { name: "Folder path" })).toBeNull(),
  );
  expect(document.activeElement).toBe(title());
});

test("a scan that ends under a focused scan control moves the focus to Browse folders", async () => {
  const scan = await startPanel(undefined, vi.fn());
  const pause = screen.getByRole("button", { name: "Pause scan" });
  pause.focus();
  scan.changeState("done");
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Pause scan" })).toBeNull(),
  );
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Browse folders" }),
  );
});

test("a cancelled scan without Browse folders moves the focus from Cancel scan to the title", async () => {
  const scan = await startPanel();
  expect(screen.queryByRole("button", { name: "Browse folders" })).toBeNull();
  const cancel = screen.getByRole("button", { name: "Cancel scan" });
  cancel.focus();
  scan.changeState("cancelled");
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Cancel scan" })).toBeNull(),
  );
  expect(document.activeElement).toBe(title());
  expect(scanStatus("Scan cancelled").hasAttribute("tabindex")).toBe(false);
});

test("a scan that ends while the focus is elsewhere does not move it", async () => {
  const scan = await startPanel(undefined, vi.fn());
  const details = screen.getByRole("region", { name: "Result details" });
  details.focus();
  scan.changeState("done");
  await waitFor(() =>
    expect(screen.queryByRole("button", { name: "Cancel scan" })).toBeNull(),
  );
  expect(document.activeElement).toBe(details);
});
