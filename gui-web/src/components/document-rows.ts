import { useCallback, useLayoutEffect, useRef, useState } from "react";

const OVERSCAN = 10;
const INITIAL_ROWS = 10;
const WIDE_ESTIMATE = 64;
const NARROW_ESTIMATE = 160;

function rowOffsets(
  count: number,
  sizes: ReadonlyMap<number, number>,
  estimate: number,
) {
  const offsets = [0];
  for (let index = 0; index < count; index += 1) {
    offsets.push((offsets[index] ?? 0) + (sizes.get(index) ?? estimate));
  }
  return offsets;
}

// Find the row containing the given offset, allowing count at the end.
function rowAt(offsets: readonly number[], position: number) {
  let lo = 0;
  let hi = offsets.length - 1;
  while (lo < hi) {
    const middle = Math.floor((lo + hi + 1) / 2);
    if ((offsets[middle] ?? Infinity) <= position) lo = middle;
    else hi = middle - 1;
  }
  return lo;
}

function initialLayout(count: number) {
  return {
    offsets: rowOffsets(count, new Map(), WIDE_ESTIMATE),
    start: 0,
    end: Math.min(count, INITIAL_ROWS + OVERSCAN),
  };
}

function windowRows(offsets: number[], top: number, height: number) {
  const count = offsets.length - 1;
  const start = rowAt(offsets, Math.max(0, top));
  const end =
    height > 0
      ? rowAt(offsets, Math.max(0, top + height)) + 1
      : start + INITIAL_ROWS;
  return {
    offsets,
    start: Math.max(0, start - OVERSCAN),
    end: Math.min(count, end + OVERSCAN),
  };
}

function readRows(body: HTMLTableSectionElement, sizes: Map<number, number>) {
  let changed = false;
  for (const row of body.querySelectorAll<HTMLTableRowElement>("[data-row]")) {
    const index = Number(row.dataset.row);
    const height = row.getBoundingClientRect().height;
    if (height > 0 && sizes.get(index) !== height) {
      sizes.set(index, height);
      changed = true;
    }
  }
  return changed;
}

/** Virtualizes only the loaded rows in the document's existing scroll area. */
export function useDocumentRows(count: number) {
  const frame = useRef<HTMLDivElement>(null);
  const body = useRef<HTMLTableSectionElement>(null);
  const observer = useRef<ResizeObserver>(null);
  const [layout, setLayout] = useState(() => initialLayout(count));
  const current = useRef(layout);
  const sizes = useRef(new Map<number, number>());
  const width = useRef(0);
  const anchor = useRef<{ position: number; scrollY: number }>(undefined);

  const watchRow = useCallback((row: HTMLTableRowElement | null) => {
    if (!row) return;
    observer.current?.observe(row);
    return () => observer.current?.unobserve(row);
  }, []);

  useLayoutEffect(() => {
    const root = frame.current;
    const section = body.current;
    if (!root || !section) return;
    let request: number | undefined;

    function refresh() {
      request = undefined;
      if (!root || !section) return;
      const before = current.current;
      const top = -section.getBoundingClientRect().top;
      const nextWidth = root.getBoundingClientRect().width;
      const widthChanged = nextWidth !== width.current;
      if (widthChanged) {
        sizes.current.clear();
        width.current = nextWidth;
      }
      const measured = readRows(section, sizes.current);
      const changed =
        widthChanged || measured || before.offsets.length !== count + 1;
      const offsets = changed
        ? rowOffsets(
            count,
            sizes.current,
            nextWidth > 0 && nextWidth <= 640 ? NARROW_ESTIMATE : WIDE_ESTIMATE,
          )
        : before.offsets;
      const index = rowAt(before.offsets, top);
      const canAnchor =
        changed &&
        top > 0 &&
        index < count &&
        index < before.offsets.length - 1;
      const correction = canAnchor
        ? (offsets[index] ?? 0) - (before.offsets[index] ?? 0)
        : 0;
      if (correction !== 0)
        anchor.current = {
          position: top + correction,
          scrollY: window.scrollY,
        };
      const next = windowRows(offsets, top + correction, window.innerHeight);
      if (changed || before.start !== next.start || before.end !== next.end) {
        current.current = next;
        setLayout(next);
      }
    }

    const schedule = () => {
      if (request === undefined)
        request = window.requestAnimationFrame(refresh);
    };
    if (typeof ResizeObserver !== "undefined") {
      observer.current = new ResizeObserver(schedule);
      observer.current.observe(root);
      for (const row of section.querySelectorAll("[data-row]")) {
        observer.current.observe(row);
      }
    }
    window.addEventListener("scroll", schedule, { passive: true });
    window.addEventListener("resize", schedule);
    schedule();
    return () => {
      window.removeEventListener("scroll", schedule);
      window.removeEventListener("resize", schedule);
      observer.current?.disconnect();
      observer.current = null;
      if (request !== undefined) window.cancelAnimationFrame(request);
    };
  }, [count]);

  useLayoutEffect(() => {
    const section = body.current;
    const pending = anchor.current;
    anchor.current = undefined;
    if (section && pending && window.scrollY === pending.scrollY) {
      const distance = pending.position + section.getBoundingClientRect().top;
      if (Math.abs(distance) >= 1) window.scrollBy(0, distance);
    }
  }, [layout]);

  // The page can append before the next measurement frame.
  const safeLayout =
    layout.offsets.length === count + 1
      ? layout
      : {
          ...layout,
          offsets: rowOffsets(
            count,
            sizes.current,
            width.current > 0 && width.current <= 640
              ? NARROW_ESTIMATE
              : WIDE_ESTIMATE,
          ),
          start: Math.min(layout.start, count),
          end: Math.min(layout.end, count),
        };

  return { frame, body, watchRow, layout: safeLayout };
}
