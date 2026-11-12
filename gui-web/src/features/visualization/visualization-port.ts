import type {
  AnalysisClient,
  VizAvailabilityRequest,
  VizAvailabilityResponse,
  VizCatalog,
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
  catalog(): Promise<VizCatalog>;
  availability(
    request: VizAvailabilityRequest,
  ): Promise<VizAvailabilityResponse>;
  open(): Promise<VizViewChannel>;
  close(viewChannel: string): Promise<void>;
  groupRows(request: VizGroupRowsRequest): Promise<VizGroupRows>;
} {
  return {
    catalog: () => client.call("viz.catalog", {}),
    availability: (request) => client.call("viz.availability", request),
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
  let resyncGeneration = 0;
  let resyncPending = false;
  let resyncInFlight = false;
  let baseline: { jobRef: string; sequence: number } | undefined;
  function refresh() {
    if (closed || resyncPending || resyncInFlight) return;
    const state = controller.getSnapshot();
    const job = state.phase === "job" ? state.job : null;
    if (closed || !job) return;
    if (reading) {
      queued = true;
      return;
    }
    reading = true;
    const generation = resyncGeneration;
    void client
      .call("job.get", { jobRef: job.jobRef })
      .then((status) => {
        if (closed || generation !== resyncGeneration) return;
        if (
          baseline?.jobRef === job.jobRef &&
          status.sequence < baseline.sequence
        )
          return;
        void controller.completeJob(status);
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
  function resync() {
    if (closed || resyncInFlight || !resyncPending) return;
    resyncPending = false;
    resyncInFlight = true;
    const generation = resyncGeneration;
    const alive = () => !closed && generation === resyncGeneration;
    void client
      .call("job.list", {})
      .then((rows) => {
        if (!alive()) return;
        const jobRef = controller.getSnapshot().job?.jobRef;
        const row = rows.find((item) => item.jobRef === jobRef);
        baseline = row
          ? { jobRef: row.jobRef, sequence: row.sequence }
          : undefined;
      })
      .catch(() => {
        if (alive()) resyncPending = true;
      })
      .finally(() => {
        resyncInFlight = false;
        if (alive()) refresh();
        else resync();
      });
    void controller.recheckVisibleModels(alive);
  }
  const stopState = controller.subscribe(refresh);
  const stopEvents = client.subscribe((event) => {
    if (event.topic === "job.resync_required") {
      resyncGeneration += 1;
      resyncPending = true;
      resync();
      return;
    }
    const job = controller.getSnapshot().job;
    if (job && event.jobRef === job.jobRef && event.topic.startsWith("job."))
      refresh();
  });
  const timer = globalThis.setInterval(() => {
    if (resyncPending) resync();
    else refresh();
  }, 500);
  refresh();
  return () => {
    closed = true;
    stopState();
    stopEvents();
    globalThis.clearInterval(timer);
  };
}
