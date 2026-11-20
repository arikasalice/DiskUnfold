import { useEffect, useState } from "react";
import type { SpaceTotal } from "../../ipc";
import { hasDedupCapacityLowerBound } from "./browse-cells";

/** Reads the pinned result's shared size limits once for the opened browse result. */
export function useBrowseDedupLimit(
  readTotal: ((resultVersion: string) => Promise<SpaceTotal>) | undefined,
  resultVersion: string | undefined,
): boolean {
  const [limit, setLimit] = useState<
    { resultVersion: string; value: boolean } | undefined
  >();
  useEffect(() => {
    setLimit(undefined);
    if (!resultVersion || !readTotal) return;
    let current = true;
    void readTotal(resultVersion)
      .then((total) => {
        if (current && total.resultVersion === resultVersion)
          setLimit({
            resultVersion,
            value: hasDedupCapacityLowerBound(total),
          });
      })
      .catch(() => {});
    return () => {
      current = false;
    };
  }, [readTotal, resultVersion]);
  return (
    limit !== undefined && limit.resultVersion === resultVersion && limit.value
  );
}
