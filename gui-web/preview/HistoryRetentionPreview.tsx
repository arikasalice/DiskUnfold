import { useMemo } from "react";
import { AnalysisError } from "../src/ipc";
import type {
  AnalysisClient,
  HistoryRetentionApplyResponse,
  HistoryRetentionPolicy,
  HistoryRetentionReview,
  HistoryRetentionReviewRequest,
} from "../src/ipc";
import { HistoryPanel } from "../src/features/history/HistoryPanel";
import { LocaleTextContext } from "../src/i18n/presentation";
import { RegionalFormatContext } from "../src/i18n/regional-format";
import { regional } from "./BrowseScanStatusPreview";
import { historyListClient } from "./history-list-fixture";
import { koreanText } from "./result-summary-fixture";

const dateFrom = Date.UTC(2026, 0, 1, 9);
const dateTo = Date.UTC(2026, 1, 1, 17);

function initialPolicy(scene: string): HistoryRetentionPolicy {
  if (scene === "header")
    return {
      enabled: true,
      specifiedDays: 30,
      suggestedDays: 90,
      policyFingerprint: "preview-enabled-policy",
    };
  if (scene === "unreadable")
    return {
      enabled: false,
      suggestedDays: 90,
      policyFingerprint: "preview-unreadable-policy",
      unavailableReason: "history.policy_unreadable",
    };
  return {
    enabled: false,
    suggestedDays: 90,
    policyFingerprint: "preview-policy",
  };
}

function previewClient(scene: string): AnalysisClient {
  let currentPolicy = initialPolicy(scene);
  let reviews = 0;
  let applies = 0;
  const requests = new Map<string, HistoryRetentionReviewRequest>();

  return {
    call: (async (name: string, body: unknown) => {
      if (name === "history.list")
        return historyListClient.call("history.list", body as never);
      if (name === "history.get")
        return historyListClient.call("history.get", body as never);
      if (name === "history.items")
        return historyListClient.call("history.items", body as never);
      if (name === "history.retention_get") return currentPolicy;
      if (name === "history.retention_review") {
        if (scene === "count-unavailable")
          throw new AnalysisError("INTERNAL", "history.count_unavailable");
        const request = body as HistoryRetentionReviewRequest;
        reviews += 1;
        const changeKind = request.enabled
          ? currentPolicy.enabled
            ? request.days !== undefined &&
              request.days <
                (currentPolicy.specifiedDays ?? currentPolicy.suggestedDays)
              ? "shorten"
              : "extend"
            : "enable"
          : currentPolicy.enabled
            ? "disable"
            : "set_period";
        const targetCount =
          scene === "zero" || scene === "zero-target"
            ? "0"
            : scene === "stale" && reviews > 1
              ? "4"
              : scene === "stale"
                ? "2"
                : "3";
        const review: HistoryRetentionReview = {
          reviewRef: `preview-review-${reviews}`,
          changeKind,
          confirmationRequired:
            changeKind === "enable" || changeKind === "shorten",
          cleanupAfterApply:
            request.enabled &&
            (changeKind === "enable" || changeKind === "shorten"),
          targetCount,
          ...(targetCount !== "0" && {
            targetEarliestAtMs: dateFrom,
            targetLatestAtMs: dateTo,
          }),
          heldCount: targetCount === "0" ? "0" : "2",
          undoInfoRemovalCount: targetCount === "0" ? "0" : "1",
          fingerprint: `preview-fingerprint-${reviews}`,
        };
        requests.set(review.reviewRef, request);
        return review;
      }
      if (name === "history.retention_apply") {
        applies += 1;
        if (scene === "stale" && applies === 1)
          throw new AnalysisError("STALE_VERSION", "history.review_stale");
        if (scene === "save-failure")
          throw new AnalysisError("INTERNAL", "history.policy_store_failed");
        const { reviewRef } = body as { reviewRef: string };
        const request = requests.get(reviewRef);
        if (!request) throw new Error("Unknown gallery review");
        currentPolicy = {
          ...currentPolicy,
          enabled: request.enabled,
          ...(request.days !== undefined && { specifiedDays: request.days }),
          policyFingerprint: `preview-applied-${applies}`,
        };
        const response: HistoryRetentionApplyResponse = {
          outcome: "applied",
          policy: currentPolicy,
        };
        return response;
      }
      if (name === "history.retention_cancel") return { outcome: "cancelled" };
      throw new Error("The gallery has no answer for " + name);
    }) as AnalysisClient["call"],
    subscribe: () => () => undefined,
  };
}

/** Product history screen with synthetic retention responses for gallery captures. */
export function HistoryRetentionPreview({ scene }: { scene: string }) {
  const client = useMemo(() => previewClient(scene), [scene]);
  return (
    <LocaleTextContext.Provider value={koreanText}>
      <RegionalFormatContext.Provider value={regional}>
        <HistoryPanel client={client} onClose={() => undefined} />
      </RegionalFormatContext.Provider>
    </LocaleTextContext.Provider>
  );
}
