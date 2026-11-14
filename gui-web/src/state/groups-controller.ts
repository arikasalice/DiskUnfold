import { AnalysisError, newCorrelationId } from "../ipc";
import type {
  AnalysisClient,
  EventEnvelope,
  GroupClassifyResponse,
  GroupRuleset,
  GroupSummary,
  Id,
  ItemRef,
} from "../ipc";
import { watchJobBaseline } from "./job-baseline";
import {
  ViewChannel,
  isRequeryReason,
  judgeResponse,
  nextGroupViewPhase,
} from "./groups-view-machine";
import type {
  GroupViewEvent,
  GroupViewPhase,
  ResponseFacts,
  ViewCondition,
} from "./groups-view-machine";

/** How a classification ended without a new result. */
export type GroupsOutcome =
  | { kind: "cancelled" }
  | { kind: "failed"; reasonKey?: string | undefined; retryable: boolean };

/** Why the shown result can no longer be trusted. */
export type GroupsStaleReason = "version" | "expired" | "rules";

/** What the view classifies: a pinned result and an optional folder inside it. */
export type GroupsTarget = {
  resultVersion?: Id | undefined;
  scope?: ItemRef | undefined;
};

/** Immutable view model published by {@link GroupsController}. */
export type GroupsSnapshot = {
  phase: GroupViewPhase;
  ruleset: GroupRuleset | undefined;
  rulesStatus: "loading" | "ready" | "error";
  /** The last accepted classification; kept while a newer one is awaited. */
  summary: GroupSummary | undefined;
  outcome: GroupsOutcome | undefined;
  /** Reason key of a failed request or of an inapplicable source. */
  reasonKey: string | undefined;
  staleReason: GroupsStaleReason | undefined;
  /** A newer result version, or "unknown" when the event did not name it. */
  newerResult: Id | "unknown" | undefined;
  /** The saved rules differ from the rules of the shown classification. */
  rulesChanged: boolean;
  jobRef: Id | undefined;
  scope: ItemRef | undefined;
};

type Pending = {
  seq: number;
  jobRef?: Id;
  classificationRef?: Id;
  resultVersion?: Id;
};

/** Extracts the reason of a failed call, if the host reported one. */
export function describeFailure(error: unknown): {
  reasonKey: string | undefined;
  retryable: boolean;
} {
  if (error instanceof AnalysisError)
    return {
      reasonKey: error.reasonKey,
      retryable: error.retryable === true,
    };
  return { reasonKey: undefined, retryable: false };
}

/** True while the visible summary is not the answer to the latest request. */
export function showsPreviousClassification(snapshot: GroupsSnapshot) {
  if (!snapshot.summary) return false;
  return (
    snapshot.phase === "requesting" ||
    snapshot.phase === "working" ||
    snapshot.phase === "requery_needed" ||
    snapshot.phase === "not_applicable" ||
    snapshot.outcome !== undefined
  );
}

function eventVersion(event: EventEnvelope): Id | undefined {
  if (typeof event.resultVersion === "string") return event.resultVersion;
  const body = event.body;
  if (body !== null && typeof body === "object" && "resultVersion" in body)
    return typeof body.resultVersion === "string"
      ? body.resultVersion
      : undefined;
  return undefined;
}

/**
 * Owns one virtual-group view: classification requests on one view channel,
 * the job that may follow, and the phase table. Nothing here classifies on its
 * own: a new result version only offers an update.
 */
export class GroupsController {
  readonly channel: ViewChannel;
  private state: GroupsSnapshot;
  private readonly listeners = new Set<() => void>();
  private target: { resultVersion: Id | undefined; scope: ItemRef | undefined };
  private condition: ViewCondition;
  private pending: Pending | undefined;
  private stopJob: (() => void) | undefined;
  private stopEvents: (() => void) | undefined;
  private started = false;
  private disposed = false;
  private readingJob = false;
  private rereadJob = false;

  constructor(
    private readonly client: AnalysisClient,
    target: GroupsTarget,
    viewRef: Id = newCorrelationId(),
  ) {
    this.channel = new ViewChannel(viewRef);
    this.target = { resultVersion: target.resultVersion, scope: target.scope };
    this.condition = {
      ...this.target,
      rulesetVersion: 0,
      rulesetFingerprint: undefined,
    };
    this.state = {
      phase: "idle",
      ruleset: undefined,
      rulesStatus: "loading",
      summary: undefined,
      outcome: undefined,
      reasonKey: undefined,
      staleReason: undefined,
      newerResult: undefined,
      rulesChanged: false,
      jobRef: undefined,
      scope: target.scope,
    };
  }

  getSnapshot = (): GroupsSnapshot => this.state;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** Subscribes to result events and reads the saved rules. Safe to call once. */
  start(): void {
    if (this.started || this.disposed) return;
    this.started = true;
    this.stopEvents = this.client.subscribe((event) => this.onEvent(event));
    void this.refreshRules();
  }

  /** Stops observing and asks the host to drop an unfinished classification. */
  dispose(): void {
    if (this.disposed) return;
    this.disposed = true;
    this.stopEvents?.();
    this.stopWatchingJob();
    if (this.pending && this.state.phase === "working")
      void this.client
        .call("group.cancel", {
          kind: "view",
          viewRef: this.channel.viewRef,
          requestSeq: this.channel.last,
        })
        .catch(() => {});
    this.listeners.clear();
  }

  /** Reads the stored rules. Called at start and by the editor to reload them. */
  async refreshRules(): Promise<GroupRuleset | undefined> {
    try {
      const ruleset = await this.client.call("group.rules.get", {});
      if (this.disposed) return undefined;
      this.applySavedRuleset(ruleset);
      return ruleset;
    } catch {
      if (!this.disposed) this.patch({ rulesStatus: "error" });
      return undefined;
    }
  }

  /**
   * Records rules that are now stored. A shown classification made with other
   * rules becomes an earlier one, and an answer still in flight is dropped.
   */
  applySavedRuleset(ruleset: GroupRuleset): void {
    if (this.disposed) return;
    this.condition = {
      ...this.condition,
      rulesetVersion: ruleset.rulesetVersion,
      rulesetFingerprint: ruleset.fingerprint,
    };
    const shown = this.state.summary;
    const changed =
      shown !== undefined && shown.rulesetFingerprint !== ruleset.fingerprint;
    this.patch({ ruleset, rulesStatus: "ready", rulesChanged: changed });
    if (changed) this.move("rules_changed", { rulesChanged: true });
  }

  /** Starts a classification of the target with the saved rules. */
  async classify(): Promise<void> {
    if (this.disposed) return;
    const retarget = this.nextTarget();
    if (
      !this.move("request", {
        outcome: undefined,
        reasonKey: undefined,
        staleReason: undefined,
        jobRef: undefined,
      })
    )
      return;
    this.target = retarget;
    this.stopWatchingJob();
    this.pending = undefined;
    this.patch({ scope: retarget.scope });
    let ruleset: GroupRuleset;
    try {
      ruleset = await this.client.call("group.rules.get", {});
    } catch (error) {
      this.failRequest(error, undefined);
      return;
    }
    if (this.disposed || this.state.phase !== "requesting") return;
    this.patch({ ruleset, rulesStatus: "ready" });
    this.condition = {
      resultVersion: retarget.resultVersion,
      scope: retarget.scope,
      rulesetVersion: ruleset.rulesetVersion,
      rulesetFingerprint: ruleset.fingerprint,
    };
    const requestSeq = this.channel.next();
    this.pending = { seq: requestSeq };
    try {
      const response = await this.client.call("group.classify", {
        viewRef: this.channel.viewRef,
        requestSeq,
        ...(retarget.resultVersion !== undefined
          ? { resultVersion: retarget.resultVersion }
          : {}),
        ...(retarget.scope !== undefined ? { scope: retarget.scope } : {}),
        rules: { kind: "saved", rulesetVersion: ruleset.rulesetVersion },
      });
      this.onClassified(response);
    } catch (error) {
      this.failRequest(error, requestSeq);
    }
  }

  /** Asks the host to cancel the running classification job. */
  async cancel(): Promise<void> {
    const jobRef = this.pending?.jobRef;
    if (this.disposed || this.state.phase !== "working" || !jobRef) return;
    try {
      const answer = await this.client.call("group.cancel", {
        kind: "job",
        jobRef,
      });
      if (answer.outcome === "already_terminal") void this.readJob(jobRef);
    } catch {
      // The job keeps running; the terminal state arrives through its events.
    }
  }

  /** Reports that a read of the shown classification found it unusable. */
  reportUnreadable(error: unknown): boolean {
    const { reasonKey } = describeFailure(error);
    if (!isRequeryReason(reasonKey)) return false;
    this.markStale(reasonKey);
    return true;
  }

  private nextTarget() {
    const { phase, newerResult, staleReason } = this.state;
    if (phase === "updatable" && newerResult !== undefined)
      // A folder reference belongs to one result version, so an update
      // classifies the whole newer result.
      return {
        resultVersion: newerResult === "unknown" ? undefined : newerResult,
        scope: undefined,
      };
    if (phase === "requery_needed" && staleReason === "version")
      return { resultVersion: undefined, scope: undefined };
    return this.target;
  }

  private emit() {
    for (const listener of [...this.listeners]) listener();
  }

  private patch(patch: Partial<GroupsSnapshot>) {
    this.state = { ...this.state, ...patch };
    this.emit();
  }

  /** Applies an event through the transition table; false when it does not apply. */
  private move(
    event: GroupViewEvent,
    patch: Partial<GroupsSnapshot> = {},
  ): boolean {
    const phase = nextGroupViewPhase(this.state.phase, event);
    if (phase === undefined) return false;
    this.state = { ...this.state, ...patch, phase };
    this.emit();
    return true;
  }

  private judge(facts: ResponseFacts, apply: () => void) {
    if (this.disposed) return;
    const verdict = judgeResponse(this.channel, this.condition, facts);
    if (verdict === "accept") apply();
    else if (verdict === "mismatch")
      this.markStale(
        facts.resultVersion !== undefined &&
          this.condition.resultVersion !== undefined &&
          facts.resultVersion !== this.condition.resultVersion
          ? "result.version_stale"
          : "group.rules_modified",
      );
  }

  private onClassified(response: GroupClassifyResponse) {
    const requestSeq = response.requestSeq;
    if (response.kind === "summary") {
      const summary = response.summary;
      this.judge(
        {
          requestSeq,
          resultVersion: summary.resultVersion,
          rulesetVersion: summary.rulesetVersion,
          rulesetFingerprint: summary.rulesetFingerprint,
        },
        () => this.showSummary(summary, "summary"),
      );
    } else if (response.kind === "job") {
      this.judge({ requestSeq, resultVersion: response.resultVersion }, () =>
        this.startJob(response, requestSeq),
      );
    } else {
      this.judge({ requestSeq }, () => {
        this.pending = undefined;
        this.move("not_applicable", { reasonKey: response.reasonKey });
      });
    }
  }

  private showSummary(summary: GroupSummary, event: "summary" | "job_summary") {
    this.pending = undefined;
    this.stopWatchingJob();
    // The folder stays what was asked for, whatever the answer echoes.
    this.target = {
      resultVersion: summary.resultVersion,
      scope: this.target.scope,
    };
    this.condition = {
      ...this.condition,
      resultVersion: summary.resultVersion,
    };
    const newer = this.state.newerResult;
    const stillNewer = newer !== undefined && newer !== summary.resultVersion;
    this.move(event, {
      summary,
      outcome: undefined,
      reasonKey: undefined,
      staleReason: undefined,
      jobRef: undefined,
      newerResult: stillNewer ? newer : undefined,
      rulesChanged:
        this.state.ruleset !== undefined &&
        this.state.ruleset.fingerprint !== summary.rulesetFingerprint,
    });
    if (stillNewer) this.move("version_changed");
  }

  private startJob(
    response: Extract<GroupClassifyResponse, { kind: "job" }>,
    seq: number,
  ) {
    const { jobRef, classificationRef, resultVersion } = response;
    this.pending = { seq, jobRef, classificationRef, resultVersion };
    this.condition = { ...this.condition, resultVersion };
    if (!this.move("job", { jobRef })) return;
    const read = () => void this.readJob(jobRef);
    this.stopJob = watchJobBaseline(this.client, jobRef, read, read).dispose;
  }

  private stopWatchingJob() {
    this.stopJob?.();
    this.stopJob = undefined;
  }

  private async readJob(jobRef: Id) {
    const job = this.pending;
    if (this.disposed || job?.jobRef !== jobRef) return;
    if (this.readingJob) {
      // An event arrived during a read; read again so a terminal state is not missed.
      this.rereadJob = true;
      return;
    }
    this.readingJob = true;
    try {
      const status = await this.client.call("job.get", { jobRef });
      if (this.pending !== job || status.jobRef !== jobRef) return;
      if (status.state === "done") await this.readJobSummary(job);
      else if (status.state === "cancelled") this.endJob({ kind: "cancelled" });
      else if (status.state === "failed" || status.state === "partial")
        this.endJob({
          kind: "failed",
          reasonKey: status.reasonKey ?? undefined,
          retryable: true,
        });
    } catch {
      // A failed read is retried by the next job event or baseline restart.
    } finally {
      this.readingJob = false;
      if (this.rereadJob) {
        this.rereadJob = false;
        void this.readJob(jobRef);
      }
    }
  }

  private async readJobSummary(job: Pending) {
    if (!job.classificationRef) return;
    try {
      const summary = await this.client.call("group.summary", {
        classificationRef: job.classificationRef,
      });
      if (this.pending !== job) return;
      this.judge(
        {
          requestSeq: job.seq,
          resultVersion: summary.resultVersion,
          rulesetVersion: summary.rulesetVersion,
          rulesetFingerprint: summary.rulesetFingerprint,
        },
        () => {
          if (
            summary.classificationRef !== job.classificationRef ||
            summary.resultVersion !== job.resultVersion
          )
            this.endJob({ kind: "failed", retryable: true });
          else this.showSummary(summary, "job_summary");
        },
      );
    } catch (error) {
      if (this.pending !== job) return;
      const failure = describeFailure(error);
      if (isRequeryReason(failure.reasonKey)) this.markStale(failure.reasonKey);
      else this.endJob({ kind: "failed", ...failure });
    }
  }

  private endJob(outcome: GroupsOutcome) {
    this.pending = undefined;
    this.stopWatchingJob();
    this.move("job_ended", { outcome, jobRef: undefined });
  }

  private failRequest(error: unknown, seq: number | undefined) {
    if (this.disposed || (seq !== undefined && seq !== this.channel.last))
      return;
    const failure = describeFailure(error);
    if (isRequeryReason(failure.reasonKey)) {
      this.markStale(failure.reasonKey);
      return;
    }
    this.pending = undefined;
    this.move("request_failed", {
      outcome: { kind: "failed", ...failure },
      reasonKey: failure.reasonKey,
    });
  }

  private markStale(reasonKey: string | undefined) {
    const staleReason: GroupsStaleReason =
      reasonKey === "group.rules_modified"
        ? "rules"
        : reasonKey === "group.result_expired"
          ? "expired"
          : "version";
    this.pending = undefined;
    this.stopWatchingJob();
    this.move("stale", { staleReason, jobRef: undefined });
  }

  private onEvent(event: EventEnvelope) {
    if (this.disposed || event.topic !== "results.version_changed") return;
    const version = eventVersion(event);
    const known =
      this.state.summary?.resultVersion ?? this.condition.resultVersion;
    if (version !== undefined && version === known) return;
    this.patch({ newerResult: version ?? "unknown" });
    this.move("version_changed");
  }
}
