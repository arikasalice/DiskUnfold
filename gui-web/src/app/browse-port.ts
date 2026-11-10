import type { BrowsePort } from "../features/browse/BrowsePanel";
import type { AnalysisClient, Id } from "../ipc";

const STATUS_INTERVAL_MS = 100;

/** Connects folder navigation to the desktop query and job commands. */
export function createBrowsePort(client: AnalysisClient): BrowsePort {
  return {
    page: (request) => client.call("query.page", request),
    async open(request, signal) {
      signal?.throwIfAborted();
      const response = await client.call("browse.open", request);
      if (response.kind === "opened") {
        signal?.throwIfAborted();
        return response;
      }
      const jobRef = response.jobRef;
      try {
        // A late receipt still owns its job, even if navigation was aborted.
        signal?.throwIfAborted();
        if (
          response.requestSeq !== request.requestSeq ||
          !request.resultVersion
        ) {
          throw new Error(
            "The folder query does not match the selected result.",
          );
        }
        const queryRef = await waitForQuery(client, jobRef, signal);
        signal?.throwIfAborted();
        const page = await client.call("query.page", {
          queryRef,
          resultVersion: request.resultVersion,
          start: 0,
          count: request.pageSize ?? 200,
        });
        signal?.throwIfAborted();
        if (
          page.rowKind !== "folder" ||
          page.queryRef !== queryRef ||
          page.resultVersion !== request.resultVersion
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
      } finally {
        if (signal?.aborted) {
          void client.call("job.cancel", { jobRef }).catch(() => {});
        }
      }
    },
  };
}

async function waitForQuery(
  client: AnalysisClient,
  jobRef: Id,
  signal?: AbortSignal,
): Promise<Id> {
  for (;;) {
    signal?.throwIfAborted();
    const status = await client.call("job.get", { jobRef });
    signal?.throwIfAborted();
    if (status.jobRef !== jobRef || status.kind !== "query.evaluate") {
      throw new Error("The folder query status could not be verified.");
    }
    if (status.state === "done" && status.resultVersion)
      return status.resultVersion;
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
