import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
} from "@testing-library/react";
import { afterEach, expect, test } from "vitest";
import { App } from "../src/app/App";
import { AnalysisError, windowsPath } from "../src/ipc";
import type { AnalysisClient, ScopeDraft } from "../src/ipc";

afterEach(cleanup);

const path = windowsPath("C:\\saved\uD800");
const timestamp = "2026-11-12T00:00:00.000Z";
const draft: ScopeDraft = {
  includes: [path],
  exclusions: [{ kind: "name_pattern", pattern: "fixture.tmp" }],
  boundary: "current_volume",
  includedMounts: [],
  source: "recent",
};

function fixture(stale = false) {
  const calls: { name: string; body: unknown }[] = [];
  const client: AnalysisClient = {
    recentScopesEnabled: true,
    subscribe: () => () => undefined,
    call: (async (name: string, body: unknown) => {
      calls.push({ name, body });
      switch (name) {
        case "scope.recent_list":
          return {
            rows: [
              {
                recentScopeRef: "saved",
                includeCount: 1,
                lastUsedAt: timestamp,
              },
            ],
          };
        case "scope.recent_open":
          return {
            draft,
            targets: [{ path, state: "matched", proposedEntry: null }],
            review: {
              scope: draft,
              targets: [{ path, access: "accessible", reasonKey: null }],
              notices: [],
              reviewFingerprint: "recent-review",
              observedAt: timestamp,
              requestSeq: (body as { requestSeq: number }).requestSeq,
            },
          };
        case "target.list":
          return {
            targets: [],
            observedAt: timestamp,
            probeJobRef: null,
            unavailableReason: null,
          };
        case "scan.start":
          if (stale) throw new AnalysisError("STALE_VERSION", "scope.changed");
          return {
            jobRef: "scan",
            existing: false,
            scopeVersion: "scope",
            scopeFingerprint: "saved",
          };
        default:
          throw new Error(name);
      }
    }) as AnalysisClient["call"],
  };
  return { calls, client };
}

async function applySavedRange() {
  fireEvent.click(
    screen.getByRole("button", { name: "Recent analysis ranges" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Review range 1" }),
  );
  fireEvent.click(
    await screen.findByRole("button", { name: "Use this range" }),
  );
  return screen.getByRole("button", { name: "Start scan" });
}

test("explicit scan uses the restored native paths, conditions and review", async () => {
  const { calls, client } = fixture();
  render(<App analysis={client} />);
  expect(calls.some(({ name }) => name === "scope.recent_list")).toBe(false);
  const start = await applySavedRange();
  expect(calls.some(({ name }) => name === "scan.start")).toBe(false);
  expect(
    (
      screen.getByRole("textbox", {
        name: "Windows folder path",
      }) as HTMLInputElement
    ).value,
  ).toBe(path.display);
  fireEvent.click(start);
  await waitFor(() =>
    expect(calls.find(({ name }) => name === "scan.start")?.body).toEqual({
      draft: { ...draft, source: "gui" },
      reviewFingerprint: "recent-review",
    }),
  );
  expect(calls.some(({ name }) => name === "scope.review")).toBe(false);
});

test("a stale restored scope cannot be re-reviewed as ordinary path input", async () => {
  const { calls, client } = fixture(true);
  render(<App analysis={client} />);
  fireEvent.click(await applySavedRange());
  await screen.findByText(
    "Reopen the saved range or select its paths again before scanning.",
  );
  expect(
    (screen.getByRole("button", { name: "Start scan" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  const review = screen.getByRole("button", { name: "Review scope" });
  expect((review as HTMLButtonElement).disabled).toBe(true);
  fireEvent.click(review);
  expect(calls.some(({ name }) => name === "scope.review")).toBe(false);
});
