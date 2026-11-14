import { useCallback, useEffect, useRef, useState } from "react";
import type { AnalysisClient } from "../ipc";
import { isSampleScreen, useSampleSession } from "../state/sample-session";

/**
 * Joins the sample session to the analysis screen. The sample result screen
 * replaces the real analysis only while it is active (or being reset); before
 * that, and when the sample is unavailable, the analysis screen stays and the
 * entry button shows the state. After a switch to real analysis it remounts
 * the analysis screen (through a new key) so it opens with an empty scope
 * draft.
 *
 * `sampleOpen` means the sample result screen is shown.
 */
export function useSampleFlow(client: AnalysisClient | undefined) {
  const sample = useSampleSession(client);
  const sampleOpen = isSampleScreen(sample.session, sample.reached);
  const [analysisKey, setAnalysisKey] = useState(0);
  const [focusRequest, setFocusRequest] = useState(0);
  const analysisSlot = useRef<HTMLDivElement>(null);
  const entryRef = useRef<HTMLButtonElement>(null);
  const wantFocus = useRef(false);
  const closeSample = sample.close;
  const { pending, session } = sample;

  // The entry button is mounted again by a switch to real analysis, so the
  // focus is given after the render that follows the request.
  useEffect(() => {
    if (!wantFocus.current || pending !== undefined || sampleOpen) return;
    const entry = entryRef.current;
    const target = entry && !entry.disabled ? entry : analysisSlot.current;
    target?.focus();
    wantFocus.current = false;
  }, [focusRequest, pending, sampleOpen, session]);

  /** Returns the focus to the entry button once the sample is gone (the analysis screen when it has none). */
  const focusEntry = useCallback(() => {
    wantFocus.current = true;
    setFocusRequest((count) => count + 1);
  }, []);

  /** Called after the session was released; nothing of the sample is passed on. */
  const startRealAnalysis = useCallback(() => {
    setAnalysisKey((key) => key + 1);
  }, []);

  return {
    sample,
    sampleOpen,
    analysisKey,
    analysisSlot,
    entryRef,
    /** Releases the session; resolves true once it is gone. */
    endSample: closeSample,
    startRealAnalysis,
    focusEntry,
  };
}
