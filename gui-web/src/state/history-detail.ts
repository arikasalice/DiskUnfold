import { useCallback, useEffect, useRef, useState } from "react";
import { AnalysisError } from "../ipc";
import type { AnalysisClient, HistoryDetail } from "../ipc";

/** Read one history detail, ignoring responses after its request is superseded. */
export function useHistoryDetail(client: AnalysisClient, jobRef: string) {
  const [detail, setDetail] = useState<HistoryDetail>();
  const [pending, setPending] = useState(true);
  const [error, setError] = useState<"removed" | "storage">();
  const generation = useRef(0);
  const busy = useRef(false);

  const read = useCallback(async () => {
    if (busy.current) return;
    busy.current = true;
    const request = ++generation.current;
    setPending(true);
    setError(undefined);
    try {
      const result = await client.call("history.get", { jobRef });
      if (generation.current === request) setDetail(result);
    } catch (failure) {
      if (generation.current === request) {
        setDetail(undefined);
        setError(
          failure instanceof AnalysisError &&
            failure.reasonKey === "history.removed"
            ? "removed"
            : "storage",
        );
      }
    } finally {
      if (generation.current === request) {
        busy.current = false;
        setPending(false);
      }
    }
  }, [client, jobRef]);

  useEffect(() => {
    setDetail(undefined);
    void read();
    return () => {
      generation.current += 1;
      busy.current = false;
    };
  }, [read]);
  return { detail, pending, error, retry: () => void read() };
}
