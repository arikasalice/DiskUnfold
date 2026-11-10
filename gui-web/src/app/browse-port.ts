import type { BrowsePort } from "../features/browse/BrowsePanel";
import type { AnalysisClient, Id } from "../ipc";

const STATUS_INTERVAL_MS = 100;

/** Connects folder navigation to the desktop query and job commands. */
export function createBrowsePort(client: AnalysisClient): BrowsePort {
  return {
    page: (request) => client.call("query.page", request),
    async open(request, signal) {
      const response = await client.call("browse.open", request);
      if (response.kind === "opened") return response;
      const jobRef = response.jobRef;
      try {
        if (
          response.requestSeq !== request.requestSeq ||
          !request.resultVersion
        ) {
          throw new Error(
            "The folder query does not match the selected result.",
          );
        }
        const queryRef = await waitForQuery(client, jobRef, signal);
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
        // The cached open supplies the location and breadcrumbs omitted from a page.
        const opened = await client.call("browse.open", request);
        if (opened.kind === "job") {
          void client
            .call("job.cancel", { jobRef: opened.jobRef })
            .catch(() => {});
          throw new Error("The completed folder query expired.");
        }
        signal?.throwIfAborted();
        if (
          opened.requestSeq !== request.requestSeq ||
          opened.page.queryRef !== queryRef ||
          opened.page.resultVersion !== page.resultVersion
        ) {
          throw new Error("The completed folder query changed before opening.");
        }
        return { ...opened, page };
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
