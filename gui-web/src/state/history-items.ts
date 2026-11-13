import { useCallback, useEffect, useRef, useState } from "react";
import { AnalysisError } from "../ipc";
import type {
  AnalysisClient,
  HistoryItemOutcome,
  HistoryItemsResponse,
} from "../ipc";
export function useHistoryItems(
  client: AnalysisClient,
  jobRef: string,
  outcome: HistoryItemOutcome | null,
) {
  const [page, setPage] = useState<HistoryItemsResponse>();
  const [pending, setPending] = useState(true);
  const [error, setError] = useState<"removed" | "storage">();
  const [refreshed, setRefreshed] = useState(false);
  const current = useRef<HistoryItemsResponse>(undefined);
  const generation = useRef(0);
  const busy = useRef(false);
  const read = useCallback(
    async (cursor: string | null) => {
      if (busy.current) return;
      busy.current = true;
      const request = ++generation.current;
      setPending(true);
      setError(undefined);
      try {
        const next = await client.call("history.items", {
          jobRef,
          outcome,
          cursor,
          count: null,
        });
        if (generation.current !== request) return;
        const previous = current.current;
        if (
          cursor &&
          previous &&
          (previous.revision.deleteGeneration !==
            next.revision.deleteGeneration ||
            previous.revision.maxRecordSeq !== next.revision.maxRecordSeq)
        ) {
          current.current = undefined;
          setPage(undefined);
          setRefreshed(true);
          busy.current = false;
          void read(null);
          return;
        }
        const combined =
          cursor && previous
            ? {
                ...next,
                rows: [...previous.rows, ...next.rows],
                completeness:
                  previous.completeness === "partial" ||
                  next.completeness === "partial"
                    ? ("partial" as const)
                    : next.completeness,
                missingReasons: [
                  ...new Set([
                    ...previous.missingReasons,
                    ...next.missingReasons,
                  ]),
                ],
              }
            : next;
        current.current = combined;
        setPage(combined);
      } catch (failure) {
        if (generation.current !== request) return;
        if (
          cursor &&
          failure instanceof AnalysisError &&
          (failure.code === "STALE_VERSION" ||
            failure.reasonKey === "history.version_changed")
        ) {
          current.current = undefined;
          setPage(undefined);
          setRefreshed(true);
          busy.current = false;
          void read(null);
          return;
        }
        if (
          failure instanceof AnalysisError &&
          failure.reasonKey === "history.removed"
        ) {
          current.current = undefined;
          setPage(undefined);
          setError("removed");
        } else setError("storage");
      } finally {
        if (generation.current === request) {
          busy.current = false;
          setPending(false);
        }
      }
    },
    [client, jobRef, outcome],
  );
  useEffect(() => {
    current.current = undefined;
    setPage(undefined);
    setRefreshed(false);
    void read(null);
    return () => {
      generation.current += 1;
      busy.current = false;
    };
  }, [read]);
  return {
    page,
    pending,
    error,
    refreshed,
    more: () => {
      if (current.current?.nextCursor) void read(current.current.nextCursor);
    },
    retry: () => void read(null),
  };
}
