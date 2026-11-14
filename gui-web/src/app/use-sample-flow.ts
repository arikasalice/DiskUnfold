import { useCallback, useEffect, useRef, useState } from "react";
import type { AnalysisClient } from "../ipc";
import { isOpenSample, useSampleSession } from "../state/sample-session";

/**
 * Joins the sample session to the analysis screen: it hides the real analysis
 * while a sample is open, and after a switch to real analysis remounts that
 * screen (through a new key) so it opens with an empty scope draft.
 */
export function useSampleFlow(client: AnalysisClient | undefined) {
  const sample = useSampleSession(client);
  const sampleOpen = isOpenSample(sample.session);
  const [analysisKey, setAnalysisKey] = useState(0);
  const analysisSlot = useRef<HTMLDivElement>(null);
  const focusAnalysis = useRef(false);
  const closeSample = sample.close;

  useEffect(() => {
    if (!sampleOpen && focusAnalysis.current) {
      analysisSlot.current?.focus();
      focusAnalysis.current = false;
    }
  }, [sampleOpen]);

  /** Releases the session; resolves true once it is gone. */
  const endSample = useCallback(async () => {
    focusAnalysis.current = true;
    const ended = await closeSample();
    if (!ended) focusAnalysis.current = false;
    return ended;
  }, [closeSample]);

  /** Called after the session was released; nothing of the sample is passed on. */
  const startRealAnalysis = useCallback(() => {
    setAnalysisKey((key) => key + 1);
    focusAnalysis.current = true;
  }, []);

  return {
    sample,
    sampleOpen,
    analysisKey,
    analysisSlot,
    endSample,
    startRealAnalysis,
  };
}
