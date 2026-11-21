import type { AnalysisClient, Id } from "../ipc";
import type {
  TypeListRequest,
  TypeListSource,
} from "../features/visualization/TypeList";
import { waitForQuery } from "./browse-port";

/** Rows per page of the file list (F06 README 46.13). */
export const TYPE_LIST_PAGE = 200;

export type TypeListPort = TypeListSource & { dispose(): void };

/**
 * Lists the files of one type below a folder with query.run (F04 IF-01, #306)
 * in a search region of its own. A request that outlives the engine's wait
 * continues as a query.evaluate job, read from its query reference.
 */
export function createTypeListPort(client: AnalysisClient): TypeListPort {
  let region: Promise<Id> | undefined;
  let requestSeq = 0;
  const regionRef = () => {
    const opened = (region ??= client
      .call("query.region.open", { kind: "search" })
      .then(({ regionRef }) => regionRef));
    opened.catch(() => {
      if (region === opened) region = undefined;
    });
    return opened;
  };
  return {
    async open(request, signal) {
      const ref = await regionRef();
      signal.throwIfAborted();
      const sequence = ++requestSeq;
      let job: Id | undefined;
      const cancel = () => {
        void client
          .call("query.cancel", {
            regionRef: ref,
            target: job
              ? { kind: "job", jobRef: job }
              : { kind: "request", requestSeq: sequence },
          })
          .catch(() => {});
      };
      signal.addEventListener("abort", cancel, { once: true });
      try {
        const [response, scopePath] = await Promise.all([
          client.call("query.run", {
            regionRef: ref,
            requestSeq: sequence,
            resultVersion: request.resultVersion,
            scope: request.scope,
            condition: request.condition,
            sortKeys: request.sortKeys,
            pageSize: TYPE_LIST_PAGE,
          }),
          scopeDisplayPath(client, request),
        ]);
        signal.throwIfAborted();
        if (
          response.requestSeq !== sequence ||
          response.resultVersion !== request.resultVersion
        )
          throw new Error("The file list does not match the selected result.");
        if (response.kind === "opened")
          return { page: response.page, scopePath };
        job = response.jobRef;
        const queryRef = await waitForQuery(client, job, () =>
          signal.throwIfAborted(),
        );
        const page = await client.call("query.page", {
          queryRef,
          resultVersion: response.resultVersion,
          start: 0,
          count: TYPE_LIST_PAGE,
        });
        signal.throwIfAborted();
        if (page.rowKind !== "search" || page.queryRef !== queryRef)
          throw new Error("The completed list returned a different result.");
        return { page, scopePath };
      } finally {
        signal.removeEventListener("abort", cancel);
      }
    },
    async next(cursor) {
      const page = await client.call("query.page", { nextCursor: cursor });
      if (page.rowKind !== "search")
        throw new Error("The file list continued with a different result.");
      return page;
    },
    dispose() {
      const closing = region;
      region = undefined;
      void closing
        ?.then((regionRef) => client.call("query.region.close", { regionRef }))
        .catch(() => {});
    },
  };
}

async function scopeDisplayPath(
  client: AnalysisClient,
  request: TypeListRequest,
): Promise<string | null> {
  try {
    const detail = await client.call("browse.detail", {
      resultVersion: request.resultVersion,
      item: request.scope,
    });
    return detail.displayPath.state === "present"
      ? detail.displayPath.value
      : null;
  } catch {
    return null;
  }
}
