import type {
  FolderQueryPage,
  FolderQueryRow,
  QueryPageRequest,
  QueryPageResponse,
} from "../ipc";

export const BROWSE_PAGE_SIZE = 200;
const MAX_PAGES = 10;
const MAX_QUERY_POSITION = 0xffff_ffff;
type CachedPage = Pick<FolderQueryPage, "rows" | "nextCursor">;

export function browsePageStart(index: number): number {
  return Math.floor(index / BROWSE_PAGE_SIZE) * BROWSE_PAGE_SIZE;
}

export function visibleBrowsePages(
  start: number,
  end: number,
  count: number,
): number[] {
  if (start >= end || start >= count) return [];
  const first = browsePageStart(start);
  const last = Math.min(browsePageStart(end - 1) + BROWSE_PAGE_SIZE, count - 1);
  const pages: number[] = [];
  for (let page = first; page <= last; page += BROWSE_PAGE_SIZE)
    pages.push(page);
  return pages;
}

/** The cache is scoped to one opened query and its pinned result version. */
export class BrowsePages {
  readonly count: number;
  readonly rowCount: number;
  private readonly queryRef: string;
  private readonly resultVersion: string;
  private readonly totalState: FolderQueryPage["total"]["state"];
  private readonly pages = new Map<number, CachedPage>();

  constructor(first: FolderQueryPage) {
    this.count = first.total.count;
    this.rowCount = first.total.state === "exact" ? this.count + 1 : -1;
    this.queryRef = first.queryRef;
    this.resultVersion = first.resultVersion;
    this.totalState = first.total.state;
    if (
      !Number.isSafeInteger(this.count) ||
      this.count < 0 ||
      this.count > MAX_QUERY_POSITION + 1 ||
      first.rows.length !== Math.min(BROWSE_PAGE_SIZE, this.count) ||
      !this.validRows(0, first)
    ) {
      throw new Error("The initial folder page does not match its query.");
    }
    this.pages.set(0, { rows: first.rows, nextCursor: first.nextCursor });
  }

  request(start: number): QueryPageRequest {
    return {
      queryRef: this.queryRef,
      resultVersion: this.resultVersion,
      start,
      count: BROWSE_PAGE_SIZE,
    };
  }

  has(start: number): boolean {
    return this.pages.has(start);
  }

  row(index: number): FolderQueryRow | undefined {
    const start = browsePageStart(index);
    return this.pages.get(start)?.rows[index - start];
  }

  nextCursor(start: number): string | null | undefined {
    return this.pages.get(start)?.nextCursor;
  }

  touch(start: number): void {
    const page = this.pages.get(start);
    if (!page) return;
    this.pages.delete(start);
    this.pages.set(start, page);
  }

  matches(
    page: QueryPageResponse,
  ): page is Extract<QueryPageResponse, { rowKind: "folder" }> {
    return (
      page.rowKind === "folder" &&
      page.queryRef === this.queryRef &&
      page.resultVersion === this.resultVersion &&
      page.total.state === this.totalState &&
      page.total.count === this.count
    );
  }

  private validRows(start: number, page: CachedPage): boolean {
    const seen = new Set(
      [...this.pages]
        .filter(([offset]) => offset !== start)
        .flatMap(([, cached]) => cached.rows.map((row) => row.item.itemNo)),
    );
    const previous = this.pages.get(start)?.rows;
    return page.rows.every(({ item }, index) => {
      if (
        item.resultVersion !== this.resultVersion ||
        seen.has(item.itemNo) ||
        (previous !== undefined && previous[index]?.item.itemNo !== item.itemNo)
      )
        return false;
      seen.add(item.itemNo);
      return true;
    });
  }

  accept(
    start: number,
    page: QueryPageResponse,
    protectedStarts: ReadonlySet<number>,
  ): boolean {
    if (
      start < 0 ||
      start >= this.count ||
      start > MAX_QUERY_POSITION ||
      browsePageStart(start) !== start ||
      !this.matches(page) ||
      page.rows.length !== Math.min(BROWSE_PAGE_SIZE, this.count - start) ||
      !this.validRows(start, page)
    )
      return false;
    this.pages.delete(start);
    this.pages.set(start, { rows: page.rows, nextCursor: page.nextCursor });
    for (const offset of this.pages.keys()) {
      if (this.pages.size <= MAX_PAGES) break;
      if (!protectedStarts.has(offset)) this.pages.delete(offset);
    }
    if (this.pages.size > MAX_PAGES) this.pages.delete(start);
    return this.pages.has(start);
  }
}
