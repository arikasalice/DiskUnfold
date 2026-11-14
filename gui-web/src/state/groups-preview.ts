import { newCorrelationId } from "../ipc";
import type {
  AnalysisClient,
  GroupRuleDraft,
  GroupSummary,
  GroupTotals,
  Id,
  ItemRef,
} from "../ipc";
import { toRuleDraft } from "./groups-conditions";
import type { SizeSystem } from "./groups-conditions";
import { describeFailure } from "./groups-controller";
import { isInputBlocker } from "./groups-editor";
import type { GroupRuleEditor } from "./groups-editor";
import { ViewChannel, judgeResponse } from "./groups-view-machine";
import type { ViewCondition } from "./groups-view-machine";
import { watchJobBaseline } from "./job-baseline";

/** How long the draft must stay unchanged before a preview is requested. */
export const PREVIEW_DELAY_MS = 400;

/** Where the preview of the open editor stands. */
export type PreviewPhase =
  /** The draft equals the stored rules, so the applied result is the preview. */
  | "applied"
  /** The draft changed and the request waits for typing to pause. */
  | "waiting"
  | "computing"
  /** `result` answers the current draft. */
  | "ready"
  /** An input problem keeps the draft from being previewed. */
  | "blocked"
  | "failed"
  | "not_applicable";

/** An unsaved classification of the draft rules. */
export type PreviewResult = {
  /** Draft keys in the order the rules were sent; summary rows follow it. */
  keys: readonly string[];
  summary: GroupSummary;
  /** Absent when the totals could not be read; sizes then read as unknown. */
  totals: GroupTotals | undefined;
};

/** Immutable view model published by {@link GroupPreview}. */
export type PreviewSnapshot = {
  phase: PreviewPhase;
  /** The last answer. While the phase is not `ready` it describes an earlier draft. */
  result: PreviewResult | undefined;
  reasonKey: string | undefined;
};

/** The result the preview classifies; it is the one the screen shows. */
export type PreviewTarget = { resultVersion: Id; scope: ItemRef | undefined };

type Request = { keys: readonly string[]; rules: GroupRuleDraft[] };

/**
 * Previews the editor's draft: after the draft has been quiet for a moment it
 * classifies the draft rules on its own view channel and keeps only the newest
 * answer. Nothing is stored; the stored rules and the shown classification are
 * never touched.
 */
export class GroupPreview {
  readonly channel: ViewChannel;
  private state: PreviewSnapshot = {
    phase: "applied",
    result: undefined,
    reasonKey: undefined,
  };
  private readonly listeners = new Set<() => void>();
  private readonly stopEditor: () => void;
  private drafts: unknown;
  private timer: ReturnType<typeof setTimeout> | undefined;
  private token = 0;
  private running = false;
  private stopJob: (() => void) | undefined;
  private last: Request | undefined;
  private disposed = false;
  private readingJob = false;
  private rereadJob = false;

  constructor(
    private readonly client: AnalysisClient,
    private readonly system: SizeSystem,
    private readonly editor: GroupRuleEditor,
    private readonly target: () => PreviewTarget,
    private readonly delayMs: number = PREVIEW_DELAY_MS,
    viewRef: Id = newCorrelationId(),
  ) {
    this.channel = new ViewChannel(viewRef);
    this.stopEditor = editor.subscribe(() => this.onEditor());
  }

  getSnapshot = (): PreviewSnapshot => this.state;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** Requests the last draft again after a failure. */
  retry(): void {
    if (this.last && !this.disposed && this.state.phase === "failed")
      void this.start(this.last);
  }

  dispose(): void {
    if (this.disposed) return;
    this.reset();
    this.disposed = true;
    this.stopEditor();
    this.listeners.clear();
  }

  private onEditor() {
    if (this.disposed) return;
    const editor = this.editor.getSnapshot();
    if (editor.phase === "viewing") {
      if (this.drafts !== undefined) this.reset();
      return;
    }
    // Selecting a group or a save phase does not change what is classified.
    if (editor.drafts === this.drafts) return;
    this.drafts = editor.drafts;
    this.cancelWork();
    if (!editor.dirty) {
      this.set({ phase: "applied", result: undefined, reasonKey: undefined });
      return;
    }
    if (isInputBlocker(editor.blocker)) {
      this.set({ ...this.state, phase: "blocked", reasonKey: undefined });
      return;
    }
    const request: Request = {
      keys: editor.drafts.map((rule) => rule.key),
      rules: editor.drafts.map((rule) => toRuleDraft(rule, this.system)),
    };
    this.last = request;
    this.set({ ...this.state, phase: "waiting", reasonKey: undefined });
    this.timer = setTimeout(() => {
      this.timer = undefined;
      void this.start(request);
    }, this.delayMs);
  }

  /** The editor closed: drop pending and running work and forget the answer. */
  private reset() {
    this.cancelWork();
    this.drafts = undefined;
    this.last = undefined;
    this.set({ phase: "applied", result: undefined, reasonKey: undefined });
  }

  /** Makes every pending or running request irrelevant. */
  private cancelWork() {
    if (this.timer !== undefined) clearTimeout(this.timer);
    this.timer = undefined;
    this.token += 1;
    this.stopJob?.();
    this.stopJob = undefined;
    if (this.running)
      void this.client
        .call("group.cancel", {
          kind: "view",
          viewRef: this.channel.viewRef,
          requestSeq: this.channel.last,
        })
        .catch(() => {});
    this.running = false;
  }

  private async start(request: Request) {
    if (this.disposed) return;
    const token = ++this.token;
    const target = this.target();
    const requestSeq = this.channel.next();
    this.running = true;
    this.set({ ...this.state, phase: "computing", reasonKey: undefined });
    // A draft has no stored rule set, so only the result and folder are pinned.
    const condition: ViewCondition = {
      resultVersion: target.resultVersion,
      scope: target.scope,
      rulesetVersion: 0,
      rulesetFingerprint: undefined,
    };
    try {
      const response = await this.client.call("group.classify", {
        viewRef: this.channel.viewRef,
        requestSeq,
        resultVersion: target.resultVersion,
        ...(target.scope !== undefined ? { scope: target.scope } : {}),
        rules: { kind: "draft", rules: request.rules },
      });
      if (token !== this.token) return;
      const verdict = judgeResponse(this.channel, condition, {
        requestSeq: response.requestSeq,
        ...(response.kind === "summary"
          ? {
              resultVersion: response.summary.resultVersion,
              rulesetVersion: response.summary.rulesetVersion,
            }
          : {}),
        ...(response.kind === "job"
          ? { resultVersion: response.resultVersion }
          : {}),
      });
      if (verdict === "superseded") return;
      if (verdict === "mismatch") {
        this.fail(token, "result.version_stale");
      } else if (response.kind === "summary") {
        await this.finish(token, request, response.summary);
      } else if (response.kind === "job") {
        this.watch(token, request, response.jobRef, response.classificationRef);
      } else {
        this.running = false;
        this.set({
          phase: "not_applicable",
          result: undefined,
          reasonKey: response.reasonKey,
        });
      }
    } catch (error) {
      if (token === this.token)
        this.fail(token, describeFailure(error).reasonKey);
    }
  }

  private fail(token: number, reasonKey: string | undefined) {
    if (token !== this.token) return;
    this.running = false;
    this.stopJob?.();
    this.stopJob = undefined;
    this.set({ phase: "failed", result: undefined, reasonKey });
  }

  /** Reads the totals that go with a summary and publishes the answer. */
  private async finish(token: number, request: Request, summary: GroupSummary) {
    if (summary.groups.length !== request.keys.length) {
      this.fail(token, undefined);
      return;
    }
    let totals: GroupTotals | undefined;
    try {
      const read = await this.client.call("group.totals", {
        classificationRef: summary.classificationRef,
        groupIds: [],
      });
      if (
        read.classificationRef === summary.classificationRef &&
        read.resultVersion === summary.resultVersion
      )
        totals = read;
    } catch {
      // The sizes then read as unknown; the counts are still valid.
    }
    if (token !== this.token) return;
    this.running = false;
    this.stopJob?.();
    this.stopJob = undefined;
    this.set({
      phase: "ready",
      result: { keys: request.keys, summary, totals },
      reasonKey: undefined,
    });
  }

  private watch(
    token: number,
    request: Request,
    jobRef: Id,
    classificationRef: Id,
  ) {
    const read = () =>
      void this.readJob(token, request, jobRef, classificationRef);
    this.stopJob = watchJobBaseline(this.client, jobRef, read, read).dispose;
  }

  private async readJob(
    token: number,
    request: Request,
    jobRef: Id,
    classificationRef: Id,
  ) {
    if (token !== this.token) return;
    if (this.readingJob) {
      // An event arrived during a read; read again so a terminal state is not missed.
      this.rereadJob = true;
      return;
    }
    this.readingJob = true;
    try {
      const status = await this.client.call("job.get", { jobRef });
      if (token !== this.token || status.jobRef !== jobRef) return;
      if (status.state === "done") {
        const summary = await this.client.call("group.summary", {
          classificationRef,
        });
        if (
          token === this.token &&
          summary.classificationRef === classificationRef
        )
          await this.finish(token, request, summary);
      } else if (["cancelled", "failed", "partial"].includes(status.state)) {
        this.fail(token, status.reasonKey ?? undefined);
      }
    } catch (error) {
      // A failed read is retried by the next job event or baseline restart.
      void error;
    } finally {
      this.readingJob = false;
      if (this.rereadJob) {
        this.rereadJob = false;
        void this.readJob(token, request, jobRef, classificationRef);
      }
    }
  }

  private set(state: PreviewSnapshot) {
    this.state = state;
    for (const listener of [...this.listeners]) listener();
  }
}
