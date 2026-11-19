import type {
  AnalysisJobStatus,
  VizRenderRequest,
  VizRenderResponse,
} from "../ipc";
import { sameItem } from "./item-ref";
import { adaptVizModel } from "./model-adapter";
import type { ModelResponse } from "./model-adapter";
import type { VisualizationPort } from "./visualization-port";

const LAST_SEQUENCE = 0xffff_ffff;

/** Phase of the request that a loader currently serves. */
export type VizLoadPhase =
  | "initial"
  | "loading"
  | "model"
  | "job"
  | "unavailable"
  | "error"
  | "cancelled";
/** Accepted-job answer of viz.render. */
export type VizJobResponse = Extract<VizRenderResponse, { kind: "job" }>;
/** The part of a view snapshot that a loader owns and publishes. */
export type VizLoadState = {
  phase: VizLoadPhase;
  response: VizRenderResponse | null;
  job: VizJobResponse | null;
  error: unknown;
};

/**
 * What the owner of the view supplies to a loader. `TEvidence` is opaque to the
 * loader: it is handed back to `admit` and `commit` and replayed on retries.
 */
export type VizLoaderSink<TEvidence, TExtra extends object> = {
  /** Extra admission check of the owner; throwing rejects the request before any state changes. */
  admit(request: VizRenderRequest, evidence: TEvidence): void;
  /** Builds the owner's part of the state change for a model that passed the loader's checks. */
  commit(
    response: ModelResponse,
    request: VizRenderRequest,
    evidence: TEvidence,
  ): TExtra;
  /**
   * Applies one state change; every call is one atomic update for the view.
   * `extra` is what `commit` returned and accompanies the load state of a model.
   */
  publish(patch: Partial<VizLoadState>, extra?: TExtra): void;
};

function validFrame(frame: ModelResponse) {
  return adaptVizModel(frame, undefined, () => ({
    accessibleLabel: "",
    displayLabel: "",
  })).validTargets;
}

/**
 * A model answers its request only for the same scopes and size basis. A model
 * for another basis would draw its values under the requested basis label, so
 * it is refused as a failed render: the previous chart stays and can be retried.
 */
function modelMatchesRequest(frame: ModelResponse, request: VizRenderRequest) {
  return (
    validFrame(frame) &&
    frame.model.representation.sizeBasis === request.sizeBasis &&
    sameItem(frame.model.aggregateScopeRef, request.aggregateScopeRef) &&
    sameItem(frame.model.navigationScopeRef, request.navigationScopeRef)
  );
}

/**
 * Turns pinned requests into committed models for one view channel. Requests
 * carry increasing sequences; an answer for any request but the latest is
 * discarded, as is an answer whose revision or references do not match the
 * request. The loader holds no selection, focus or table state: the owner adds
 * its own through the sink.
 */
export class VizModelLoader<TEvidence, TExtra extends object> {
  private phase: VizLoadPhase = "initial";
  private job: VizJobResponse | null = null;
  private seq = 0;
  private channel: string | null = null;
  private lastRequest: VizRenderRequest | null = null;
  private lastCompletedJob: string | null = null;
  private lastEvidence: TEvidence;

  /** `noEvidence` is the evidence replayed until a request supplies its own. */
  constructor(
    private readonly port: Pick<VisualizationPort, "render" | "cancel">,
    private readonly sink: VizLoaderSink<TEvidence, TExtra>,
    noEvidence: TEvidence,
  ) {
    this.lastEvidence = noEvidence;
  }

  /** The sequence the next request must carry at least. Throws once the sequence space is used up. */
  nextRequestSequence() {
    if (this.seq >= LAST_SEQUENCE)
      throw new Error("Visualization view sequence exhausted");
    return this.seq + 1;
  }

  private emit(patch: Partial<VizLoadState>, extra?: TExtra) {
    if (patch.phase !== undefined) this.phase = patch.phase;
    if (patch.job !== undefined) this.job = patch.job;
    this.sink.publish(patch, extra);
  }

  /**
   * Requests a model. Rejects when the request belongs to another channel, is
   * not pinned to a revision, does not advance the sequence, or fails the
   * sink's admission check; a rejected request leaves the loader unchanged.
   */
  async render(request: VizRenderRequest, evidence: TEvidence) {
    if (this.channel !== null && this.channel !== request.viewChannel)
      throw new Error("A controller is bound to one view channel");
    if (!request.resultRevision || request.requestSeq <= this.seq)
      throw new Error("A pinned revision and increasing sequence are required");
    this.sink.admit(request, evidence);
    this.channel = request.viewChannel;
    this.seq = request.requestSeq;
    this.lastRequest = request;
    this.lastEvidence = evidence;
    const seq = this.seq;
    this.emit({ phase: "loading", response: null, error: null });
    try {
      const response = await this.port.render(request);
      if (seq !== this.seq) return;
      if (
        response.requestSeq !== seq ||
        response.resultRevision !== request.resultRevision
      ) {
        this.emit({
          phase: "error",
          error: new Error("Mismatched visualization response"),
        });
        return;
      }
      if (response.kind !== "model") {
        this.emit({
          phase: response.kind,
          response,
          job: response.kind === "job" ? response : null,
        });
        return;
      }
      if (!modelMatchesRequest(response, request)) {
        this.emit({
          phase: "error",
          error: new Error("Inconsistent model references"),
        });
        return;
      }
      this.emit(
        { phase: "model", job: null, response },
        this.sink.commit(response, request, evidence),
      );
    } catch (error) {
      if (seq === this.seq) this.emit({ phase: "error", error });
    }
  }

  /** F24 status must come from job.get for the current accepted visualization job. */
  completeJob(status: AnalysisJobStatus) {
    const job = this.job;
    const request = this.lastRequest;
    if (
      this.phase !== "job" ||
      !job ||
      !request ||
      status.jobRef !== job.jobRef ||
      status.kind !== "viz.compute" ||
      request.requestSeq !== job.requestSeq ||
      this.seq !== job.requestSeq
    )
      return Promise.resolve();
    if (status.state === "done" && this.seq >= LAST_SEQUENCE) {
      this.emit({
        phase: "error",
        job: null,
        error: new Error("Visualization view sequence exhausted"),
      });
      return Promise.resolve();
    }
    if (
      status.state === "done" &&
      this.seq < LAST_SEQUENCE &&
      this.lastCompletedJob !== status.jobRef
    ) {
      this.lastCompletedJob = status.jobRef;
      return this.render(
        { ...request, requestSeq: this.seq + 1 },
        this.lastEvidence,
      ).catch((error: unknown) => {
        if (this.job?.jobRef === status.jobRef)
          this.emit({ phase: "error", error });
      });
    }
    if (status.state === "cancelled")
      this.emit({ phase: "cancelled", job: null, error: null });
    else if (["failed", "partial"].includes(status.state))
      this.emit({
        phase: "error",
        job: null,
        error: new Error("Visualization job did not complete"),
      });
    return Promise.resolve();
  }

  /** Repeats the last request with the next sequence after a failure. */
  retry() {
    if (
      this.phase !== "error" ||
      !this.lastRequest ||
      this.seq >= LAST_SEQUENCE
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
      this.phase === "loading" ? this.seq : this.job?.requestSeq;
    if (requestSeq === undefined) return Promise.resolve();
    return this.port
      .cancel({
        target: { kind: "subscription", viewChannel: this.channel, requestSeq },
      })
      .then(() => undefined);
  }
}
