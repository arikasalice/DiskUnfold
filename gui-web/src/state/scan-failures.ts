import { useCallback, useEffect, useRef, useState } from "react";
import type { AnalysisClient, ScanFailureKind, ScanFailures } from "../ipc";

/** Pages within a single displayed result and filter. Remount when either changes. */
export function useScanFailures(
  client: AnalysisClient,
  resultVersion: string,
  kind?: ScanFailureKind,
) {
  const [page, setPage] = useState<ScanFailures>();
  const [pending, setPending] = useState(true);
  const [error, setError] = useState<string>();
  const inFlight = useRef(false);
  const generation = useRef(0);

  const read = useCallback(
    async (cursor?: string) => {
      if (inFlight.current) return;
      inFlight.current = true;
      const request = ++generation.current;
      setPending(true);
      setError(undefined);
      try {
        const next = await client.call("scan.failures", {
          resultVersion,
          ...(kind && { kind }),
          ...(cursor && { cursor }),
        });
        if (generation.current !== request) return;
        if (
          next.resultVersion !== resultVersion ||
          (kind && next.rows.some((row) => row.kind !== kind))
        ) {
          throw new Error("The scan issues do not match the selected result.");
        }
        setPage((previous) =>
          cursor && previous
            ? { ...next, rows: [...previous.rows, ...next.rows] }
            : next,
        );
      } catch (failure) {
        if (generation.current === request) {
          setError(
            failure instanceof Error
              ? failure.message
              : "Scan issues could not be read.",
          );
        }
      } finally {
        if (generation.current === request) {
          inFlight.current = false;
          setPending(false);
        }
      }
    },
    [client, kind, resultVersion],
  );

  useEffect(() => {
    void read();
    return () => {
      generation.current += 1;
      inFlight.current = false;
    };
  }, [read]);

  return { page, pending, error, readMore: () => void read(page?.nextCursor) };
}
