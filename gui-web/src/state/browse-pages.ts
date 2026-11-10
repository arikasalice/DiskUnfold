import type {
  FolderQueryPage,
  FolderQueryRow,
  QueryPageRequest,
  QueryPageResponse,
} from "../ipc";

export const BROWSE_PAGE_SIZE = 200;
const MAX_PAGES = 10;

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
  private readonly pages = new Map<number, readonly FolderQueryRow[]>();

  constructor(private readonly first: FolderQueryPage) {
    this.count = first.total.count;
    this.rowCount = first.total.state === "exact" ? this.count + 1 : -1;
    this.pages.set(0, first.rows);
  }

  request(start: number): QueryPageRequest {
    return {
      queryRef: this.first.queryRef,
      resultVersion: this.first.resultVersion,
      start,
      count: BROWSE_PAGE_SIZE,
    };
  }

  has(start: number): boolean {
    return this.pages.has(start);
  }

  row(index: number): FolderQueryRow | undefined {
    const start = browsePageStart(index);
    return this.pages.get(start)?.[index - start];
  }

  touch(start: number): void {
    const rows = this.pages.get(start);
    if (!rows) return;
    this.pages.delete(start);
    this.pages.set(start, rows);
  }

  accept(
    start: number,
    page: QueryPageResponse,
    protectedStarts: ReadonlySet<number>,
  ): boolean {
    if (
      start < 0 ||
      start >= this.count ||
      browsePageStart(start) !== start ||
      page.rowKind !== "folder" ||
      page.queryRef !== this.first.queryRef ||
      page.resultVersion !== this.first.resultVersion ||
      page.total.state !== this.first.total.state ||
      page.total.count !== this.count ||
      page.rows.length !== Math.min(BROWSE_PAGE_SIZE, this.count - start) ||
      page.rows.some((row) => row.item.resultVersion !== page.resultVersion)
    ) {
      return false;
    }

    // Every cached key must identify one absolute position. Repeated reads of
    // the same page may refresh its entries, including after a failed request.
    const seen = new Set(
      [...this.pages]
        .filter(([offset]) => offset !== start)
        .flatMap(([, rows]) => rows.map((row) => row.item.itemNo)),
    );
    for (const { item } of page.rows) {
      if (seen.has(item.itemNo)) return false;
      seen.add(item.itemNo);
    }

    this.pages.delete(start);
    this.pages.set(start, page.rows);
    for (const offset of this.pages.keys()) {
      if (this.pages.size <= MAX_PAGES) break;
      if (!protectedStarts.has(offset)) this.pages.delete(offset);
    }
    if (this.pages.size > MAX_PAGES) this.pages.delete(start);
    return this.pages.has(start);
  }
}
