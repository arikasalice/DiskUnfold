import { useCallback, useEffect, useRef, useState } from "react";
import { AnalysisError } from "../ipc";
import type {
  AnalysisClient,
  HistoryChangedBody,
  HistoryListFilter,
  HistoryListResponse,
  HistoryListRevision,
} from "../ipc";

function decimal(value: unknown): value is string {
  return typeof value === "string" && /^(0|[1-9]\d{0,19})$/.test(value);
}

function changedRevision(body: unknown): HistoryListRevision | undefined {
  const revision = (body as Partial<HistoryChangedBody> | null)?.revision;
  if (!decimal(revision?.deleteGeneration) || !decimal(revision.maxRecordSeq))
    return undefined;
  return revision;
}

function isNewer(next: HistoryListRevision, known?: HistoryListRevision) {
  if (!known) return true;
  const generation = BigInt(next.deleteGeneration);
  const previous = BigInt(known.deleteGeneration);
  return (
    generation > previous ||
    (generation === previous &&
      BigInt(next.maxRecordSeq) > BigInt(known.maxRecordSeq))
  );
}

/** Keeps cursor pages bound to their filter and refreshes at committed revisions. */
export function useHistory(client: AnalysisClient, filter: HistoryListFilter) {
  const [page, setPage] = useState<HistoryListResponse>();
  const [pending, setPending] = useState(true);
  const [error, setError] = useState(false);
  const [refreshed, setRefreshed] = useState(false);
  const generation = useRef(0);
  const inFlight = useRef(false);
  const knownRevision = useRef<HistoryListRevision>(undefined);

  const read = useCallback(
    async (cursor: string | null) => {
      if (cursor && inFlight.current) return;
      inFlight.current = true;
      const request = ++generation.current;
      setPending(true);
      setError(false);
      try {
        const next = await client.call("history.list", {
          filter,
          cursor,
          count: null,
        });
        if (generation.current !== request) return;
        if (
          knownRevision.current &&
          isNewer(knownRevision.current, next.revision)
        ) {
          setPage(undefined);
          setError(true);
          return;
        }
        if (isNewer(next.revision, knownRevision.current))
          knownRevision.current = next.revision;
        setPage((previous) =>
          cursor && previous
            ? { ...next, rows: [...previous.rows, ...next.rows] }
            : next,
        );
      } catch (failure) {
        if (generation.current !== request) return;
        if (
          cursor &&
          failure instanceof AnalysisError &&
          failure.reasonKey === "history.version_changed"
        ) {
          setPage(undefined);
          setRefreshed(true);
          void read(null);
          return;
        }
        setError(true);
      } finally {
        if (generation.current === request) {
          inFlight.current = false;
          setPending(false);
        }
      }
    },
    [client, filter],
  );

  useEffect(() => {
    setPage(undefined);
    setRefreshed(false);
    knownRevision.current = undefined;
    let frame: number | undefined;
    const unsubscribe = client.subscribe((event) => {
      if (event.topic === "history.changed") {
        const revision = changedRevision(event.body);
        if (!revision || !isNewer(revision, knownRevision.current)) return;
        knownRevision.current = revision;
      } else if (event.topic !== "job.resync_required") {
        return;
      }
      generation.current += 1;
      setPage(undefined);
      setPending(true);
      setError(false);
      setRefreshed(true);
      if (frame !== undefined) return;
      frame = window.requestAnimationFrame(() => {
        frame = undefined;
        void read(null);
      });
    });
    void read(null);
    return () => {
      unsubscribe();
      if (frame !== undefined) window.cancelAnimationFrame(frame);
      generation.current += 1;
      inFlight.current = false;
    };
  }, [client, read]);

  return {
    page,
    pending,
    error,
    refreshed,
    retry: () => void read(null),
    more: () => {
      if (page?.nextCursor) void read(page.nextCursor);
    },
  };
}
