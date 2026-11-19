import { useEffect, useState } from "react";
import type { VizRow, VizRows, VizRowsRequest } from "../../ipc";
import { matchingRowsPage } from "../../viz";
import type { ModelResponse } from "../../viz";

const NO_ROWS: readonly VizRow[] = [];

/**
 * The rows of every expanded folder of `frame`, read with `parentShapeRef`, so
 * the shapes drawn inside a folder get their names. The adapter matches these
 * rows to shapes by item or group reference, never by position.
 *
 * A folder is expanded when other shapes name it as their parent; its table
 * holds one row for each of those shapes. A read that fails, or a page that
 * does not belong to `frame`, leaves those shapes unnamed. Answers for an
 * earlier model are dropped, and a model committed again with the same folders
 * is not read again. The shared row state of the controller is not touched.
 */
export function useFolderRows(
  read: (request: VizRowsRequest) => Promise<VizRows>,
  graph: { frame: ModelResponse } | null | undefined,
): readonly VizRow[] {
  const frame = graph?.frame ?? null;
  const requests = frame ? folderRequests(frame) : [];
  const key = frame
    ? JSON.stringify([frame.observedAt, frame.completeness, requests])
    : "";
  const [loaded, setLoaded] = useState<{
    key: string;
    rows: readonly VizRow[];
  } | null>(null);
  useEffect(() => {
    if (!frame || requests.length === 0) return;
    let live = true;
    void Promise.allSettled(
      requests.map(async (request) => {
        const response = await read(request);
        return matchingRowsPage(frame, { request, response })
          ? response.rows
          : [];
      }),
    ).then((results) => {
      if (live)
        setLoaded({
          key,
          rows: results.flatMap((result) =>
            result.status === "fulfilled" ? result.value : [],
          ),
        });
    });
    return () => {
      live = false;
    };
    // The key holds everything the reads and their checks use from `frame`.
  }, [read, key]);
  return loaded?.key === key ? loaded.rows : NO_ROWS;
}

function folderRequests(frame: ModelResponse): VizRowsRequest[] {
  const children = new Map<string, number>();
  for (const shape of frame.model.shapes)
    if (shape.parentShapeRef)
      children.set(
        shape.parentShapeRef,
        (children.get(shape.parentShapeRef) ?? 0) + 1,
      );
  return [...children].map(([parentShapeRef, count]) => ({
    modelRef: frame.model.modelRef,
    resultRevision: frame.resultRevision,
    start: 0,
    count,
    parentShapeRef,
  }));
}
