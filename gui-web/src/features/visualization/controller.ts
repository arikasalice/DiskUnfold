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
import {
  selectionPresent,
  transferItem,
  transferSelection,
} from "./selection-transfer";
import type { Reconciled } from "./selection-transfer";
import type {
  ModelResponse,
  ModelRowsPage,
  VisualizationPort,
  VizSelection,
} from "../../viz";

type View = Pick<VizRenderRequest, "viewport" | "visibleWindow">;
type Committed = { frame: ModelResponse; view: View };
export type { Reconciled } from "./selection-transfer";
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
/**
 * The same result and folder drawn as another format keeps an item selection
 * that has no shape there; switching back shows it again (F06 README 3, #241).
 * Another folder, result or an ambiguous match still loses it.
 */
function keptAcrossFormats(
  selection: VizSelection | null,
  previous: ModelResponse | undefined,
  next: ModelResponse,
): VizSelection | null {
  if (
    !selection ||
    !previous ||
    selection.resultRevision !== next.resultRevision ||
    previous.model.representation.format === next.model.representation.format ||
    !sameItem(previous.model.aggregateScopeRef, next.model.aggregateScopeRef)
  )
    return null;
  // An item the new chart draws is carried by its shape, never kept beside it.
  if (
    selection.kind === "item" &&
    next.model.shapes.some((shape) =>
      sameItem(shape.itemRef, selection.itemRef),
    )
  )
    return null;
  return selection;
}

/** The selection carried onto a model that will be displayed, and whether it was kept as is. */
function carrySelection(
  selection: VizSelection | null,
  previous: ModelResponse | undefined,
  next: ModelResponse,
  evidence: readonly Reconciled[],
) {
  const transferred = transferSelection(selection, next, evidence);
  if (transferred) return { selection: transferred, kept: false };
  const kept = keptAcrossFormats(selection, previous, next);
  return { selection: kept, kept: kept !== null };
}

/**
 * Only a new result that cannot carry the selection reports it lost; on the
 * same result (another folder) it is released quietly (F06 README 41).
 */
function lostToNewResult(
  previous: VizSelection | null,
  next: VizSelection | null,
  response: ModelResponse,
): boolean {
  return (
    !!previous && !next && previous.resultRevision !== response.resultRevision
  );
}

/** A kept selection keeps the evidence it had; a carried one is read from the new model. */
function evidenceAfter(
  carried: { selection: VizSelection | null; kept: boolean },
  previous: SelectionEvidence | null,
  next: ModelResponse,
): SelectionEvidence | null {
  if (carried.kept) return previous;
  return carried.selection && selectionEvidence(carried.selection, next, null);
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
  /**
   * Owner of the shared pendingRows, failedRows and error: the latest rows or
   * fallback read. A read that finishes after another one started may still
   * commit its own page, but it never touches the shared request state.
   */
  private rowsOwner = 0;
  private fallbackOwner = 0;
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
    // A new phase ends the fallback read, and its progress and failure with it.
    const fallbackOwned = this.fallbackOwner === this.rowsOwner;
    const dropRows = patch.phase === "unavailable" || fallbackOwned;
    if (dropRows) this.rowsOwner++;
    this.update({
      ...extra,
      fallbackPage: null,
      ...(dropRows ? { pendingRows: null, failedRows: null } : {}),
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
    const carried = canDisplay
      ? carrySelection(
          oldSelection,
          this.state.graph?.frame,
          response,
          evidence,
        )
      : { selection: oldSelection, kept: false };
    const selection = carried.selection;
    const selectionLost =
      canDisplay && lostToNewResult(oldSelection, selection, response);
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
    this.rowsOwner++;
    if (canDisplay)
      this.selectedEvidence = evidenceAfter(
        carried,
        previousEvidence,
        response,
      );
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
  /** Reads the rows of an expanded folder for shape names; the shared row state is untouched. */
  folderRows = (request: VizRowsRequest) =>
    readRowsPages((page) => this.port.rows(page), request);
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
    const owner = ++this.rowsOwner;
    this.update({ pendingRows: request, failedRows: null });
    try {
      const response = await readRowsPages(
        (page) => this.port.rows(page),
        request,
      );
      const page = { request, response };
      if (seq !== this.rowsSeq || this.state.table !== frame) return;
      if (!pageFor(frame, page)) {
        this.ownedUpdate(owner, {
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
        ...this.released(owner),
      });
    } catch (error) {
      if (seq === this.rowsSeq && this.state.table === frame)
        this.ownedUpdate(owner, {
          pendingRows: null,
          failedRows: request,
          error,
        });
    }
  }
  /** Reads rows of the fallback model; a page that arrives after the answer changed is dropped. */
  private async fallbackRows(request: VizRowsRequest) {
    const seq = ++this.fallbackSeq;
    const owner = ++this.rowsOwner;
    this.fallbackOwner = owner;
    this.update({ pendingRows: request, failedRows: null });
    try {
      const response = await this.port.rows(request);
      if (seq !== this.fallbackSeq) return;
      if (!fallbackRowsMatch(request, response)) {
        this.ownedUpdate(owner, {
          pendingRows: null,
          failedRows: request,
          error: new Error("Mismatched visualization rows"),
        });
        return;
      }
      this.update({
        fallbackPage: { request, response },
        ...this.released(owner),
      });
    } catch (error) {
      if (seq === this.fallbackSeq)
        this.ownedUpdate(owner, {
          pendingRows: null,
          failedRows: request,
          error,
        });
    }
  }
  /** Applies a change to the shared row request state only for its current owner. */
  private ownedUpdate(owner: number, patch: Partial<VisualizationSnapshot>) {
    if (owner === this.rowsOwner) this.update(patch);
  }
  /** Clears the progress of a read that finished, unless a newer read owns it. */
  private released(owner: number): Partial<VisualizationSnapshot> {
    return owner === this.rowsOwner ? { pendingRows: null } : {};
  }
  private isFallbackRequest(request: VizRowsRequest) {
    const fallback = fallbackReference(this.state);
    return (
      fallback?.modelRef === request.modelRef &&
      fallback.resultRevision === request.resultRevision
    );
  }
}
