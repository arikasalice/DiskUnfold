import type {
  AnalysisClient,
  VizGroupRows,
  VizGroupRowsRequest,
  VizViewChannel,
} from "../../ipc";
import type { VisualizationController, VisualizationPort } from "./controller";

/** Typed client adapter. The host must register the published commands first. */
export function createVisualizationPort(
  client: AnalysisClient,
  canDisplay: VisualizationPort["canDisplay"],
): VisualizationPort & {
  open(): Promise<VizViewChannel>;
  close(viewChannel: string): Promise<void>;
  groupRows(request: VizGroupRowsRequest): Promise<VizGroupRows>;
} {
  return {
    open: () => client.call("viz.view.open", {}),
    async close(viewChannel) {
      await client.call("viz.view.close", { viewChannel });
    },
    render: (request) => client.call("viz.render", request),
    cancel: (request) => client.call("viz.cancel", request),
    rows: (request) => client.call("viz.rows", request),
    // Copy the flat request so an async transport sees the call-time values.
    groupRows: (request) => client.call("viz.group_rows", { ...request }),
    canDisplay,
  };
}

/** Re-reads F24 status after events and polls so a lost event cannot strand the view. */
export function observeVisualizationJob(
  controller: VisualizationController,
  client: AnalysisClient,
): () => void {
  let closed = false;
  let reading = false;
  let queued = false;
  function refresh() {
    const state = controller.getSnapshot();
    const job = state.phase === "job" ? state.job : null;
    if (closed || !job) return;
    if (reading) {
      queued = true;
      return;
    }
    reading = true;
    void client
      .call("job.get", { jobRef: job.jobRef })
      .then((status) => {
        if (!closed) void controller.completeJob(status);
      })
      .catch(() => {})
      .finally(() => {
        reading = false;
        if (queued && !closed) {
          queued = false;
          refresh();
        }
      });
  }
  const stopState = controller.subscribe(refresh);
  const stopEvents = client.subscribe((event) => {
    const job = controller.getSnapshot().job;
    if (
      event.topic === "job.resync_required" ||
      (job && event.jobRef === job.jobRef && event.topic.startsWith("job."))
    )
      refresh();
  });
  const timer = globalThis.setInterval(refresh, 500);
  refresh();
  return () => {
    closed = true;
    stopState();
    stopEvents();
    globalThis.clearInterval(timer);
  };
}
