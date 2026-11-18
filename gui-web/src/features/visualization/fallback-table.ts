import type { VizRenderResponse, VizRows, VizRowsRequest } from "../../ipc";

/** The model whose rows stand in for a chart the engine cannot draw. */
export type FallbackReference = { modelRef: string; resultRevision: string };

/**
 * The fallback table of the current answer, or null. Only an unavailable
 * answer that names a fallback of its own result revision has one; a
 * fallback pinned to another revision is ignored.
 */
export function fallbackReference(state: {
  phase: string;
  response: VizRenderResponse | null;
}): FallbackReference | null {
  const answer = state.response;
  if (
    state.phase !== "unavailable" ||
    answer?.kind !== "unavailable" ||
    !answer.fallback ||
    answer.fallback.resultRevision !== answer.resultRevision
  )
    return null;
  return answer.fallback;
}

/** Whether a page of fallback rows belongs to the request that asked for it. */
export function fallbackRowsMatch(
  request: VizRowsRequest,
  response: VizRows,
): boolean {
  return (
    response.resultRevision === request.resultRevision &&
    response.rows.every(
      (row) =>
        (row.itemRef === null) !== (row.groupRef === null) &&
        (row.itemRef === null ||
          row.itemRef.resultVersion === request.resultRevision),
    )
  );
}
