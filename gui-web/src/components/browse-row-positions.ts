const OVERSCAN = 10;

export type BrowseRowWindow = {
  visibleStart: number;
  visibleEnd: number;
  indexes: number[];
};

/** Absolute row coordinates without allocating an entry for unmeasured rows. */
export class BrowseRowPositions {
  private readonly measured: number[];
  private readonly adjustments = [0];

  constructor(
    readonly count: number,
    private readonly estimate: number,
    sizes: ReadonlyMap<number, number>,
  ) {
    this.measured = [...sizes.keys()]
      .filter((index) => index >= 0 && index < count)
      .sort((left, right) => left - right);
    for (const index of this.measured) {
      const previous = this.adjustments.at(-1) ?? 0;
      this.adjustments.push(
        previous + (sizes.get(index) ?? estimate) - estimate,
      );
    }
  }

  offset(index: number): number {
    const measuredBefore = lowerBound(
      this.measured.length,
      (position) => (this.measured[position] ?? 0) >= index,
    );
    return index * this.estimate + (this.adjustments[measuredBefore] ?? 0);
  }

  get height(): number {
    return this.offset(this.count);
  }

  at(position: number): number {
    return lowerBound(this.count, (index) => this.offset(index + 1) > position);
  }

  window(
    top: number,
    bottom: number,
    pins: readonly number[],
  ): BrowseRowWindow {
    const visibleStart = this.at(top);
    const visibleEnd =
      bottom > Math.max(0, top) && visibleStart < this.count
        ? lowerBound(this.count, (index) => this.offset(index) >= bottom)
        : visibleStart;
    const outside = [...new Set(pins)].filter(
      (index) =>
        index >= 0 &&
        index < this.count &&
        (index < visibleStart || index >= visibleEnd),
    );
    const indexes = new Set(outside);
    const start = Math.max(0, visibleStart - OVERSCAN);
    const end = Math.min(this.count, visibleEnd + OVERSCAN);
    const limit = visibleEnd - visibleStart + 2 * OVERSCAN;
    for (let index = visibleStart; index < visibleEnd; index += 1) {
      indexes.add(index);
    }
    for (let distance = 1; distance <= OVERSCAN; distance += 1) {
      const before = visibleStart - distance;
      const after = visibleEnd + distance - 1;
      if (indexes.size < limit && before >= start) indexes.add(before);
      if (indexes.size < limit && after < end) indexes.add(after);
    }
    return {
      visibleStart,
      visibleEnd,
      indexes: [...indexes].sort((left, right) => left - right),
    };
  }
}

function lowerBound(
  length: number,
  matches: (index: number) => boolean,
): number {
  let start = 0;
  let end = length;
  while (start < end) {
    const middle = start + Math.floor((end - start) / 2);
    if (matches(middle)) end = middle;
    else start = middle + 1;
  }
  return start;
}
