import type { VizRenderRequest } from "../ipc";

/** Explicit conditions for one pinned F06 view. Channel and sequence are lifecycle-owned. */
export type VisualizationViewRequest = Omit<
  VizRenderRequest,
  "viewChannel" | "requestSeq" | "resultRevision"
> & { resultRevision: NonNullable<VizRenderRequest["resultRevision"]> };
