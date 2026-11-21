import {
  cleanup,
  fireEvent,
  render,
  screen,
  waitFor,
  within,
} from "@testing-library/react";
import { afterEach, expect, test, vi } from "vitest";
import { HistoryPanel } from "../src/features/history/HistoryPanel";
import { AnalysisError } from "../src/ipc";
import type {
  AnalysisClient,
  HistoryListResponse,
  HistoryRetentionApplyResponse,
  HistoryRetentionPolicy,
  HistoryRetentionReview,
} from "../src/ipc";

afterEach(() => {
  cleanup();
  vi.restoreAllMocks();
});

const emptyHistory: HistoryListResponse = {
  revision: { deleteGeneration: "0", maxRecordSeq: "0" },
  rows: [],
  nextCursor: null,
};

const policy = (
  options: Partial<HistoryRetentionPolicy> = {},
): HistoryRetentionPolicy => ({
  enabled: false,
  suggestedDays: 90,
  policyFingerprint: "policy-fingerprint",
  ...options,
});

const review = (
  changeKind: HistoryRetentionReview["changeKind"],
  options: Partial<HistoryRetentionReview> = {},
): HistoryRetentionReview => ({
  reviewRef: "review-1",
  changeKind,
  confirmationRequired: changeKind === "enable" || changeKind === "shorten",
  cleanupAfterApply: changeKind === "enable" || changeKind === "shorten",
  targetCount: "3",
  targetEarliestAtMs: Date.UTC(2026, 0, 1),
  targetLatestAtMs: Date.UTC(2026, 1, 1),
  heldCount: "2",
  undoInfoRemovalCount: "1",
  fingerprint: "review-fingerprint-1",
  ...options,
});

function setup({
  initialPolicy = policy(),
  reviewValue = review("enable"),
  applyValue,
}: {
  initialPolicy?: HistoryRetentionPolicy;
  reviewValue?: HistoryRetentionReview | (() => HistoryRetentionReview);
  applyValue?:
    | HistoryRetentionApplyResponse
    | (() => HistoryRetentionApplyResponse)
    | Error;
} = {}) {
  let currentPolicy = initialPolicy;
  const call = vi.fn(async (name: string, body: unknown): Promise<unknown> => {
    if (name === "history.list") return emptyHistory;
    if (name === "history.retention_get") return currentPolicy;
    if (name === "history.retention_review")
      return typeof reviewValue === "function" ? reviewValue() : reviewValue;
    if (name === "history.retention_apply") {
      if (applyValue instanceof Error) throw applyValue;
      const result =
        typeof applyValue === "function"
          ? applyValue()
          : (applyValue ?? {
              outcome: "applied",
              policy: policy({ enabled: true, specifiedDays: 90 }),
            });
      currentPolicy = result.policy;
      return result;
    }
    if (name === "history.retention_cancel") return { outcome: "cancelled" };
    throw new Error(`Unexpected command ${name}: ${JSON.stringify(body)}`);
  });
  const client = {
    call: call as unknown as AnalysisClient["call"],
    subscribe: () => () => undefined,
  } as AnalysisClient;
  render(<HistoryPanel client={client} onClose={() => undefined} />);
  return { call };
}

async function openRetention() {
  await screen.findByRole("button", { name: "Retention" });
  fireEvent.click(screen.getByRole("button", { name: "Retention" }));
  await screen.findByRole("heading", { name: "History retention" });
}

test("shows the suggested value without sending a review or save request", async () => {
  const { call } = setup();
  await openRetention();
  expect(
    (
      screen.getByRole("spinbutton", {
        name: "Keep history for",
      }) as HTMLInputElement
    ).value,
  ).toBe("90");
  expect(screen.getByText("Suggested")).toBeDefined();
  expect(
    (screen.getByRole("button", { name: "Apply" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
  expect(
    call.mock.calls.filter(([name]) => name.startsWith("history.retention_")),
  ).toEqual([["history.retention_get", {}]]);
});

test("the switch reviews the suggested period and waits for confirmation", async () => {
  const { call } = setup();
  await openRetention();
  const toggle = screen.getByRole("switch", { name: "Clean up automatically" });
  fireEvent.click(toggle);
  await screen.findByRole("alertdialog", {
    name: "Turn on automatic cleanup?",
  });
  expect(call).toHaveBeenCalledWith("history.retention_review", {
    enabled: true,
    days: 90,
  });
  expect((toggle as HTMLInputElement).checked).toBe(false);
});

test("the enable confirmation shows the entered period when there are no targets", async () => {
  const { call } = setup({
    reviewValue: review("enable", {
      targetCount: "0",
      targetEarliestAtMs: null,
      targetLatestAtMs: null,
    }),
  });
  await openRetention();
  fireEvent.change(
    screen.getByRole("spinbutton", { name: "Keep history for" }),
    { target: { value: "12" } },
  );
  fireEvent.click(
    screen.getByRole("switch", { name: "Clean up automatically" }),
  );
  const dialog = await screen.findByRole("alertdialog", {
    name: "Turn on automatic cleanup?",
  });
  expect(within(dialog).getByText("Retention period")).toBeDefined();
  expect(within(dialog).getByText("12 days")).toBeDefined();
  expect(
    within(dialog).getByText(
      "Nothing will be removed now. History older than 12 days will be cleaned up in the future.",
    ),
  ).toBeDefined();
  expect(call).toHaveBeenCalledWith("history.retention_review", {
    enabled: true,
    days: 12,
  });
});

test("only enable and shorten reviews open the confirmation dialog", async () => {
  const { call } = setup({
    initialPolicy: policy({ enabled: false, specifiedDays: 90 }),
    reviewValue: review("set_period", { confirmationRequired: false }),
    applyValue: {
      outcome: "applied",
      policy: policy({ enabled: false, specifiedDays: 120 }),
    },
  });
  await openRetention();
  const days = screen.getByRole("spinbutton", { name: "Keep history for" });
  fireEvent.change(days, { target: { value: "120" } });
  fireEvent.click(screen.getByRole("button", { name: "Apply" }));
  await waitFor(() =>
    expect(call).toHaveBeenCalledWith("history.retention_apply", {
      reviewRef: "review-1",
      fingerprint: "review-fingerprint-1",
    }),
  );
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(call).toHaveBeenCalledWith("history.retention_get", {});
});

test("closing the confirmation cancels its review once and restores focus", async () => {
  const { call } = setup();
  await openRetention();
  const toggle = screen.getByRole("switch", { name: "Clean up automatically" });
  toggle.focus();
  fireEvent.click(toggle);
  await screen.findByRole("alertdialog");
  fireEvent.click(screen.getByRole("button", { name: "Cancel" }));
  await waitFor(() =>
    expect(
      call.mock.calls.filter(([name]) => name === "history.retention_cancel"),
    ).toHaveLength(1),
  );
  await waitFor(() => expect(document.activeElement).toBe(toggle));
  expect(
    call.mock.calls.some(([name]) => name === "history.retention_apply"),
  ).toBe(false);
});

test("a stale apply reviews once more, replaces the counts and focuses Cancel", async () => {
  let reviewCount = 0;
  let applyCount = 0;
  const { call } = setup({
    initialPolicy: policy({ enabled: true, specifiedDays: 90 }),
    reviewValue: () =>
      ++reviewCount === 1
        ? review("shorten", { reviewRef: "old-review", targetCount: "2" })
        : review("shorten", {
            reviewRef: "new-review",
            targetCount: "4",
            fingerprint: "fresh-fingerprint",
          }),
    applyValue: () => {
      applyCount += 1;
      if (applyCount === 1)
        throw new AnalysisError("STALE_VERSION", "history.review_stale");
      return {
        outcome: "applied",
        policy: policy({ enabled: true, specifiedDays: 30 }),
      };
    },
  });
  await openRetention();
  fireEvent.change(
    screen.getByRole("spinbutton", { name: "Keep history for" }),
    {
      target: { value: "30" },
    },
  );
  fireEvent.click(screen.getByRole("button", { name: "Apply" }));
  const dialog = await screen.findByRole("alertdialog", {
    name: "Shorten retention to 30 days?",
  });
  fireEvent.click(within(dialog).getByRole("button", { name: "Shorten" }));
  await screen.findByText(
    "History changed in the meantime, so the counts were updated.",
  );
  await waitFor(() =>
    expect(
      call.mock.calls.filter(([name]) => name === "history.retention_review"),
    ).toHaveLength(2),
  );
  expect(document.activeElement).toBe(
    screen.getByRole("button", { name: "Cancel" }),
  );
  expect(screen.getByText("4 records")).toBeDefined();
  expect(call).toHaveBeenCalledWith("history.retention_cancel", {
    reviewRef: "old-review",
  });
});

test("count_unavailable shows a panel error without presenting a zero", async () => {
  setup({
    reviewValue: () => {
      throw new AnalysisError("INTERNAL", "history.count_unavailable");
    },
  });
  await openRetention();
  fireEvent.click(
    screen.getByRole("switch", { name: "Clean up automatically" }),
  );
  await screen.findByText(
    "History to be removed couldn't be counted, so this can't be changed. Try again shortly.",
  );
  expect(screen.queryByRole("alertdialog")).toBeNull();
  expect(screen.queryByText(/0 records/)).toBeNull();
});

test("a policy save failure keeps the confirmation open with its error", async () => {
  const { call } = setup({
    applyValue: () => {
      throw new AnalysisError("INTERNAL", "history.policy_store_failed");
    },
  });
  await openRetention();
  fireEvent.click(
    screen.getByRole("switch", { name: "Clean up automatically" }),
  );
  await screen.findByRole("alertdialog");
  fireEvent.click(screen.getByRole("button", { name: "Turn on" }));
  await screen.findByText("Settings couldn't be saved. Try again.");
  expect(screen.getByRole("alertdialog")).toBeDefined();
  expect(
    call.mock.calls.filter(([name]) => name === "history.retention_get"),
  ).toHaveLength(1);
});

test("Apply stays disabled when the entered period equals the stored value", async () => {
  setup({ initialPolicy: policy({ specifiedDays: 45 }) });
  await openRetention();
  expect(
    (
      screen.getByRole("spinbutton", {
        name: "Keep history for",
      }) as HTMLInputElement
    ).value,
  ).toBe("45");
  expect(
    (screen.getByRole("button", { name: "Apply" }) as HTMLButtonElement)
      .disabled,
  ).toBe(true);
});

test("invalid days show the required error and send no review", async () => {
  const { call } = setup({ initialPolicy: policy({ specifiedDays: 90 }) });
  await openRetention();
  fireEvent.change(
    screen.getByRole("spinbutton", { name: "Keep history for" }),
    {
      target: { value: "0" },
    },
  );
  fireEvent.click(screen.getByRole("button", { name: "Apply" }));
  await screen.findByText("Enter a whole number of 1 or more.");
  expect(
    call.mock.calls.some(([name]) => name === "history.retention_review"),
  ).toBe(false);
});

test("an invalid period blocks enabling auto-cleanup and focuses the field", async () => {
  const { call } = setup({ initialPolicy: policy({ specifiedDays: 90 }) });
  await openRetention();
  const days = screen.getByRole("spinbutton", { name: "Keep history for" });
  const toggle = screen.getByRole("switch", { name: "Clean up automatically" });
  fireEvent.change(days, { target: { value: "0" } });
  fireEvent.click(toggle);
  await screen.findByText("Enter a whole number of 1 or more.");
  expect(document.activeElement).toBe(days);
  expect((toggle as HTMLInputElement).checked).toBe(false);
  expect(
    call.mock.calls.some(([name]) => name === "history.retention_review"),
  ).toBe(false);
});

test("closing the side panel returns focus to its Retention button", async () => {
  setup();
  const trigger = await screen.findByRole("button", { name: "Retention" });
  fireEvent.click(trigger);
  await screen.findByRole("heading", { name: "History retention" });
  fireEvent.click(
    screen.getByRole("button", { name: "Close retention settings" }),
  );
  await waitFor(() => expect(document.activeElement).toBe(trigger));
});

test("the Retention header action reports when its panel is expanded", async () => {
  setup();
  const trigger = await screen.findByRole("button", { name: "Retention" });
  expect(trigger.getAttribute("aria-expanded")).toBe("false");
  fireEvent.click(trigger);
  await screen.findByRole("heading", { name: "History retention" });
  expect(trigger.getAttribute("aria-expanded")).toBe("true");
});
