import { AnalysisError } from "../../ipc";
import type {
  AnalysisJobStatus,
  ItemRef,
  VizRenderRequest,
  VizRenderResponse,
  VizRowsRequest,
} from "../../ipc";
import {
  matchingRowsPage,
  plainRowName,
  readRowsPages,
  sameItem,
  VizModelLoader,
} from "../../viz";
import { fallbackReference, fallbackRowsMatch } from "./fallback-table";
import type {
  ModelResponse,
  ModelRowsPage,
  VisualizationPort,
  VizSelection,
} from "../../viz";

type View = Pick<VizRenderRequest, "viewport" | "visibleWindow">;
type Committed = { frame: ModelResponse; view: View };
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
  /** Rows of the fallback model while an unavailable answer names one. */
  fallbackPage: ModelRowsPage | null;
  pendingRows: VizRowsRequest | null;
  failedRows: VizRowsRequest | null;
  selection: VizSelection | null;
  focus: ItemRef | null;
  previousSelection: VizSelection | null;
  previousSelectionEvidence: SelectionEvidence | null;
  offeredRevision: string | null;
  revisionCheck: "stale" | "unverified" | null;
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

function pageFor(frame: ModelResponse | null, page: ModelRowsPage | null) {
  return !!frame && !!page && matchingRowsPage(frame, page);
}
function visibleFrames(
  graph: ModelResponse | undefined,
  table: ModelResponse | null,
) {
  if (!graph) return table ? [table] : [];
  if (
    !table ||
    (table.model.modelRef === graph.model.modelRef &&
      table.resultRevision === graph.resultRevision)
  )
    return [graph];
  return [graph, table];
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
    name: rows.length === 1 ? plainRowName(rows[0]!) || null : null,
    observedAt: frame.observedAt,
    sizeBasis: frame.model.representation.sizeBasis,
    origin: frame.model.origin,
  };
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
    fallbackPage: null,
    pendingRows: null,
    failedRows: null,
    selection: null,
    focus: null,
    previousSelection: null,
    previousSelectionEvidence: null,
    offeredRevision: null,
    revisionCheck: null,
    phase: "initial",
    response: null,
    job: null,
    error: null,
  };
  private selectedEvidence: SelectionEvidence | null = null;
  private readonly loader: VizModelLoader<
    readonly Reconciled[],
    Partial<VisualizationSnapshot>
  >;
  private focusHasKey = false;
  private rowsSeq = 0;
  private fallbackSeq = 0;
  private revisionCheckSeq = 0;
  private listeners = new Set<() => void>();
  constructor(private readonly port: VisualizationPort) {
    this.loader = new VizModelLoader(
      port,
      {
        admit: (request, evidence) => this.admit(request, evidence),
        commit: (response, request, evidence) =>
          this.commitPatch(response, request, evidence),
        publish: (patch, extra) => this.publish(patch, extra),
      },
      [],
    );
  }
  getSnapshot = () => this.state;
  nextRequestSequence() {
    return this.loader.nextRequestSequence();
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
  /**
   * Applies one loader change. A new phase ends the fallback table of the
   * answer before it, and an unavailable answer drops the row state of the
   * model it hides; the previous graph itself is kept.
   */
  private publish(
    patch: Partial<VisualizationSnapshot>,
    extra?: Partial<VisualizationSnapshot>,
  ) {
    if (patch.phase === undefined) return this.update({ ...extra, ...patch });
    this.fallbackSeq++;
    this.update({
      ...extra,
      fallbackPage: null,
      ...(patch.phase === "unavailable"
        ? { pendingRows: null, failedRows: null }
        : {}),
      ...patch,
    });
  }
  offerRevision(revision: string) {
    if (revision !== this.state.graph?.frame.resultRevision)
      this.update({ offeredRevision: revision });
  }
  /** Rechecks the exact visible models without replacing their source or row pages. */
  async recheckVisibleModels(alive: () => boolean) {
    const graph = this.state.graph;
    const table = this.state.table;
    const seq = ++this.revisionCheckSeq;
    const frames = visibleFrames(graph?.frame, table);
    const results = await Promise.all(
      frames.map((frame) => this.checkFrame(frame)),
    );
    if (
      !alive() ||
      seq !== this.revisionCheckSeq ||
      graph !== this.state.graph ||
      table !== this.state.table
    )
      return;
    const revisionCheck = results.includes("stale")
      ? "stale"
      : results.includes("unverified")
        ? "unverified"
        : null;
    if (revisionCheck !== this.state.revisionCheck)
      this.update({ revisionCheck });
  }
  private async checkFrame(frame: ModelResponse) {
    const request = {
      modelRef: frame.model.modelRef,
      resultRevision: frame.resultRevision,
      start: 0,
      count: 1,
    };
    try {
      const response = await this.port.rows(request);
      return matchingRowsPage(frame, { request, response })
        ? null
        : "unverified";
    } catch (error) {
      return error instanceof AnalysisError &&
        ["STALE_VERSION", "NOT_FOUND"].includes(error.code)
        ? "stale"
        : "unverified";
    }
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
  render(request: VizRenderRequest, evidence: readonly Reconciled[] = []) {
    return this.loader.render(request, evidence);
  }
  private admit(request: VizRenderRequest, evidence: readonly Reconciled[]) {
    const initialFocus = this.state.focus;
    // A focus request across revisions must be backed by the same evidence.
    if (
      request.focusItemRef &&
      initialFocus &&
      !sameItem(
        transferItem(initialFocus, request.resultRevision!, evidence),
        request.focusItemRef,
      )
    )
      throw new Error("Focus lacks correspondence evidence");
  }
  /** Carries selection and focus onto a model the loader accepted; the loader publishes the result. */
  private commitPatch(
    response: ModelResponse,
    request: VizRenderRequest,
    evidence: readonly Reconciled[],
  ): Partial<VisualizationSnapshot> {
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
    return {
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
      revisionCheck: null,
    };
  }
  /** F24 status must come from job.get for the current accepted visualization job. */
  completeJob(status: AnalysisJobStatus) {
    return this.loader.completeJob(status);
  }
  retry() {
    return this.loader.retry();
  }
  /** Explicit user cancellation or view teardown; never called for a new request. */
  cancelCurrent() {
    return this.loader.cancelCurrent();
  }
  async rows(request: VizRowsRequest) {
    if (this.isFallbackRequest(request)) return this.fallbackRows(request);
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
      const response = await readRowsPages(
        (page) => this.port.rows(page),
        request,
      );
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
  /** Reads rows of the fallback model; a page that arrives after the answer changed is dropped. */
  private async fallbackRows(request: VizRowsRequest) {
    const seq = this.fallbackSeq;
    this.update({ pendingRows: request, failedRows: null });
    try {
      const response = await this.port.rows(request);
      if (seq !== this.fallbackSeq) return;
      if (!fallbackRowsMatch(request, response)) {
        this.update({
          pendingRows: null,
          failedRows: request,
          error: new Error("Mismatched visualization rows"),
        });
        return;
      }
      this.update({ fallbackPage: { request, response }, pendingRows: null });
    } catch (error) {
      if (seq === this.fallbackSeq)
        this.update({ pendingRows: null, failedRows: request, error });
    }
  }
  private isFallbackRequest(request: VizRowsRequest) {
    const fallback = fallbackReference(this.state);
    return (
      fallback?.modelRef === request.modelRef &&
      fallback.resultRevision === request.resultRevision
    );
  }
}
