import { useCallback, useEffect, useRef, useState } from "react";
import { AnalysisError } from "../../ipc";
import type {
  AnalysisClient,
  RecentScopeList,
  RecentScopeListRow,
  RecentScopeOpen,
} from "../../ipc";

type ListState =
  | { kind: "idle" }
  | { kind: "loading" }
  | { kind: "ready"; value: RecentScopeList }
  | { kind: "error"; message: string };

type DetailState =
  | { kind: "idle" }
  | { kind: "loading"; row: RecentScopeListRow }
  | { kind: "ready"; row: RecentScopeListRow; value: RecentScopeOpen }
  | {
      kind: "error";
      row: RecentScopeListRow;
      message: string;
      missing: boolean;
    };

/** Owns recent-scope requests and ignores responses made obsolete by later choices. */
export function useRecentScopes(
  client: AnalysisClient,
  nextRequestSeq: () => number,
  isRequestSeqCurrent: (requestSeq: number) => boolean,
) {
  const [list, setList] = useState<ListState>({ kind: "idle" });
  const [detail, setDetail] = useState<DetailState>({ kind: "idle" });
  const request = useRef(0);
  const mounted = useRef(true);

  useEffect(() => {
    mounted.current = true;
    return () => {
      mounted.current = false;
      request.current += 1;
    };
  }, []);

  const loadList = useCallback(async () => {
    const sequence = ++request.current;
    setList({ kind: "loading" });
    setDetail({ kind: "idle" });
    try {
      const value = await client.call("scope.recent_list", {});
      if (mounted.current && request.current === sequence)
        setList({ kind: "ready", value });
    } catch {
      if (mounted.current && request.current === sequence)
        setList({
          kind: "error",
          message: "Recent analysis ranges could not be loaded.",
        });
    }
  }, [client]);

  const openRow = useCallback(
    async (row: RecentScopeListRow) => {
      const sequence = ++request.current;
      const requestSeq = nextRequestSeq();
      setDetail({ kind: "loading", row });
      try {
        const value = await client.call("scope.recent_open", {
          recentScopeRef: row.recentScopeRef,
          requestSeq,
        });
        if (mounted.current && request.current === sequence) {
          if (!isRequestSeqCurrent(requestSeq)) {
            setDetail({
              kind: "error",
              row,
              missing: false,
              message: "The scope changed. Check the saved range again.",
            });
            return;
          }
          if (value.review && value.review.requestSeq !== requestSeq) {
            setDetail({
              kind: "error",
              row,
              missing: false,
              message: "The scope review changed. Open the saved range again.",
            });
          } else setDetail({ kind: "ready", row, value });
        }
      } catch (error) {
        if (
          mounted.current &&
          request.current === sequence &&
          !isRequestSeqCurrent(requestSeq)
        ) {
          setDetail({
            kind: "error",
            row,
            missing: false,
            message: "The scope changed. Check the saved range again.",
          });
        } else if (mounted.current && request.current === sequence)
          setDetail({
            kind: "error",
            row,
            missing:
              error instanceof AnalysisError && error.code === "NOT_FOUND",
            message:
              error instanceof AnalysisError && error.code === "NOT_FOUND"
                ? "This saved range is no longer available. Refresh the list and choose another range."
                : "This saved range could not be opened. It may be damaged or from an unsupported version.",
          });
      }
    },
    [client, isRequestSeqCurrent, nextRequestSeq],
  );

  const invalidate = useCallback(() => {
    request.current += 1;
    setDetail({ kind: "idle" });
  }, []);

  return { list, detail, loadList, openRow, invalidate };
}
