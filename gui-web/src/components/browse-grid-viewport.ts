import { useCallback, useLayoutEffect, useRef, useState } from "react";
import { BrowseRowPositions } from "./browse-row-positions";

const WIDE_ROW = 64;
const NARROW_ROW = 160;
const MAX_MEASUREMENTS = 2048;
// Keep the native scroll extent bounded even when the registered result is huge.
const MAX_PHYSICAL_HEIGHT = 8_000_000;

function scrollRange(height: number, viewport: number) {
  return Math.max(0, height - viewport);
}

export function browseScrollGeometry(
  positions: BrowseRowPositions,
  viewport: number,
) {
  const height = Math.min(positions.height, MAX_PHYSICAL_HEIGHT);
  const physical = scrollRange(height, viewport);
  const logical = scrollRange(positions.height, viewport);
  return {
    height,
    maxLogicalTop: logical,
    viewportTop: (top: number) => Math.min(physical, Math.max(0, top)),
    logicalTop: (top: number) =>
      physical > 0
        ? Math.min(physical, Math.max(0, top)) * (logical / physical)
        : 0,
    physicalTop: (top: number) =>
      logical > 0
        ? Math.min(logical, Math.max(0, top)) * (physical / logical)
        : 0,
  };
}

export function useBrowseGridViewport(
  count: number,
  root: { current: HTMLDivElement | null },
  onLayoutScroll?: (before: number, after: number) => void,
) {
  const body = useRef<HTMLDivElement>(null);
  const [section, setSection] = useState<HTMLDivElement | null>(null);
  const attachBody = useCallback((element: HTMLDivElement | null) => {
    body.current = element;
    setSection(element);
  }, []);
  const observer = useRef<ResizeObserver | undefined>(undefined);
  const measurements = useRef(new Map<number, number>());
  const width = useRef(0);
  const correction = useRef<{
    before: number;
    physical: number;
    logical: number;
  }>(undefined);
  const logicalAnchor = useRef<{ top: number; logical: number }>(undefined);
  const [positions, setPositions] = useState(
    () => new BrowseRowPositions(count, WIDE_ROW, new Map()),
  );
  const [scroll, setScroll] = useState<{
    top: number;
    viewport: number;
    logical?: number;
  }>({
    top: 0,
    viewport: 0,
  });
  const geometry = browseScrollGeometry(positions, scroll.viewport);
  const logicalTop = Math.min(
    geometry.maxLogicalTop,
    scroll.logical ?? geometry.logicalTop(scroll.top),
  );
  const physicalTop = geometry.viewportTop(scroll.top);
  const edge = scroll.top - physicalTop;

  const watchRow = useCallback((element: HTMLDivElement | null) => {
    if (!element) return;
    observer.current?.observe(element);
    return () => observer.current?.unobserve(element);
  }, []);

  const updateScroll = useCallback(() => {
    if (!body.current) return;
    const top = (root.current?.scrollTop ?? 0) - body.current.offsetTop;
    const viewport = root.current?.clientHeight ?? 0;
    if (logicalAnchor.current?.top !== top) logicalAnchor.current = undefined;
    const logical = logicalAnchor.current?.logical;
    setScroll((old) =>
      old.top === top && old.viewport === viewport && old.logical === logical
        ? old
        : { top, viewport, ...(logical !== undefined && { logical }) },
    );
  }, [root]);

  function moveViewport(physical: number, logical: number) {
    if (!body.current) return;
    const distance =
      physical - ((root.current?.scrollTop ?? 0) - body.current.offsetTop);
    if (Math.abs(distance) >= 1) {
      const before = root.current?.scrollTop ?? 0;
      if (root.current) root.current.scrollTop = before + distance;
      onLayoutScroll?.(before, root.current?.scrollTop ?? 0);
    }
    const actual = (root.current?.scrollTop ?? 0) - body.current.offsetTop;
    // A compressed scroll range can put adjacent rows in the same native pixel.
    // Preserve the precise logical position until the list actually moves.
    logicalAnchor.current =
      Math.abs(physical - actual) < 1
        ? {
            top: actual,
            logical: Math.max(0, Math.min(logical, geometry.maxLogicalTop)),
          }
        : undefined;
    updateScroll();
  }

  function reveal(index: number) {
    if (!body.current) return false;
    const from = positions.offset(index);
    const to = positions.offset(index + 1);
    const top = (root.current?.scrollTop ?? 0) - body.current.offsetTop;
    const logical =
      logicalAnchor.current?.top === top
        ? logicalAnchor.current.logical
        : geometry.logicalTop(top);
    const edge = top - geometry.viewportTop(top);
    const above = from < logical + Math.max(0, edge);
    if (above || to > logical + edge + scroll.viewport) {
      const target =
        above || to - from > scroll.viewport ? from : to - scroll.viewport;
      moveViewport(geometry.physicalTop(target), target);
      return true;
    }
    return false;
  }

  useLayoutEffect(() => {
    if (!section) return;
    const scrollRoot = root.current;
    let frame: number | undefined;
    function measure() {
      frame = undefined;
      if (!section) return;
      const nextWidth = section.clientWidth;
      let changed = nextWidth !== width.current || positions.count !== count;
      if (nextWidth !== width.current) {
        width.current = nextWidth;
        measurements.current.clear();
      }
      for (const row of section.querySelectorAll<HTMLElement>("[data-row]")) {
        const index = Number(row.dataset.row);
        const size = row.getBoundingClientRect().height;
        if (size <= 0 || size === measurements.current.get(index)) continue;
        measurements.current.delete(index);
        measurements.current.set(index, size);
        changed = true;
      }
      for (const index of measurements.current.keys()) {
        if (measurements.current.size <= MAX_MEASUREMENTS) break;
        measurements.current.delete(index);
      }
      if (!changed) return;
      const next = new BrowseRowPositions(
        count,
        nextWidth > 0 && nextWidth <= 640 ? NARROW_ROW : WIDE_ROW,
        measurements.current,
      );
      const physical = (root.current?.scrollTop ?? 0) - section.offsetTop;
      const top =
        logicalAnchor.current?.top === physical
          ? logicalAnchor.current.logical
          : geometry.logicalTop(physical);
      const anchor = Math.min(positions.at(top), next.count);
      const corrected = top + next.offset(anchor) - positions.offset(anchor);
      if (physical > 0 || top > 0) {
        correction.current = {
          before: root.current?.scrollTop ?? 0,
          physical:
            browseScrollGeometry(next, scroll.viewport).physicalTop(corrected) -
            geometry.viewportTop(physical) +
            physical,
          logical: corrected,
        };
      }
      setPositions(next);
    }
    function schedule() {
      if (frame === undefined) frame = requestAnimationFrame(measure);
    }
    function resize() {
      updateScroll();
      schedule();
    }
    if (typeof ResizeObserver !== "undefined") {
      observer.current = new ResizeObserver(resize);
      observer.current.observe(section);
      if (root.current) observer.current.observe(root.current);
      for (const row of section.querySelectorAll("[data-row]"))
        observer.current.observe(row);
    }
    window.addEventListener("resize", resize);
    scrollRoot?.addEventListener("scroll", updateScroll, { passive: true });
    resize();
    return () => {
      observer.current?.disconnect();
      observer.current = undefined;
      window.removeEventListener("resize", resize);
      scrollRoot?.removeEventListener("scroll", updateScroll);
      if (frame !== undefined) cancelAnimationFrame(frame);
    };
  }, [count, positions, scroll.viewport, section, updateScroll, root]);

  useLayoutEffect(() => {
    const pending = correction.current;
    correction.current = undefined;
    if (pending && root.current?.scrollTop === pending.before)
      moveViewport(pending.physical, pending.logical);
  }, [positions]);
  useLayoutEffect(updateScroll);

  return {
    body,
    attachBody,
    watchRow,
    reveal,
    height: geometry.height,
    pageRows: Math.max(
      1,
      positions.at(logicalTop + scroll.viewport) - positions.at(logicalTop),
    ),
    window: (pins: readonly number[]) =>
      positions.window(
        logicalTop + Math.max(0, edge),
        logicalTop + Math.max(0, edge + scroll.viewport),
        pins,
      ),
    rowTop: (index: number) =>
      Math.max(
        positions.offset(index) - positions.offset(index + 1),
        Math.min(
          geometry.height,
          physicalTop + positions.offset(index) - logicalTop,
        ),
      ),
  };
}
