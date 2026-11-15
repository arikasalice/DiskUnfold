import type { VizRows, VizRowsRequest } from "../ipc";

/** The most rows one `viz.rows` call may return; the published page limit. */
export const MAX_ROWS_PER_READ = 1000;

/**
 * Reads `request.count` rows in as many calls as the page limit needs and joins
 * them into one answer. All pages must describe the same result: a page whose
 * revision, observation, completeness or total differs fails the whole read, so
 * names never mix two models. A read ends early when the rows run out.
 */
export async function readRowsPages(
  read: (request: VizRowsRequest) => Promise<VizRows>,
  request: VizRowsRequest,
): Promise<VizRows> {
  if (request.count <= MAX_ROWS_PER_READ) return read(request);
  const first = await read({ ...request, count: MAX_ROWS_PER_READ });
  const rows = [...first.rows];
  while (
    rows.length < request.count &&
    request.start + rows.length < first.totalCount
  ) {
    const page = await read({
      ...request,
      start: request.start + rows.length,
      count: Math.min(MAX_ROWS_PER_READ, request.count - rows.length),
    });
    if (
      page.resultRevision !== first.resultRevision ||
      page.observedAt !== first.observedAt ||
      page.completeness !== first.completeness ||
      page.totalCount !== first.totalCount
    )
      throw new Error("Rows changed between pages");
    if (page.rows.length === 0) break;
    rows.push(...page.rows);
  }
  return { ...first, rows };
}
