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
  const correction = useRef<{ before: number; distance: number }>(undefined);
  const [positions, setPositions] = useState(
    () => new BrowseRowPositions(count, WIDE_ROW, new Map()),
  );
  const [scroll, setScroll] = useState({
    top: 0,
    viewport: window.innerHeight,
  });
  const geometry = browseScrollGeometry(positions, scroll.viewport);
  const logicalTop = geometry.logicalTop(scroll.top);
  const physicalTop = geometry.viewportTop(scroll.top);
  const edge = scroll.top - physicalTop;

  const watchRow = useCallback((element: HTMLDivElement | null) => {
    if (!element) return;
    observer.current?.observe(element);
    return () => observer.current?.unobserve(element);
  }, []);

  const updateScroll = useCallback(() => {
    if (!body.current) return;
    const top = -body.current.getBoundingClientRect().top;
    const viewport = window.innerHeight;
    setScroll((old) =>
      old.top === top && old.viewport === viewport ? old : { top, viewport },
    );
  }, []);

  function scrollDocumentBy(distance: number) {
    const before = window.scrollY;
    window.scrollBy(0, distance);
    onLayoutScroll?.(before, window.scrollY);
    updateScroll();
  }

  function reveal(index: number) {
    if (!body.current) return false;
    const from = positions.offset(index);
    const to = positions.offset(index + 1);
    const top = -body.current.getBoundingClientRect().top;
    const logical = geometry.logicalTop(top);
    const edge = top - geometry.viewportTop(top);
    const above = from < logical + Math.max(0, edge);
    if (above || to > logical + edge + scroll.viewport) {
      const target =
        above || to - from > scroll.viewport ? from : to - scroll.viewport;
      const distance = geometry.physicalTop(target) - top;
      if (Math.abs(distance) < 1) return false;
      scrollDocumentBy(distance);
      return true;
    }
    return false;
  }

  useLayoutEffect(() => {
    if (!section) return;
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
      const physical = -section.getBoundingClientRect().top;
      const top = geometry.logicalTop(physical);
      const anchor = Math.min(positions.at(top), next.count);
      const corrected = top + next.offset(anchor) - positions.offset(anchor);
      if (physical > 0) {
        correction.current = {
          before: window.scrollY,
          distance:
            browseScrollGeometry(next, scroll.viewport).physicalTop(corrected) -
            geometry.viewportTop(physical),
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
      for (const row of section.querySelectorAll("[data-row]"))
        observer.current.observe(row);
    }
    window.addEventListener("resize", resize);
    window.addEventListener("scroll", updateScroll, { passive: true });
    resize();
    return () => {
      observer.current?.disconnect();
      observer.current = undefined;
      window.removeEventListener("resize", resize);
      window.removeEventListener("scroll", updateScroll);
      if (frame !== undefined) cancelAnimationFrame(frame);
    };
  }, [count, positions, scroll.viewport, section, updateScroll]);

  useLayoutEffect(() => {
    const pending = correction.current;
    correction.current = undefined;
    if (
      pending &&
      window.scrollY === pending.before &&
      Math.abs(pending.distance) >= 1
    ) {
      scrollDocumentBy(pending.distance);
    }
  }, [positions]);

  return {
    body,
    attachBody,
    watchRow,
    reveal,
    height: geometry.height,
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
