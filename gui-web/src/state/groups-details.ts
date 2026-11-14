import type {
  AnalysisClient,
  GroupExplanation,
  GroupTotals,
  Id,
  ItemRef,
} from "../ipc";
import { describeFailure } from "./groups-controller";
import { isRequeryReason, sameItemRef } from "./groups-view-machine";

/** State of the newest read of one kind of detail. */
export type ReadState<K, V> =
  | { phase: "idle" }
  | { phase: "loading"; key: K }
  | { phase: "ready"; key: K; value: V }
  | { phase: "error"; key: K; reasonKey: string | undefined };

/**
 * Runs one read at a time per key and keeps only the newest answer. A key
 * change drops an older answer even if it arrives later.
 */
export class LatestRead<K, V> {
  private state: ReadState<K, V> = { phase: "idle" };
  private token = 0;
  private disposed = false;
  private readonly listeners = new Set<() => void>();

  constructor(
    private readonly read: (key: K) => Promise<V>,
    private readonly same: (left: K, right: K) => boolean,
    /** Receives each failure; returns true when it means the result is stale. */
    private readonly onFailure: (error: unknown) => boolean,
  ) {}

  getSnapshot = (): ReadState<K, V> => this.state;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** Starts reading unless the same key is already loading or loaded. */
  load(key: K): void {
    const current = this.state;
    if (
      current.phase !== "idle" &&
      current.phase !== "error" &&
      this.same(current.key, key)
    )
      return;
    void this.run(key);
  }

  /** Reads again after a failure. */
  retry(): void {
    if (this.state.phase === "error") void this.run(this.state.key);
  }

  /** Forgets the current answer and ignores any read still in flight. */
  clear(): void {
    this.token += 1;
    this.set({ phase: "idle" });
  }

  dispose(): void {
    this.disposed = true;
    this.token += 1;
    this.listeners.clear();
  }

  private set(state: ReadState<K, V>) {
    this.state = state;
    for (const listener of [...this.listeners]) listener();
  }

  private async run(key: K) {
    const token = ++this.token;
    this.set({ phase: "loading", key });
    try {
      const value = await this.read(key);
      if (!this.disposed && token === this.token)
        this.set({ phase: "ready", key, value });
    } catch (error) {
      if (this.disposed || token !== this.token) return;
      this.onFailure(error);
      this.set({
        phase: "error",
        key,
        reasonKey: describeFailure(error).reasonKey,
      });
    }
  }
}

/** The classification a totals read belongs to. */
export type TotalsKey = { classificationRef: Id; resultVersion: Id };

/** Reads the set totals of every group in one classification. */
export function createTotalsRead(
  client: AnalysisClient,
  onFailure: (error: unknown) => boolean,
): LatestRead<TotalsKey, GroupTotals> {
  return new LatestRead(
    async (key) => {
      // An empty selection asks for every group.
      const totals = await client.call("group.totals", {
        classificationRef: key.classificationRef,
        groupIds: [],
      });
      if (
        totals.classificationRef !== key.classificationRef ||
        totals.resultVersion !== key.resultVersion
      )
        throw new Error("The totals belong to a different classification.");
      return totals;
    },
    (left, right) =>
      left.classificationRef === right.classificationRef &&
      left.resultVersion === right.resultVersion,
    onFailure,
  );
}

/** The member whose classification reasons are read. */
export type ExplainKey = { classificationRef: Id; item: ItemRef };

/** Reads why one file is, or is not, in each group. */
export function createExplainRead(
  client: AnalysisClient,
  onFailure: (error: unknown) => boolean,
): LatestRead<ExplainKey, GroupExplanation> {
  return new LatestRead(
    async (key) => {
      const explanation = await client.call("group.explain", {
        classificationRef: key.classificationRef,
        item: key.item,
      });
      if (
        explanation.classificationRef !== key.classificationRef ||
        !sameItemRef(explanation.item, key.item)
      )
        throw new Error("The explanation belongs to a different file.");
      return explanation;
    },
    (left, right) =>
      left.classificationRef === right.classificationRef &&
      sameItemRef(left.item, right.item),
    onFailure,
  );
}

/** Wraps a controller's stale-reason reporter so reads can share it. */
export function staleReporter(
  report: (error: unknown) => boolean,
): (error: unknown) => boolean {
  return (error) =>
    isRequeryReason(describeFailure(error).reasonKey) && report(error);
}
