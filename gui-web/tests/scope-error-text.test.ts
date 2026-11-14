import { expect, test } from "vitest";
import {
  AnalysisError,
  scopeErrorText,
  scopeReviewErrorText,
} from "../src/ipc";
import type { LocaleText } from "../src/i18n/presentation";

const fallbackText: LocaleText = (_key, fallback) => fallback;

test.each([
  [
    "scope notice",
    "scan.path_denied",
    "Access to a selected folder was denied. Check its permissions and review the scope again.",
  ],
  [
    "review reason",
    "scope.target_unavailable",
    "Check that every selected path is an accessible local folder, then review the scope again.",
  ],
  [
    "job reason",
    "job.queue_full",
    "Too many jobs are waiting. Try the scan again later.",
  ],
  [
    "closing job reason",
    "job.shutting_down",
    "The scan stopped because DiskUnfold is closing.",
  ],
])(
  "F02: a start failure with a %s key shows that reason",
  (_source, reasonKey, message) => {
    const error = new AnalysisError("INVALID_ARGUMENT", reasonKey);
    expect(scopeErrorText(error, fallbackText)).toBe(message);
  },
);

test("F02: a reason key is shown in the active language", () => {
  const korean: LocaleText = (key, fallback) =>
    key === "ui.analysis.scan_reason_queue_full"
      ? "대기 중인 작업이 너무 많습니다."
      : fallback;
  const error = new AnalysisError("RESOURCE_EXHAUSTED", "job.queue_full");
  expect(scopeErrorText(error, korean)).toBe("대기 중인 작업이 너무 많습니다.");
});

test.each([
  ["an unknown reason key", new AnalysisError("INTERNAL", "scope.future")],
  // A start request is never cancelled; that copy describes a running scan.
  ["a cancellation key", new AnalysisError("INTERNAL", "job.cancelled")],
  ["a transport failure", new Error("socket closed")],
  ["a non-error value", "failed"],
])("F02: %s uses the general message", (_case, error) => {
  expect(scopeErrorText(error, fallbackText)).toBe("The operation failed");
});

test.each([
  [
    "a review reason before the scope notice of the same key",
    "path.unresolvable",
    "Enter a valid local folder path, then review the scope again.",
  ],
  [
    "a scope notice",
    "scan.path_denied",
    "Access to a selected folder was denied. Check its permissions and review the scope again.",
  ],
  ["no job reason", "job.queue_full", "The operation failed"],
  ["no job reason", "job.shutting_down", "The operation failed"],
])("F02: a review failure shows %s", (_case, reasonKey, message) => {
  const error = new AnalysisError("INVALID_ARGUMENT", reasonKey);
  expect(scopeReviewErrorText(error, fallbackText)).toBe(message);
});

test("F02: a stale version asks for a new review before any reason", () => {
  const error = new AnalysisError("STALE_VERSION", "scan.path_denied");
  expect(scopeErrorText(error, fallbackText)).toBe(
    "The target or result changed. Review the scope again.",
  );
});
