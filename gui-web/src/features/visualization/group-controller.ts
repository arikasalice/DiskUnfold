import type {
  VizGroupRows,
  VizGroupRowsRequest,
  VizShape,
  VizSizeRange,
  VizRenderResponse,
} from "../../ipc";

export type GroupFrame = Pick<
  Extract<VizRenderResponse, { kind: "model" }>,
  "resultRevision" | "observedAt" | "completeness"
> & { modelRef: string };
export type GroupSelection = {
  groupRef: string;
  /** The caller supplies the already validated model shape as membership proof. */
  shape: Pick<VizShape, "groupRef" | "size">;
};
export type GroupPage = {
  request: VizGroupRowsRequest;
  response: VizGroupRows;
};
export type GroupFailure = "stale" | "missing" | "invalid" | "page";
export type GroupSnapshot = {
  frame: GroupFrame | null;
  groupRef: string | null;
  groupTotal: VizGroupRows["groupTotal"] | null;
  page: GroupPage | null;
  start: number;
  count: number;
  status: "closed" | "loading" | "ready" | "error";
  failure: GroupFailure | null;
  canRetry: boolean;
  sizeRange: VizSizeRange | null;
  totalCount: number | null;
  rangeLabel: string | undefined;
  retryStart: number | null;
  hasPrevious: boolean;
  hasNext: boolean;
};
export type GroupRowsPort = {
  rows(request: VizGroupRowsRequest): Promise<VizGroupRows>;
};

const PAGE_SIZE = 100;
function validPaging(start: number, count: number) {
  return (
    Number.isInteger(start) &&
    start >= 0 &&
    Number.isInteger(count) &&
    count >= 1 &&
    count <= 1000
  );
}

function sameSize(
  a: VizGroupRows["groupTotal"],
  b: VizGroupRows["groupTotal"],
) {
  return (
    a.state === b.state &&
    a.basis === b.basis &&
    a.evidence === b.evidence &&
    (a.state === "unknown" || (b.state !== "unknown" && a.bytes === b.bytes))
  );
}
function sameRange(a: VizSizeRange, b: VizSizeRange) {
  return (
    a.kind === b.kind &&
    (a.kind === "unknown" ||
      (b.kind === "known" && sameSize(a.min, b.min) && sameSize(a.max, b.max)))
  );
}
const frameKey = (f: GroupFrame) =>
  JSON.stringify([f.modelRef, f.resultRevision, f.observedAt, f.completeness]);
function failureCode(error: unknown): GroupFailure {
  const code =
    typeof error === "object" && error !== null && "code" in error
      ? (error as { code?: unknown }).code
      : undefined;
  if (code === "STALE_VERSION") return "stale";
  if (code === "NOT_FOUND") return "missing";
  if (code === "INVALID_ARGUMENT") return "invalid";
  return "page";
}

/** Session-local group paging. It trusts no totals or membership beyond supplied evidence. */
export class GroupController {
  private state: GroupSnapshot = {
    frame: null,
    groupRef: null,
    groupTotal: null,
    page: null,
    start: 0,
    count: PAGE_SIZE,
    status: "closed",
    failure: null,
    canRetry: false,
    sizeRange: null,
    totalCount: null,
    rangeLabel: undefined,
    retryStart: null,
    hasPrevious: false,
    hasNext: false,
  };
  private epoch = 0;
  private sequence = 0;
  private previousStarts: number[] = [];
  private listeners = new Set<() => void>();
  constructor(private readonly port: GroupRowsPort) {}
  getSnapshot = () => this.state;
  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => this.listeners.delete(listener);
  };
  private update(patch: Partial<GroupSnapshot>) {
    this.state = { ...this.state, ...patch };
    this.listeners.forEach((listener) => listener());
  }
  private clear(
    frame: GroupFrame | null,
    status: GroupSnapshot["status"] = "closed",
  ) {
    this.previousStarts = [];
    this.epoch++;
    this.sequence++;
    this.state = {
      ...this.state,
      frame,
      groupRef: null,
      groupTotal: null,
      page: null,
      start: 0,
      status,
      failure: null,
      canRetry: false,
      sizeRange: null,
      totalCount: null,
      rangeLabel: undefined,
      retryStart: null,
      hasPrevious: false,
      hasNext: false,
    };
    this.listeners.forEach((listener) => listener());
  }
  replaceFrame(frame: GroupFrame) {
    if (!this.state.frame || frameKey(frame) !== frameKey(this.state.frame))
      this.clear(frame);
    else this.update({ frame });
  }
  async open(frame: GroupFrame, selection: GroupSelection, count = PAGE_SIZE) {
    this.replaceFrame(frame);
    if (!Number.isInteger(count) || count < 1 || count > 1000) return false;
    if (selection.shape.groupRef !== selection.groupRef) return false;
    this.previousStarts = [];
    this.epoch++;
    this.sequence++;
    this.state = {
      ...this.state,
      groupRef: selection.groupRef,
      groupTotal: selection.shape.size,
      page: null,
      start: 0,
      count,
      status: "loading",
      failure: null,
      canRetry: false,
      sizeRange: null,
      totalCount: null,
      rangeLabel: undefined,
      retryStart: null,
      hasPrevious: false,
      hasNext: false,
    };
    this.listeners.forEach((listener) => listener());
    return this.fetch(0);
  }
  close() {
    this.clear(this.state.frame);
  }
  next() {
    if (!this.state.hasNext || this.state.status === "loading")
      return Promise.resolve(false);
    return this.fetch(
      this.state.page!.request.start + this.state.page!.response.rows.length,
    );
  }
  previous() {
    if (!this.state.hasPrevious || this.state.status === "loading")
      return Promise.resolve(false);
    return this.fetch(this.previousStarts[this.previousStarts.length - 1] ?? 0);
  }
  retry() {
    if (!this.state.canRetry || this.state.status !== "error")
      return Promise.resolve(false);
    return this.fetch(this.state.retryStart ?? this.state.start);
  }
  private async fetch(start: number): Promise<boolean> {
    const { frame, groupRef, groupTotal, count } = this.state;
    if (!frame || !groupRef || !groupTotal || !validPaging(start, count))
      return false;
    const epoch = this.epoch;
    const seq = ++this.sequence;
    const request = { modelRef: frame.modelRef, groupRef, start, count };
    this.update({ status: "loading", failure: null, canRetry: false });
    try {
      const response = await this.port.rows(request);
      if (epoch !== this.epoch || seq !== this.sequence) return false;
      if (
        !this.validMetadata(response, frame, groupTotal, start) ||
        !this.validRows(response, frame, start, count)
      ) {
        this.update({
          status: "error",
          failure: "invalid",
          canRetry: false,
          page: null,
          sizeRange: null,
          totalCount: null,
          rangeLabel: undefined,
          hasPrevious: false,
          hasNext: false,
        });
        return false;
      }
      if (this.state.page && start > this.state.start)
        this.previousStarts.push(this.state.start);
      else if (this.state.page && start < this.state.start)
        this.previousStarts.pop();
      this.update({
        page: { request, response },
        start,
        totalCount: response.totalCount,
        sizeRange: response.sizeRange,
        status: "ready",
        failure: null,
        canRetry: false,
        retryStart: null,
        rangeLabel:
          response.totalCount === 0
            ? "0"
            : `${start + 1}–${start + response.rows.length} / ${response.totalCount}`,
        hasPrevious: start > 0,
        hasNext: start + response.rows.length < response.totalCount,
      });
      return true;
    } catch (error) {
      if (epoch !== this.epoch || seq !== this.sequence) return false;
      return this.recordFailure(error, start);
    }
  }
  private recordFailure(error: unknown, start: number) {
    const failure = failureCode(error);
    const retryable =
      failure === "page" &&
      !(
        typeof error === "object" &&
        error !== null &&
        "retryable" in error &&
        error.retryable === false
      );
    this.update({
      status: "error",
      failure,
      canRetry: retryable,
      retryStart: retryable ? start : null,
      ...(!retryable
        ? {
            page: null,
            sizeRange: null,
            totalCount: null,
            rangeLabel: undefined,
            hasPrevious: false,
            hasNext: false,
          }
        : {}),
    });
    return false;
  }
  private validMetadata(
    response: VizGroupRows,
    frame: GroupFrame,
    groupTotal: VizGroupRows["groupTotal"],
    start: number,
  ) {
    return !(
      response.resultRevision !== frame.resultRevision ||
      response.observedAt !== frame.observedAt ||
      response.completeness !== frame.completeness ||
      !Number.isSafeInteger(response.totalCount) ||
      response.totalCount < 0 ||
      start > response.totalCount ||
      !sameSize(response.groupTotal, groupTotal) ||
      (this.state.totalCount !== null &&
        (response.totalCount !== this.state.totalCount ||
          !this.state.sizeRange ||
          !sameRange(response.sizeRange, this.state.sizeRange))) ||
      !this.validRange(response.sizeRange)
    );
  }
  private validRows(
    response: VizGroupRows,
    frame: GroupFrame,
    start: number,
    count: number,
  ) {
    return !(
      response.rows.length > count ||
      start + response.rows.length > response.totalCount ||
      (response.totalCount > start && response.rows.length === 0) ||
      response.rows.some(
        (row) =>
          (row.itemRef === null) === (row.groupRef === null) ||
          (row.itemRef !== null &&
            row.itemRef.resultVersion !== frame.resultRevision),
      )
    );
  }
  private validRange(value: VizSizeRange) {
    if (value.kind !== "known") return value.kind === "unknown";
    if (value.min.state === "unknown" || value.max.state === "unknown")
      return false;
    try {
      return BigInt(value.min.bytes) <= BigInt(value.max.bytes);
    } catch {
      return false;
    }
  }
}
