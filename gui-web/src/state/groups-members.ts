import type {
  AnalysisClient,
  BrowseSortKey,
  GroupCategory,
  GroupMemberRow,
  Id,
  ItemRef,
} from "../ipc";
import { describeFailure } from "./groups-controller";
import { isRequeryReason } from "./groups-view-machine";

/** Rows requested by one group.members call. The wire limit is 1000. */
export const MEMBER_CHUNK = 100;
const MAX_CHUNKS = 40;

/** Stable row key: a member row is identified by its item reference. */
export function memberRowKey(item: ItemRef): string {
  return `${item.resultVersion}:${item.itemNo}`;
}

function categoryKey(category: GroupCategory): string {
  return category.kind === "group"
    ? `group:${category.groupId}`
    : category.kind;
}

function sortKey(sort: BrowseSortKey | undefined): string {
  if (!sort) return "default";
  return `${sort.id}@${sort.revision}:${sort.descending ? "desc" : "asc"}:${sort.sizeBasis ?? ""}`;
}

/** One member list: a category of one classification in one order. */
export type MembersView = {
  classificationRef: Id;
  resultVersion: Id;
  category: GroupCategory;
  sort?: BrowseSortKey | undefined;
  /** The count the summary announced for this category. */
  count: number;
};

/** The cache key: classification, category and sort. Any change drops the cache. */
export function membersCacheKey(view: MembersView): string {
  return [
    view.classificationRef,
    categoryKey(view.category),
    sortKey(view.sort),
  ].join("|");
}

/** A bounded, sparse cache of member rows by position. */
export class MemberCache {
  private readonly chunks = new Map<number, readonly GroupMemberRow[]>();

  constructor(
    readonly key: string,
    readonly total: number,
  ) {}

  row(index: number): GroupMemberRow | undefined {
    const start = Math.floor(index / MEMBER_CHUNK) * MEMBER_CHUNK;
    return this.chunks.get(start)?.[index - start];
  }

  has(start: number): boolean {
    return this.chunks.has(start);
  }

  store(start: number, rows: readonly GroupMemberRow[]): void {
    this.chunks.delete(start);
    this.chunks.set(start, rows);
  }

  /** Chunk starts that overlap [first, end) and are not cached. */
  missing(first: number, end: number): number[] {
    const from = Math.max(0, first);
    const to = Math.min(this.total, end);
    const starts: number[] = [];
    for (
      let start = Math.floor(from / MEMBER_CHUNK) * MEMBER_CHUNK;
      start < to;
      start += MEMBER_CHUNK
    )
      if (!this.chunks.has(start)) starts.push(start);
    return starts;
  }

  /** Drops the oldest chunks outside [first, end) once the bound is exceeded. */
  trim(first: number, end: number): void {
    for (const start of this.chunks.keys()) {
      if (this.chunks.size <= MAX_CHUNKS) break;
      if (start + MEMBER_CHUNK <= first || start >= end)
        this.chunks.delete(start);
    }
  }
}

/** View model of a {@link GroupMembersReader}. */
export type MembersSnapshot = {
  key: string | undefined;
  total: number;
  /** Changes whenever a row arrives, so a list can render again. */
  revision: number;
  loading: boolean;
  failed: { retryable: boolean } | undefined;
};

/**
 * Reads member rows for the rows on screen and one screen before and after.
 * A change of classification, category or sort empties the cache, and an
 * answer that belongs to an earlier view is dropped.
 */
export class GroupMembersReader {
  private view: MembersView | undefined;
  private cache: MemberCache | undefined;
  private generation = 0;
  private window = { start: 0, end: 0 };
  private readonly inFlight = new Set<number>();
  private readonly failedChunks = new Set<number>();
  private state: MembersSnapshot = {
    key: undefined,
    total: 0,
    revision: 0,
    loading: false,
    failed: undefined,
  };
  private readonly listeners = new Set<() => void>();
  private disposed = false;

  constructor(
    private readonly client: AnalysisClient,
    /** Called when a read shows that the classification can no longer be read. */
    private readonly onUnreadable: (error: unknown) => boolean,
  ) {}

  getSnapshot = (): MembersSnapshot => this.state;

  subscribe = (listener: () => void) => {
    this.listeners.add(listener);
    return () => {
      this.listeners.delete(listener);
    };
  };

  /** Chooses the list to read; the same view is a no-op. */
  setView(view: MembersView | undefined): void {
    const key = view ? membersCacheKey(view) : undefined;
    if (key === this.state.key) return;
    this.generation += 1;
    this.inFlight.clear();
    this.failedChunks.clear();
    this.view = view;
    this.cache = view && key ? new MemberCache(key, view.count) : undefined;
    this.publish({
      key,
      total: view?.count ?? 0,
      loading: false,
      failed: undefined,
    });
    this.pump();
  }

  /** The cached row at a position, if it has arrived. */
  row(index: number): GroupMemberRow | undefined {
    return this.cache?.row(index);
  }

  /** Reports the visible rows [start, end); requests what is missing around them. */
  show(start: number, end: number): void {
    this.window = { start, end };
    this.pump();
  }

  /** Requests the chunk holding one position, for example before moving focus to it. */
  ensure(index: number): void {
    const { view, cache } = this;
    if (this.disposed || !view || !cache) return;
    for (const start of cache.missing(index, index + 1))
      if (!this.inFlight.has(start) && !this.failedChunks.has(start))
        void this.fetch(view, cache, start);
  }

  /** Requests again the chunks whose read failed. */
  retry(): void {
    this.failedChunks.clear();
    this.publish({ failed: undefined });
    this.pump();
  }

  dispose(): void {
    this.disposed = true;
    this.generation += 1;
    this.listeners.clear();
  }

  private publish(patch: Partial<MembersSnapshot>) {
    this.state = { ...this.state, ...patch, revision: this.state.revision + 1 };
    for (const listener of [...this.listeners]) listener();
  }

  private pump() {
    const { view, cache } = this;
    if (this.disposed || !view || !cache) return;
    const span = Math.max(1, this.window.end - this.window.start);
    const wanted = cache.missing(
      this.window.start - span,
      this.window.end + span,
    );
    for (const start of wanted)
      if (!this.inFlight.has(start) && !this.failedChunks.has(start))
        void this.fetch(view, cache, start);
    cache.trim(this.window.start - span, this.window.end + span);
  }

  private async fetch(view: MembersView, cache: MemberCache, start: number) {
    const generation = this.generation;
    const count = Math.min(MEMBER_CHUNK, cache.total - start);
    if (count <= 0) return;
    this.inFlight.add(start);
    if (!this.state.loading) this.publish({ loading: true });
    try {
      const members = await this.client.call("group.members", {
        classificationRef: view.classificationRef,
        resultVersion: view.resultVersion,
        category: view.category,
        ...(view.sort ? { sort: view.sort } : {}),
        start,
        count,
      });
      if (this.disposed || generation !== this.generation) return;
      this.inFlight.delete(start);
      if (
        members.classificationRef !== view.classificationRef ||
        members.resultVersion !== view.resultVersion ||
        members.start !== start ||
        members.rows.length > count
      ) {
        this.failChunk(start, false);
        return;
      }
      if (members.total !== cache.total) {
        // The category size differs from the summary; trust the answer.
        this.generation += 1;
        this.inFlight.clear();
        this.cache = new MemberCache(cache.key, members.total);
      }
      this.cache?.store(start, members.rows);
      this.publish({
        total: members.total,
        loading: this.inFlight.size > 0,
        failed: undefined,
      });
      this.pump();
    } catch (error) {
      if (this.disposed || generation !== this.generation) return;
      this.inFlight.delete(start);
      if (isRequeryReason(describeFailure(error).reasonKey)) {
        this.onUnreadable(error);
        this.publish({ loading: false });
        return;
      }
      this.failChunk(start, describeFailure(error).retryable);
    }
  }

  private failChunk(start: number, retryable: boolean) {
    this.failedChunks.add(start);
    this.publish({ loading: this.inFlight.size > 0, failed: { retryable } });
  }
}
