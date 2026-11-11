import type {
  AnalysisJobStatus,
  ItemRef,
  VizCancelRequest,
  VizCancelResponse,
  VizRenderRequest,
  VizRenderResponse,
  VizRows,
  VizRowsRequest,
} from "../../ipc";
import { adaptVizModel, matchingRowsPage } from "./model-adapter";
import type {
  ModelResponse,
  ModelRowsPage,
  VizSelection,
} from "./model-adapter";

type View = Pick<VizRenderRequest, "viewport" | "visibleWindow">;
type Committed = { frame: ModelResponse; view: View };
export type VisualizationPort = {
  render(request: VizRenderRequest): Promise<VizRenderResponse>;
  cancel(request: VizCancelRequest): Promise<VizCancelResponse>;
  rows(request: VizRowsRequest): Promise<VizRows>;
  /** True only when the whole model is supported by the chosen renderer. */
  canDisplay(frame: ModelResponse): boolean;
};
export type Reconciled = {
  from: ItemRef;
  to: ItemRef;
  outcome: "same" | "moved";
};
export type SelectionEvidence = {
  name: string | null;
  observedAt: ModelResponse["observedAt"];
  sizeBasis: ModelResponse["model"]["representation"]["sizeBasis"];
  origin: ModelResponse["model"]["origin"];
};
export type VisualizationSnapshot = {
  graph: Committed | null;
  table: ModelResponse | null;
  graphPage: ModelRowsPage | null;
  tablePage: ModelRowsPage | null;
  pendingRows: VizRowsRequest | null;
  failedRows: VizRowsRequest | null;
  selection: VizSelection | null;
  focus: ItemRef | null;
  previousSelection: VizSelection | null;
  previousSelectionEvidence: SelectionEvidence | null;
  offeredRevision: string | null;
  phase:
    | "initial"
    | "loading"
    | "model"
    | "job"
    | "unavailable"
    | "error"
    | "cancelled";
  response: VizRenderResponse | null;
  job: Extract<VizRenderResponse, { kind: "job" }> | null;
  error: unknown;
};

function sameItem(
  a: ItemRef | null | undefined,
  b: ItemRef | null | undefined,
) {
  return (
    !!a && !!b && a.resultVersion === b.resultVersion && a.itemNo === b.itemNo
  );
}
function pageFor(frame: ModelResponse | null, page: ModelRowsPage | null) {
  return !!frame && !!page && matchingRowsPage(frame, page);
}
function validFrame(frame: ModelResponse) {
  return adaptVizModel(frame, undefined, () => ({
    accessibleLabel: "",
    displayLabel: "",
  })).validTargets;
}

function selectionEvidence(
  selection: VizSelection,
  frame: ModelResponse,
  page: ModelRowsPage | null,
): SelectionEvidence {
  const rows =
    pageFor(frame, page) && page
      ? page.response.rows.filter((row) =>
          selection.kind === "item"
            ? sameItem(row.itemRef, selection.itemRef)
            : row.groupRef === selection.groupRef,
        )
      : [];
  return {
    name:
      rows.length === 1 && rows[0]!.displayName ? rows[0]!.displayName : null,
    observedAt: frame.observedAt,
    sizeBasis: frame.model.representation.sizeBasis,
    origin: frame.model.origin,
  };
}
function modelMatchesRequest(frame: ModelResponse, request: VizRenderRequest) {
  return (
    validFrame(frame) &&
    sameItem(frame.model.aggregateScopeRef, request.aggregateScopeRef) &&
    sameItem(frame.model.navigationScopeRef, request.navigationScopeRef)
  );
}
function selectionPresent(selection: VizSelection, frame: ModelResponse) {
  if (
    selection.modelRef !== frame.model.modelRef ||
    selection.resultRevision !== frame.resultRevision
  )
    return false;
  const shapes = frame.model.shapes.filter((shape) =>
    selection.kind === "item"
      ? sameItem(shape.itemRef, selection.itemRef)
      : shape.groupRef === selection.groupRef,
  );
  return (
    shapes.length === 1 &&
    (selection.kind === "group" ||
      JSON.stringify(shapes[0]!.matchKey) ===
        JSON.stringify(selection.matchKey))
  );
}
function transferItem(
  item: ItemRef | null,
  revision: string,
  evidence: readonly Reconciled[],
) {
  if (!item) return null;
  if (item.resultVersion === revision) return item;
  const matches = evidence.filter(
    (entry) =>
      sameItem(entry.from, item) &&
      entry.to.resultVersion === revision &&
      (entry.outcome === "same" || entry.outcome === "moved"),
  );
  return matches.length === 1 ? matches[0]!.to : null;
}
function transferSelection(
  selection: VizSelection | null,
  frame: ModelResponse,
  evidence: readonly Reconciled[],
): VizSelection | null {
  if (!selection) return null;
  if (selection.kind === "group")
    return selection.modelRef === frame.model.modelRef &&
      selection.resultRevision === frame.resultRevision
      ? selection
      : null;
  if (
    selection.itemRef.resultVersion !== frame.resultRevision &&
    selection.matchKey.state !== "present"
  )
    return null;
  const item = transferItem(selection.itemRef, frame.resultRevision, evidence);
  if (!item) return null;
  const shapes = frame.model.shapes.filter((candidate) =>
    sameItem(candidate.itemRef, item),
  );
  if (shapes.length !== 1) return null;
  const shape = shapes[0]!;
  if (
    selection.resultRevision === frame.resultRevision &&
    JSON.stringify(shape.matchKey) !== JSON.stringify(selection.matchKey)
  )
    return null;
  return {
    kind: "item",
    itemRef: item,
    matchKey: shape.matchKey,
    modelRef: frame.model.modelRef,
    resultRevision: frame.resultRevision,
  };
}

/** Session-local boundary. Transport, reconciliation, events and product assembly are injected. */
export class VisualizationController {
  private state: VisualizationSnapshot = {
    graph: null,
    table: null,
    graphPage: null,
    tablePage: null,
    pendingRows: null,
    failedRows: null,
    selection: null,
    focus: null,
    previousSelection: null,
    previousSelectionEvidence: null,
    offeredRevision: null,
    phase: "initial",
    response: null,
    job: null,
    error: null,
  };
  private selectedEvidence: SelectionEvidence | null = null;
  private lastCompletedJob: string | null = null;
  private seq = 0;
  private channel: string | null = null;
  private lastRequest: VizRenderRequest | null = null;
  private lastEvidence: readonly Reconciled[] = [];
  private focusHasKey = false;
  private rowsSeq = 0;
  private listeners = new Set<() => void>();
  constructor(private readonly port: VisualizationPort) {}
  getSnapshot = () => this.state;
  nextRequestSequence() {
    if (this.seq >= 0xffff_ffff)
      throw new Error("Visualization view sequence exhausted");
    return this.seq + 1;
  }
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };
  private update(patch: Partial<VisualizationSnapshot>) {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((listener) => listener());
  }
  offerRevision(revision: string) {
    if (revision !== this.state.graph?.frame.resultRevision)
      this.update({ offeredRevision: revision });
  }
  select(selection: VizSelection | null) {
    const frame = this.state.graph?.frame;
    if (selection && (!frame || !selectionPresent(selection, frame))) return;
    this.selectedEvidence =
      selection && frame
        ? selectionEvidence(selection, frame, this.state.graphPage)
        : null;
    this.update({
      selection,
      previousSelection: null,
      previousSelectionEvidence: null,
    });
  }
  focus(item: ItemRef | null) {
    const frame = this.state.graph?.frame;
    if (
      item &&
      (!frame ||
        !frame.model.shapes.some((shape) => sameItem(shape.itemRef, item)))
    )
      return;
    this.focusHasKey =
      !!item &&
      !!frame?.model.shapes.some(
        (shape) =>
          sameItem(shape.itemRef, item) && shape.matchKey.state === "present",
      );
    this.update({ focus: item });
  }
  /** Evidence is supplied by the owner of browse.reconcile, never inferred from names or positions. */
  async render(
    request: VizRenderRequest,
    evidence: readonly Reconciled[] = [],
  ) {
    if (this.channel !== null && this.channel !== request.viewChannel)
      throw new Error("A controller is bound to one view channel");
    if (!request.resultRevision || request.requestSeq <= this.seq)
      throw new Error("A pinned revision and increasing sequence are required");
    const initialFocus = this.state.focus;
    const transfer = (item: ItemRef | null) =>
      transferItem(item, request.resultRevision!, evidence);
    // A focus request across revisions must be backed by the same evidence.
    if (
      request.focusItemRef &&
      initialFocus &&
      !sameItem(transfer(initialFocus), request.focusItemRef)
    )
      throw new Error("Focus lacks correspondence evidence");
    this.channel = request.viewChannel;
    this.seq = request.requestSeq;
    this.lastRequest = request;
    this.lastEvidence = evidence;
    const seq = this.seq;
    this.update({ phase: "loading", response: null, error: null });
    try {
      const response = await this.port.render(request);
      if (seq !== this.seq) return;
      if (
        response.requestSeq !== seq ||
        response.resultRevision !== request.resultRevision
      ) {
        this.update({
          phase: "error",
          error: new Error("Mismatched visualization response"),
        });
        return;
      }
      if (response.kind !== "model") {
        this.update({
          phase: response.kind,
          response,
          job: response.kind === "job" ? response : null,
        });
        return;
      }
      if (!modelMatchesRequest(response, request)) {
        this.update({
          phase: "error",
          error: new Error("Inconsistent model references"),
        });
        return;
      }
      this.commitModel(response, request, evidence);
    } catch (error) {
      if (seq === this.seq) this.update({ phase: "error", error });
    }
  }
  private commitModel(
    response: ModelResponse,
    request: VizRenderRequest,
    evidence: readonly Reconciled[],
  ) {
    const canDisplay = this.port.canDisplay(response);
    const oldSelection = this.state.selection;
    const previousEvidence = this.selectedEvidence;
    const selection = canDisplay
      ? transferSelection(oldSelection, response, evidence)
      : oldSelection;
    const selectionLost = Boolean(canDisplay && oldSelection && !selection);
    const oldFocus = this.state.focus;
    const canTransferFocus =
      !oldFocus ||
      oldFocus.resultVersion === response.resultRevision ||
      this.focusHasKey;
    const transferredFocus = canTransferFocus
      ? transferItem(oldFocus, response.resultRevision, evidence)
      : null;
    const focus = canDisplay
      ? transferredFocus &&
        response.model.shapes.filter((shape) =>
          sameItem(shape.itemRef, transferredFocus),
        ).length === 1
        ? transferredFocus
        : null
      : oldFocus;
    if (canDisplay)
      this.focusHasKey =
        !!focus &&
        response.model.shapes.some(
          (shape) =>
            sameItem(shape.itemRef, focus) &&
            shape.matchKey.state === "present",
        );
    const graph = canDisplay
      ? {
          frame: response,
          view: {
            viewport: request.viewport,
            visibleWindow: request.visibleWindow,
          },
        }
      : this.state.graph;
    this.rowsSeq++;
    if (canDisplay)
      this.selectedEvidence = selection
        ? selectionEvidence(selection, response, null)
        : null;
    this.update({
      graph,
      table: response,
      graphPage: canDisplay ? null : this.state.graphPage,
      tablePage: null,
      pendingRows: null,
      failedRows: null,
      selection,
      focus,
      previousSelection: selectionLost
        ? oldSelection
        : this.state.previousSelection,
      previousSelectionEvidence: selectionLost
        ? previousEvidence
        : this.state.previousSelectionEvidence,
      offeredRevision:
        canDisplay && this.state.offeredRevision === response.resultRevision
          ? null
          : this.state.offeredRevision,
      phase: "model",
      job: null,
      response,
    });
  }
  /** F24 status must come from job.get for the current accepted visualization job. */
  completeJob(status: AnalysisJobStatus) {
    const job = this.state.job;
    const request = this.lastRequest;
    if (
      this.state.phase !== "job" ||
      !job ||
      !request ||
      status.jobRef !== job.jobRef ||
      status.kind !== "viz.compute" ||
      request.requestSeq !== job.requestSeq ||
      this.seq !== job.requestSeq
    )
      return Promise.resolve();
    if (status.state === "done" && this.seq >= 0xffff_ffff) {
      this.update({
        phase: "error",
        job: null,
        error: new Error("Visualization view sequence exhausted"),
      });
      return Promise.resolve();
    }
    if (
      status.state === "done" &&
      this.seq < 0xffff_ffff &&
      this.lastCompletedJob !== status.jobRef
    ) {
      this.lastCompletedJob = status.jobRef;
      return this.render(
        { ...request, requestSeq: this.seq + 1 },
        this.lastEvidence,
      ).catch((error: unknown) => {
        if (this.state.job?.jobRef === status.jobRef)
          this.update({ phase: "error", error });
      });
    }
    if (status.state === "cancelled")
      this.update({ phase: "cancelled", job: null, error: null });
    else if (["failed", "partial"].includes(status.state))
      this.update({
        phase: "error",
        job: null,
        error: new Error("Visualization job did not complete"),
      });
    return Promise.resolve();
  }
  retry() {
    if (
      this.state.phase !== "error" ||
      !this.lastRequest ||
      this.seq >= 0xffff_ffff
    )
      return Promise.resolve();
    return this.render(
      { ...this.lastRequest, requestSeq: this.seq + 1 },
      this.lastEvidence,
    );
  }
  /** Explicit user cancellation or view teardown; never called for a new request. */
  cancelCurrent() {
    if (!this.channel) return Promise.resolve();
    const requestSeq =
      this.state.phase === "loading" ? this.seq : this.state.job?.requestSeq;
    if (requestSeq === undefined) return Promise.resolve();
    return this.port
      .cancel({
        target: { kind: "subscription", viewChannel: this.channel, requestSeq },
      })
      .then(() => undefined);
  }
  async rows(request: VizRowsRequest) {
    const frame = this.state.table;
    if (
      !frame ||
      request.modelRef !== frame.model.modelRef ||
      request.resultRevision !== frame.resultRevision
    )
      throw new Error("Rows must target the current table model");
    const seq = ++this.rowsSeq;
    this.update({ pendingRows: request, failedRows: null });
    try {
      const response = await this.port.rows(request);
      const page = { request, response };
      if (seq !== this.rowsSeq || this.state.table !== frame) return;
      if (!pageFor(frame, page)) {
        this.update({
          pendingRows: null,
          failedRows: request,
          error: new Error("Mismatched visualization rows"),
        });
        return;
      }
      if (
        pageFor(this.state.graph?.frame ?? null, page) &&
        this.state.selection &&
        this.state.graph
      ) {
        const details = selectionEvidence(
          this.state.selection,
          this.state.graph.frame,
          page,
        );
        if (details.name) this.selectedEvidence = details;
      }
      this.update({
        tablePage: page,
        graphPage: pageFor(this.state.graph?.frame ?? null, page)
          ? page
          : this.state.graphPage,
        pendingRows: null,
      });
    } catch (error) {
      if (seq === this.rowsSeq && this.state.table === frame)
        this.update({ pendingRows: null, failedRows: request, error });
    }
  }
}
