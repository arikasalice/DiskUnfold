import type { BrowsePort } from "../features/browse/BrowsePanel";
import type { AnalysisClient, Id } from "../ipc";

const STATUS_INTERVAL_MS = 100;

/** Connects folder navigation to the desktop query and job commands. */
export function createBrowsePort(
  client: AnalysisClient,
): BrowsePort & { dispose(): void } {
  let disposed = false;
  let region: { id: Promise<Id>; closed: boolean } | undefined;
  let offset = 0;
  const closeRegion = (entry: NonNullable<typeof region>) => {
    if (entry.closed) return;
    entry.closed = true;
    if (region === entry) region = undefined;
    void entry.id
      .then((regionRef) => client.call("query.region.close", { regionRef }))
      .catch(() => {});
  };
  const getRegion = () => {
    region ??= {
      id: client
        .call("query.region.open", { kind: "browse_list" })
        .then(({ regionRef }) => regionRef),
      closed: false,
    };
    const entry = region;
    void entry.id.catch(() => {
      if (region === entry) region = undefined;
    });
    return entry;
  };
  async function dispatch(
    request: Parameters<BrowsePort["open"]>[0],
    signal?: AbortSignal,
  ) {
    signal?.throwIfAborted();
    if (disposed) throw new DOMException("Browse region closed", "AbortError");
    if (request.requestSeq - offset > 0xffff_ffff) {
      if (region) closeRegion(region);
      offset = request.requestSeq - 1;
    }
    const wireSeq = request.requestSeq - offset;
    const entry = getRegion();
    const regionRef = await entry.id;
    if (disposed || entry.closed)
      throw new DOMException("Browse region closed", "AbortError");
    signal?.throwIfAborted();
    const cancelRequest = () => {
      void client
        .call("query.cancel", {
          regionRef,
          target: { kind: "request", requestSeq: wireSeq },
        })
        .catch(() => {});
      // Closing also fences an open that has not reached server admission yet.
      closeRegion(entry);
    };
    signal?.addEventListener("abort", cancelRequest, { once: true });
    let response;
    try {
      response = await client.call("browse.open", {
        ...request,
        requestSeq: wireSeq,
        regionRef,
      });
    } finally {
      signal?.removeEventListener("abort", cancelRequest);
    }
    return { response, wireSeq, regionRef, entry };
  }
  return {
    dispose() {
      if (disposed) return;
      disposed = true;
      if (region) closeRegion(region);
    },
    page: (request) => client.call("query.page", request),
    async open(request, signal) {
      const { response, wireSeq, regionRef, entry } = await dispatch(
        request,
        signal,
      );
      if (response.kind === "opened") {
        if (entry.closed)
          throw new DOMException("Browse region closed", "AbortError");
        signal?.throwIfAborted();
        if (response.requestSeq !== wireSeq)
          throw new Error("The folder query sequence does not match.");
        return { ...response, requestSeq: request.requestSeq };
      }
      const jobRef = response.jobRef;
      const checkLive = () => {
        if (disposed || entry.closed)
          throw new DOMException("Browse region closed", "AbortError");
        signal?.throwIfAborted();
      };
      try {
        // A late receipt still owns its job, even if navigation was aborted.
        checkLive();
        if (
          response.requestSeq !== wireSeq ||
          (request.resultVersion != null &&
            response.resultVersion !== request.resultVersion)
        ) {
          throw new Error(
            "The folder query does not match the selected result.",
          );
        }
        const queryRef = await waitForQuery(client, jobRef, checkLive);
        checkLive();
        const page = await client.call("query.page", {
          queryRef,
          resultVersion: response.resultVersion,
          start: 0,
          count: request.pageSize ?? 200,
        });
        checkLive();
        if (
          page.rowKind !== "folder" ||
          page.queryRef !== queryRef ||
          page.resultVersion !== response.resultVersion
        ) {
          throw new Error("The completed query returned a different result.");
        }
        const { browseContext: context, ...folderPage } = page;
        if (!context) {
          throw new Error(
            "The completed folder query has no location context.",
          );
        }
        return {
          kind: "opened",
          requestSeq: request.requestSeq,
          ...context,
          page: folderPage,
        };
      } catch (error) {
        checkLive();
        throw error;
      } finally {
        if (entry.closed) {
          void client.call("job.cancel", { jobRef }).catch(() => {});
        } else if (signal?.aborted) {
          void client
            .call("query.cancel", {
              regionRef,
              target: { kind: "job", jobRef },
            })
            .catch(() => {});
        }
      }
    },
  };
}

async function waitForQuery(
  client: AnalysisClient,
  jobRef: Id,
  checkLive: () => void,
): Promise<Id> {
  for (;;) {
    checkLive();
    const status = await client.call("job.get", { jobRef });
    checkLive();
    if (status.jobRef !== jobRef || status.kind !== "query.evaluate") {
      throw new Error("The folder query status could not be verified.");
    }
    if (status.state === "done" && status.queryRef) return status.queryRef;
    if (
      status.state === "done" ||
      status.state === "partial" ||
      status.state === "failed" ||
      status.state === "cancelled"
    ) {
      throw new Error("The folder query did not complete.");
    }
    await new Promise<void>((resolve) =>
      setTimeout(resolve, STATUS_INTERVAL_MS),
    );
  }
}
