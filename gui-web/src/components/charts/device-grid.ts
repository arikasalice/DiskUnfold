import { useEffect, useLayoutEffect, useState } from "react";
import type { RefObject } from "react";

/**
 * How the chart's user units fall on device pixels: `scale` device pixels per
 * user unit, and the shift (in user units) that puts the chart's origin on a
 * device pixel. The scale is the device pixel ratio times the app scale, which
 * is a CSS zoom and shows in the drawn box against the chart's own width.
 */
export type DeviceGrid = Readonly<{ scale: number; dx: number; dy: number }>;

export const NO_GRID: DeviceGrid = { scale: 1, dx: 0, dy: 0 };

/** Measures the grid of a drawn chart `width` user units wide. */
export function measureGrid(
  svg: Element,
  width: number,
  ratio: number = globalThis.devicePixelRatio,
): DeviceGrid {
  const box = svg.getBoundingClientRect();
  const pixels = Number.isFinite(ratio) && ratio > 0 ? ratio : 1;
  const zoom = box.width > 0 && width > 0 ? box.width / width : 1;
  const scale = pixels * zoom;
  const shift = (at: number) => (Math.round(at * pixels) - at * pixels) / scale;
  return { scale, dx: shift(box.left), dy: shift(box.top) };
}

/** A position in user units moved to the nearest device pixel. */
export function snap(value: number, grid: DeviceGrid): number {
  return Math.round(value * grid.scale) / grid.scale;
}

/** A thickness of `css` user units as whole device pixels, at least one, in user units. */
export function whole(css: number, grid: DeviceGrid): number {
  return Math.max(1, Math.round(css * grid.scale)) / grid.scale;
}

/**
 * The grid of the chart drawn in `svg`, measured after every layout and on a
 * window resize, when the position, the device pixel ratio or the app scale may
 * have changed. It only re-renders when the grid changes.
 */
export function useDeviceGrid(
  svg: RefObject<Element | null>,
  width: number,
): DeviceGrid {
  const [grid, setGrid] = useState(NO_GRID);
  const [, setResized] = useState(0);
  useLayoutEffect(() => {
    if (!svg.current) return;
    const next = measureGrid(svg.current, width);
    setGrid((current) =>
      current.scale === next.scale &&
      current.dx === next.dx &&
      current.dy === next.dy
        ? current
        : next,
    );
  });
  useEffect(() => {
    const resized = () => setResized((count) => count + 1);
    globalThis.addEventListener?.("resize", resized);
    return () => globalThis.removeEventListener?.("resize", resized);
  }, []);
  return grid;
}
