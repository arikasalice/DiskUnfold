import { useEffect, useLayoutEffect, useRef, useState } from "react";
import type { AnalysisClient, HistoryRetentionPolicy } from "../../ipc";
import { isImeProcessingKey } from "../../components";
import type { RetentionPanelError } from "./history-retention-model";
import { useHistoryRetentionReview } from "./useHistoryRetentionReview";

/** Owns the history retention panel and connects its review controller. */
export function useHistoryRetention(client: AnalysisClient) {
  const [policy, setPolicy] = useState<HistoryRetentionPolicy>();
  const [draftDays, setDraftDays] = useState("");
  const [loading, setLoading] = useState(true);
  const [isOpen, setIsOpen] = useState(false);
  const [panelError, setPanelError] = useState<RetentionPanelError>();
  const triggerRef = useRef<HTMLButtonElement>(null);
  const titleRef = useRef<HTMLHeadingElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);
  const mounted = useRef(true);
  const review = useHistoryRetentionReview(client, {
    policy,
    draftDays,
    setPolicy,
    setDraftDays,
    setPanelError,
    inputRef,
    mounted,
  });

  useEffect(() => {
    mounted.current = true;
    let current = true;
    void client
      .call("history.retention_get", {})
      .then((value) => {
        if (!current) return;
        setPolicy(value);
        setDraftDays(String(value.specifiedDays ?? value.suggestedDays));
      })
      .catch(() => {
        if (current) setPanelError("load_failed");
      })
      .finally(() => {
        if (current) setLoading(false);
      });
    return () => {
      current = false;
      mounted.current = false;
    };
  }, [client]);

  useLayoutEffect(() => {
    if (isOpen) titleRef.current?.focus({ preventScroll: true });
  }, [isOpen]);

  useEffect(() => {
    if (!isOpen || review.activeReview) return;
    const closeOnEscape = (event: KeyboardEvent) => {
      if (event.key !== "Escape" || isImeProcessingKey(event) || review.busy)
        return;
      event.preventDefault();
      event.stopPropagation();
      setIsOpen(false);
      triggerRef.current?.focus();
    };
    window.addEventListener("keydown", closeOnEscape);
    return () => window.removeEventListener("keydown", closeOnEscape);
  }, [isOpen, review.activeReview, review.busy]);

  function closePanel() {
    review.cancelActiveReview();
    setIsOpen(false);
    triggerRef.current?.focus();
  }

  return {
    policy,
    draftDays,
    loading,
    busy: review.busy,
    isOpen,
    activeReview: review.activeReview,
    panelError,
    announcement: review.announcement,
    validationAttempted: review.validationAttempted,
    suggestionChanged: review.suggestionChanged,
    triggerRef,
    titleRef,
    inputRef,
    openPanel: () => setIsOpen(true),
    closePanel,
    setDraftDays: review.setDraftDays,
    changeEnabled: review.changeEnabled,
    applyPeriod: review.applyPeriod,
    confirmReview: review.confirmReview,
    cancelActiveReview: review.cancelActiveReview,
  };
}

export type RetentionController = ReturnType<typeof useHistoryRetention>;
